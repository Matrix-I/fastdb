use super::{grid::metadata_grid, model::TableProperties};
use gpui::*;
use std::collections::HashMap;
use ui_core::theme::Theme;
impl TableProperties {
    fn sqlite_constraint_rows(&self) -> Vec<Vec<String>> {
        let details: HashMap<_, _> = self
            .details
            .iter()
            .flat_map(|d| &d.constraints)
            .map(|c| ((c.name.as_str(), c.kind.as_str()), &c.columns))
            .collect();
        let mut primary_key = self.columns.iter().filter(|column| column.is_primary_key);
        let single_primary_key = primary_key.next().filter(|_| primary_key.next().is_none());
        self.constraints
            .iter()
            .map(|(name, kind)| {
                let columns = details
                    .get(&(name.as_str(), kind.as_str()))
                    .map(|columns| columns.join(", "))
                    .or_else(|| {
                        single_primary_key
                            .filter(|_| kind == "PRIMARY KEY")
                            .map(|column| column.name.clone())
                    })
                    .unwrap_or_else(|| "Unavailable".into());
                vec![
                    name.clone(),
                    kind.clone(),
                    columns,
                    "Unavailable".into(),
                    "See DDL".into(),
                ]
            })
            .collect()
    }

    pub(in crate::workspace) fn render_constraints(&self, theme: &Theme) -> AnyElement {
        let sqlite = self.kind == db_wire::DbDriverKind::Sqlite;
        let rows = if let Some(details) = self.constraint_details() {
            details
                .constraints
                .iter()
                .map(|c| {
                    vec![
                        c.name.clone(),
                        c.kind.clone(),
                        if c.columns.is_empty() {
                            "Unavailable".into()
                        } else {
                            c.columns.join(", ")
                        },
                        if self.kind == db_wire::DbDriverKind::MySql {
                            "Unavailable"
                        } else if c.validated {
                            "Validated"
                        } else {
                            "Not validated"
                        }
                        .into(),
                        c.definition.clone(),
                    ]
                })
                .collect()
        } else if sqlite {
            self.sqlite_constraint_rows()
        } else {
            self.constraints
                .iter()
                .map(|(name, kind)| {
                    vec![
                        name.clone(),
                        kind.clone(),
                        if kind == "PRIMARY KEY" {
                            self.columns
                                .iter()
                                .filter(|c| c.is_primary_key)
                                .map(|c| c.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        } else {
                            "Unavailable".into()
                        },
                        "Unavailable".into(),
                        "Unavailable".into(),
                    ]
                })
                .collect()
        };
        metadata_grid(
            "props-constraints-grid",
            &[
                ("Constraint Name", 220.0),
                ("Type", 130.0),
                ("Columns", 220.0),
                ("Status", 120.0),
                ("Definition", 620.0),
            ],
            rows,
            if self.constraint_details().is_some() {
                "No constraints."
            } else if sqlite {
                "No primary, unique, or foreign keys reported. See DDL for CHECK constraints."
            } else {
                "Constraints loaded: no objects found. Native definitions are unavailable."
            },
            theme,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::TableProperties;
    use db_wire::{ColumnMeta, ConstraintDetails, DataType, DbDriverKind, TableDetails};

    #[test]
    fn sqlite_constraints_use_native_order_or_a_single_primary_key_column() {
        let model = TableProperties::new(
            DbDriverKind::Sqlite,
            "main".into(),
            "items".into(),
            vec![
                ColumnMeta::new("a", DataType::Text).with_pk(true),
                ColumnMeta::new("b", DataType::Text).with_pk(true),
            ],
            vec![("PRIMARY KEY".into(), "PRIMARY KEY".into())],
            vec![],
        );
        assert_eq!(model.sqlite_constraint_rows()[0][2], "Unavailable");
        let model = model.with_metadata(
            Some(TableDetails {
                constraints: vec![ConstraintDetails {
                    name: "PRIMARY KEY".into(),
                    kind: "PRIMARY KEY".into(),
                    columns: vec!["b".into(), "a".into()],
                    definition: r#"Backing index "sqlite_autoindex_items_1""#.into(),
                    validated: false,
                }],
                ..Default::default()
            }),
            None,
        );
        let rows = model.sqlite_constraint_rows();
        assert_eq!(&rows[0][2..], ["b, a", "Unavailable", "See DDL"]);

        let mut rowid = model.with_metadata(None, None);
        rowid.columns = vec![ColumnMeta::new("id", DataType::Int4).with_pk(true)];
        assert_eq!(rowid.sqlite_constraint_rows()[0][2], "id");
    }
}
