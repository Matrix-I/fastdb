use gpui::*;
use ui_core::theme::Theme;

pub(super) fn metadata_grid(
    id: &'static str,
    headers: &[(&str, f32)],
    rows: Vec<Vec<String>>,
    empty: &str,
    theme: &Theme,
) -> AnyElement {
    let mut grid = div()
        .id(id)
        .flex()
        .flex_col()
        .flex_1()
        .h_full()
        .overflow_y_scroll()
        .overflow_x_scroll()
        .text_size(px(11.5));
    let mut header = div()
        .flex()
        .min_h(px(24.0))
        .flex_shrink_0()
        .bg(theme.bg_grid_header)
        .text_color(theme.text_secondary);
    for (label, width) in headers {
        header = header.child(
            div()
                .w(px(*width))
                .flex_shrink_0()
                .px_2()
                .py_1()
                .child(label.to_string()),
        );
    }
    grid = grid.child(header);
    if rows.is_empty() {
        grid = grid.child(
            div()
                .p_4()
                .text_color(theme.text_muted)
                .child(empty.to_string()),
        );
    }
    for row in rows {
        let mut view = div()
            .flex()
            .flex_shrink_0()
            .min_h(px(26.0))
            .border_b_1()
            .border_color(theme.border)
            .text_color(theme.text_primary)
            .hover(|s| s.bg(theme.bg_surface_hover));
        for (value, (_, width)) in row.into_iter().zip(headers) {
            view = view.child(
                div()
                    .w(px(*width))
                    .flex_shrink_0()
                    .px_2()
                    .py_1()
                    .child(value),
            );
        }
        grid = grid.child(view);
    }
    grid.into_any_element()
}
