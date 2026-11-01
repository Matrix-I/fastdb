use super::model::TableProperties;
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_foreignkeys(&self, theme: &Theme) -> AnyElement {
        let mut grid = div()
            .id("props-foreign-keys-grid")
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .overflow_y_scroll()
            .text_size(px(11.5))
            .text_color(theme.text_primary);
        let names: Vec<_> = self
            .constraints
            .iter()
            .filter(|(_, kind)| kind.to_uppercase().contains("FOREIGN"))
            .collect();
        if names.is_empty() {
            grid = grid.child(
                div()
                    .p_4()
                    .text_color(theme.text_muted)
                    .child("Foreign key definitions are unavailable or empty."),
            );
        }
        for (name, _) in names {
            grid = grid.child(
                div().flex().gap_3().p_2().child(name.clone()).child(
                    div()
                        .text_color(theme.text_muted)
                        .child("Referenced table, columns and actions: Unavailable"),
                ),
            );
        }
        grid.into_any_element()
    }
}
