use super::model::TableProperties;
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_indexes(&self, theme: &Theme) -> AnyElement {
        let indexes_vec = &self.indexes;

        let mut grid = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .id("props-indexes-grid")
            .overflow_y_scroll();

        grid = grid.child(
            div()
                .flex()
                .items_center()
                .h(px(24.0))
                .bg(theme.bg_grid_header)
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(11.0))
                .text_color(theme.text_secondary)
                .child(
                    div()
                        .w(px(240.0))
                        .px_2()
                        .border_r_1()
                        .border_color(theme.border)
                        .child("Index Name"),
                )
                .child(
                    div()
                        .w(px(100.0))
                        .px_2()
                        .border_r_1()
                        .border_color(theme.border)
                        .child("Type"),
                )
                .child(
                    div()
                        .w(px(160.0))
                        .px_2()
                        .border_r_1()
                        .border_color(theme.border)
                        .child("Columns"),
                )
                .child(div().flex_1().px_2().child("Unique")),
        );

        let effective_indexes = indexes_vec;
        if effective_indexes.is_empty() {
            grid = grid.child(
                div()
                    .p_4()
                    .text_size(px(11.5))
                    .text_color(theme.text_muted)
                    .child("Index metadata is unavailable or empty."),
            );
        } else {
            for iname in effective_indexes {
                grid = grid.child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(22.0))
                        .border_b_1()
                        .border_color(hsla(0.0, 0.0, 0.0, 0.04))
                        .text_size(px(11.5))
                        .hover(|s| s.bg(theme.bg_surface_hover))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .w(px(240.0))
                                .px_2()
                                .child(img("icons/table_index.svg").w(px(13.0)).h(px(13.0)))
                                .child(div().text_color(theme.text_primary).child(iname.clone())),
                        )
                        .child(
                            div()
                                .w(px(100.0))
                                .px_2()
                                .text_color(theme.text_secondary)
                                .child("Unavailable"),
                        )
                        .child(
                            div()
                                .w(px(160.0))
                                .px_2()
                                .text_color(rgb(0x58a6ff))
                                .child("Unavailable"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .px_2()
                                .text_color(theme.text_muted)
                                .child("Unavailable"),
                        ),
                );
            }
        }

        grid.into_any_element()
    }
}
