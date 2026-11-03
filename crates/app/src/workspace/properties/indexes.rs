use super::{grid::metadata_grid, model::TableProperties};
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_indexes(&self, theme: &Theme) -> AnyElement {
        let rows = if let Some(details) = &self.details {
            details
                .indexes
                .iter()
                .map(|i| {
                    vec![
                        i.name.clone(),
                        i.method.clone(),
                        i.columns.join(", "),
                        if i.unique { "Yes" } else { "No" }.into(),
                        if i.valid { "Valid" } else { "Invalid" }.into(),
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
            if self.details.is_some() {
                "No indexes."
            } else {
                "Indexes loaded: no objects found. Native definitions are unavailable."
            },
            theme,
        )
    }
}
