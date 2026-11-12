use super::{grid::metadata_grid, model::TableProperties};
use db_wire::DbDriverKind;
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_indexes(&self, theme: &Theme) -> AnyElement {
        let details = self.index_details();
        let rows = if let Some(details) = details {
            details
                .indexes
                .iter()
                .map(|i| {
                    vec![
                        i.name.clone(),
                        i.method.clone(),
                        i.columns.join(", "),
                        if i.unique { "Yes" } else { "No" }.into(),
                        index_status(self.kind, i.valid).into(),
                        i.definition.clone(),
                    ]
                })
                .collect()
        } else {
            self.indexes
                .iter()
                .map(|name| {
                    vec![
                        name.clone(),
                        "Unavailable".into(),
                        "Unavailable".into(),
                        "Unavailable".into(),
                        "Unavailable".into(),
                        "Unavailable".into(),
                    ]
                })
                .collect()
        };
        metadata_grid(
            "props-indexes-grid",
            &[
                ("Index Name", 220.0),
                ("Method", 85.0),
                ("Keys / INCLUDE columns", 250.0),
                ("Unique", 70.0),
                ("Status", 80.0),
                ("Definition (including predicate)", 620.0),
            ],
            rows,
            if details.is_some() {
                "No indexes."
            } else {
                "Indexes loaded: no objects found. Native definitions are unavailable."
            },
            theme,
        )
    }
}

fn index_status(kind: DbDriverKind, valid: bool) -> &'static str {
    match kind {
        DbDriverKind::Sqlite | DbDriverKind::MySql => "Unavailable",
        _ if valid => "Valid",
        _ => "Invalid",
    }
}

#[cfg(test)]
mod tests {
    use super::{DbDriverKind, index_status};

    #[test]
    fn mysql_index_details_do_not_imply_validation() {
        assert_eq!(index_status(DbDriverKind::MySql, false), "Unavailable");
        assert_eq!(index_status(DbDriverKind::Postgres, false), "Invalid");
    }
}
