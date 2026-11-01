use super::model::TableProperties;
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_statistics(&self, theme: &Theme) -> AnyElement {
        let mut stats_table = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .min_w(px(520.0))
            .whitespace_nowrap()
            .id("props-stats-table");

        stats_table = stats_table.child(
            div()
                .flex()
                .items_center()
                .h(px(24.0))
                .flex_shrink_0()
                .bg(theme.bg_grid_header)
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(11.0))
                .text_color(theme.text_secondary)
                .child(
                    div()
                        .w(px(220.0))
                        .flex_shrink_0()
                        .px_2()
                        .border_r_1()
                        .border_color(theme.border)
                        .child("Name"),
                )
                .child(div().flex_1().px_2().child("Value")),
        );

        for (k, v) in self.statistics_rows() {
            stats_table = stats_table.child(
                div()
                    .flex()
                    .items_center()
                    .h(px(22.0))
                    .flex_shrink_0()
                    .border_b_1()
                    .border_color(hsla(0.0, 0.0, 0.0, 0.04))
                    .text_size(px(11.5))
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .child(
                        div()
                            .w(px(220.0))
                            .flex_shrink_0()
                            .px_2()
                            .text_color(theme.text_secondary)
                            .child(k),
                    )
                    .child(
                        div()
                            .flex_1()
                            .px_2()
                            .text_color(theme.text_primary)
                            .child(v.to_string()),
                    ),
            );
        }

        stats_table.into_any_element()
    }
}
