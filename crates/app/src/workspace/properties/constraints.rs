use super::{grid::metadata_grid, model::TableProperties};
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_constraints(&self, theme: &Theme) -> AnyElement {
        let rows = if let Some(details) = &self.details {
            details
                .constraints
                .iter()
                .map(|c| {
                    vec![
                        c.name.clone(),
                        c.kind.clone(),
                        c.columns.join(", "),
                        if c.validated {
                            "Validated"
                        } else {
                            "Not validated"
                        }
                        .into(),
                        c.definition.clone(),
                    ]
                })
                .collect()
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
            if self.details.is_some() {
                "No constraints."
            } else {
                "Constraint metadata is unavailable or empty."
            },
            theme,
        )
    }
}
