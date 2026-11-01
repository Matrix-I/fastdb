use db_wire::{ColumnMeta, DbDriverKind, sql::generate_table_ddl};
#[derive(Clone)]
pub(in crate::workspace) struct TableProperties {
    pub schema: String,
    pub table: String,
    pub columns: Vec<ColumnMeta>,
    pub constraints: Vec<(String, String)>,
    pub indexes: Vec<String>,
    pub ddl: String,
    pub details: Option<db_wire::TableDetails>,
    pub metadata_error: Option<String>,
}
impl TableProperties {
    pub fn statistics_rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = vec![
            ("Table Name", self.table.clone()),
            ("Schema", self.schema.clone()),
            ("Columns Count", self.columns.len().to_string()),
        ];
        let d = self.details.as_ref();
        let text = |value: Option<&String>| value.cloned().unwrap_or_else(|| "Unavailable".into());
        let flag = |value: Option<bool>| {
            match value {
                Some(true) => "Yes",
                Some(false) => "No",
                None => "Unavailable",
            }
            .to_string()
        };
        let number = |value: Option<i64>| {
            value
                .map(|v| v.to_string())
                .unwrap_or_else(|| "Unavailable".into())
        };
        rows.extend([
            ("Object ID", text(d.and_then(|d| d.object_id.as_ref()))),
            ("Owner", text(d.and_then(|d| d.owner.as_ref()))),
            ("Tablespace", text(d.and_then(|d| d.tablespace.as_ref()))),
            (
                "Comment",
                d.map(|d| d.comment.clone().unwrap_or_else(|| "None".into()))
                    .unwrap_or_else(|| "Unavailable".into()),
            ),
            (
                "Has Primary Key",
                if let Some(d) = d {
                    flag(Some(d.constraints.iter().any(|c| c.kind == "PRIMARY KEY")))
                } else if self.columns.is_empty() {
                    "Unavailable".into()
                } else {
                    flag(Some(self.columns.iter().any(|c| c.is_primary_key)))
                },
            ),
            ("Row-Level Security", flag(d.and_then(|d| d.row_security))),
            (
                "Force Row-Level Security",
                flag(d.and_then(|d| d.force_row_security)),
            ),
            ("Partitioned", flag(d.and_then(|d| d.partitioned))),
            (
                "Partition Key",
                text(d.and_then(|d| d.partition_key.as_ref())),
            ),
            ("Persistence", text(d.and_then(|d| d.persistence.as_ref()))),
            (
                "Estimated Rows (ANALYZE)",
                number(d.and_then(|d| d.estimated_rows)),
            ),
            (
                "Table + TOAST (bytes)",
                number(d.and_then(|d| d.table_bytes)),
            ),
            ("Indexes (bytes)", number(d.and_then(|d| d.index_bytes))),
            (
                "Total Relation (bytes)",
                number(d.and_then(|d| d.total_bytes)),
            ),
            ("Last Vacuum", text(d.and_then(|d| d.last_vacuum.as_ref()))),
            (
                "Last Autovacuum",
                text(d.and_then(|d| d.last_autovacuum.as_ref())),
            ),
            (
                "Last Analyze",
                text(d.and_then(|d| d.last_analyze.as_ref())),
            ),
            (
                "Last Autoanalyze",
                text(d.and_then(|d| d.last_autoanalyze.as_ref())),
            ),
        ]);
        if let Some(error) = &self.metadata_error {
            rows.push(("Metadata Error", error.clone()));
        }
        rows
    }

    pub fn new(
        kind: DbDriverKind,
        schema: String,
        table: String,
        columns: Vec<ColumnMeta>,
        constraints: Vec<(String, String)>,
        indexes: Vec<String>,
    ) -> Self {
        let ddl = generate_table_ddl(kind, &schema, &table, &columns, &constraints, &indexes);
        Self {
            schema,
            table,
            columns,
            constraints,
            indexes,
            ddl,
            details: None,
            metadata_error: None,
        }
    }

    pub fn with_metadata(
        mut self,
        details: Option<db_wire::TableDetails>,
        error: Option<String>,
    ) -> Self {
        if let Some(details) = &details {
            self.ddl = self
                .ddl
                .lines()
                .filter(|line| {
                    !line.starts_with("-- Constraint metadata:")
                        && !line.starts_with("-- Index metadata:")
                })
                .map(|line| format!("{line}\n"))
                .collect();
            for constraint in &details.constraints {
                self.ddl.push_str(&format!(
                    "-- Catalog constraint {}: {}\n",
                    constraint.name.replace(['\r', '\n'], " "),
                    constraint.definition.replace(['\r', '\n'], " ")
                ));
            }
            for index in &details.indexes {
                self.ddl.push_str(&format!(
                    "-- Catalog index: {}\n",
                    index.definition.replace(['\r', '\n'], " ")
                ));
            }
        }
        self.details = details;
        self.metadata_error = error;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::TableProperties;
    use db_wire::{ColumnMeta, DataType, DbDriverKind};

    #[test]
    fn ddl_preview_includes_available_definitions_as_safe_comments() {
        let model = TableProperties::new(
            DbDriverKind::Postgres,
            "public".into(),
            "items".into(),
            vec![ColumnMeta::new("id", DataType::Int4).with_raw_type("integer")],
            vec![("positive".into(), "CHECK".into())],
            vec!["idx".into()],
        )
        .with_metadata(
            Some(db_wire::TableDetails {
                constraints: vec![db_wire::ConstraintDetails {
                    name: "positive\nname".into(),
                    kind: "CHECK".into(),
                    columns: vec!["id".into()],
                    definition: "CHECK (id > 0)\nNOT VALID".into(),
                    validated: false,
                }],
                indexes: vec![db_wire::IndexDetails {
                    name: "idx".into(),
                    method: "btree".into(),
                    columns: vec!["id".into()],
                    unique: false,
                    valid: true,
                    definition: "CREATE INDEX idx\nON public.items (id)".into(),
                }],
                ..Default::default()
            }),
            None,
        );
        assert!(!model.ddl.contains("definition unavailable"));
        assert!(
            model
                .ddl
                .contains("-- Catalog constraint positive name: CHECK (id > 0) NOT VALID")
        );
        assert!(
            model
                .ddl
                .contains("-- Catalog index: CREATE INDEX idx ON public.items (id)")
        );
        assert!(!model.ddl.lines().any(|line| line.starts_with("ON public")));
    }

    #[test]
    fn statistics_preserve_unknown_values_and_show_metadata_errors() {
        let model = TableProperties::new(
            DbDriverKind::Postgres,
            "public".into(),
            "items".into(),
            vec![],
            vec![],
            vec![],
        );
        let values: std::collections::HashMap<_, _> = model.statistics_rows().into_iter().collect();
        assert_eq!(values["Estimated Rows (ANALYZE)"], "Unavailable");
        assert_eq!(values["Row-Level Security"], "Unavailable");
        assert_eq!(values["Has Primary Key"], "Unavailable");
        let model = model.with_metadata(
            Some(db_wire::TableDetails {
                estimated_rows: Some(0),
                row_security: Some(false),
                table_bytes: Some(0),
                ..Default::default()
            }),
            None,
        );
        let values: std::collections::HashMap<_, _> = model.statistics_rows().into_iter().collect();
        assert_eq!(values["Estimated Rows (ANALYZE)"], "0");
        assert_eq!(values["Row-Level Security"], "No");
        assert_eq!(values["Table + TOAST (bytes)"], "0");
        assert_eq!(values["Last Analyze"], "Unavailable");
        let failed_refresh = model.with_metadata(None, Some("Permission denied".into()));
        let values: std::collections::HashMap<_, _> =
            failed_refresh.statistics_rows().into_iter().collect();
        assert_eq!(values["Estimated Rows (ANALYZE)"], "Unavailable");
        assert_eq!(values["Metadata Error"], "Permission denied");
    }
    #[test]
    fn unavailable_metadata_does_not_create_sample_columns_or_fake_definitions() {
        let empty = TableProperties::new(
            DbDriverKind::Postgres,
            "public".into(),
            "additionalInfo".into(),
            vec![],
            vec![],
            vec![],
        );
        assert!(empty.columns.is_empty());
        assert!(empty.indexes.is_empty());
        assert!(empty.constraints.is_empty());
        assert!(empty.ddl.contains("unavailable"));
        assert!(!empty.ddl.contains("CREATE TABLE"));
        let known = TableProperties::new(
            DbDriverKind::Postgres,
            "other".into(),
            "additionalInfo".into(),
            vec![ColumnMeta::new("real_column", DataType::Text).with_raw_type("text")],
            vec![],
            vec![],
        );
        assert!(known.ddl.contains("real_column"));
        assert!(!known.ddl.contains("ubiid"));
        assert!(empty.columns.is_empty());
    }
}
