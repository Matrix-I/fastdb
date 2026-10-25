use super::*;

/// Contents of a single-line input: placeholder, the value with a caret at `caret`
/// (a char index) while active, or the whole value highlighted when selected.
/// Clicking a character reports the caret position through `on_pick`.
#[allow(clippy::too_many_arguments)]
pub(super) fn render_field_content(
    value: &str,
    placeholder: &str,
    is_active: bool,
    is_selected_all: bool,
    caret: usize,
    cursor_visible: bool,
    theme: &Theme,
    on_pick: &CaretPick,
) -> Div {
    let row = div().flex().items_center().h(px(18.0)).w_full().overflow_hidden();

    if value.is_empty() {
        let placeholder_el = div().text_color(theme.text_muted).child(placeholder.to_string());
        if is_active {
            row.children(render_caret_text("", Some(0), cursor_visible, on_pick))
                .child(placeholder_el.pl_1())
        } else {
            row.child(placeholder_el)
        }
    } else if is_active && is_selected_all {
        row.child(
            div()
                .flex()
                .items_center()
                .px_1()
                .py_0p5()
                .rounded_xs()
                .bg(rgb(0x264f78))
                .text_color(rgb(0xffffff))
                .children(render_caret_text(value, None, false, on_pick)),
        )
    } else {
        let caret = is_active.then_some(caret);
        row.text_color(theme.text_primary)
            .children(render_caret_text(value, caret, cursor_visible, on_pick))
    }
}

pub(super) fn dbeaver_driver_icon(driver: &str, is_connected: bool) -> Div {
    let icon_path = match driver {
        "MySql" | "mysql" => "icons/mysql.png",
        "Sqlite" | "sqlite" => "icons/sqlite.png",
        _ => "icons/postgresql.png",
    };

    div()
        .relative()
        .w(px(16.0))
        .h(px(16.0))
        .flex_shrink_0()
        .child(
            img(icon_path)
                .w(px(16.0))
                .h(px(16.0))
        )
        .child(
            if is_connected {
                div()
                    .absolute()
                    .bottom(px(-1.0))
                    .left(px(-1.0))
                    .w(px(7.0))
                    .h(px(8.0))
                    .child(
                        img("icons/success_ovr.png")
                            .w(px(7.0))
                            .h(px(8.0))
                    )
                    .into_any_element()
            } else {
                div().into_any_element()
            }
        )
}
