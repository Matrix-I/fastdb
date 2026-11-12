use super::{grid::metadata_grid, model::TableProperties};
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_foreignkeys(&self, theme: &Theme) -> AnyElement {
        let details = self.foreign_key_details();
        let rows = if let Some(details) = details {
            details
                .constraints
                .iter()
                .filter(|c| c.kind == "FOREIGN KEY")
                .map(|c| vec![c.name.clone(), c.columns.join(", "), c.definition.clone()])
                .collect()
        } else {
            self.foreign_keys
                .iter()
                .map(|key| {
                    vec![
                        key.name.clone(),
                        key.source_column.clone(),
                        format!(
                            "{}.{} ({}) · actions/deferrability unavailable",
                            key.target_schema, key.target_table, key.target_column
                        ),
                    ]
                })
                .collect()
        };
        metadata_grid(
            "props-foreign-keys-grid",
            &[
                ("Foreign Key", 220.0),
                ("Columns", 220.0),
                ("Definition (reference, actions, deferrability)", 760.0),
            ],
            rows,
            if details.is_some() || self.kind == db_wire::DbDriverKind::Sqlite {
                "No foreign keys."
            } else {
                "Foreign keys loaded: no objects found. Native definitions/actions are unavailable."
            },
            theme,
        )
    }
}
