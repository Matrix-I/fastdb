use super::model::TableProperties;
use gpui::*;
use ui_core::theme::Theme;
impl TableProperties {
    pub(in crate::workspace) fn render_constraints(&self, theme: &Theme) -> AnyElement {
        let columns_vec = &self.columns;
        let constraints_vec = &self.constraints;

        let mut grid = div()
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .id("props-constraints-grid")
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
                        .child("Constraint Name"),
                )
                .child(
                    div()
                        .w(px(140.0))
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
                .child(div().flex_1().px_2().child("Status")),
        );

        let pk_cols: Vec<String> = columns_vec
            .iter()
            .filter(|c| c.is_primary_key)
            .map(|c| c.name.clone())
            .collect();
        let effective_constraints = constraints_vec;
        if effective_constraints.is_empty() {
            grid = grid.child(
                div()
                    .p_4()
                    .text_size(px(11.5))
                    .text_color(theme.text_muted)
                    .child("Constraint metadata is unavailable or empty."),
            );
        } else {
            for (cname, ctype) in effective_constraints {
                let cols_str = if ctype.contains("PRIMARY KEY") {
                    if !pk_cols.is_empty() {
                        pk_cols.join(", ")
                    } else {
                        "Unavailable".to_string()
                    }
                } else {
                    "-".to_string()
                };

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
                                .child(img("icons/constraint.svg").w(px(13.0)).h(px(13.0)))
                                .child(div().text_color(theme.text_primary).child(cname.clone())),
                        )
                        .child(
                            div()
                                .w(px(140.0))
                                .px_2()
                                .text_color(theme.text_secondary)
                                .child(ctype.clone()),
                        )
                        .child(
                            div()
                                .w(px(160.0))
                                .px_2()
                                .text_color(rgb(0x58a6ff))
                                .child(cols_str),
                        )
                        .child(
                            div()
                                .flex_1()
                                .px_2()
                                .text_color(theme.text_success)
                                .child("Unavailable"),
                        ),
                );
            }
        }

        grid.into_any_element()
    }
}
