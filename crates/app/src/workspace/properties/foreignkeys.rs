use super::{grid::metadata_grid, model::TableProperties};
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_foreignkeys(&self, theme: &Theme) -> AnyElement {
        let rows = if let Some(details) = &self.details {
            details
                .constraints
                .iter()
                .filter(|c| c.kind == "FOREIGN KEY")
                .map(|c| vec![c.name.clone(), c.columns.join(", "), c.definition.clone()])
                .collect()
        } else {
            self.constraints
                .iter()
                .filter(|(_, kind)| kind == "FOREIGN KEY")
                .map(|(name, _)| {
                    vec![
                        name.clone(),
                        "Unavailable".into(),
                        "Referenced table, columns and actions: Unavailable".into(),
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
            if self.details.is_some() {
                "No foreign keys."
            } else {
                "Foreign key definitions are unavailable or empty."
            },
            theme,
        )
    }
}
