use super::*;

pub(super) fn render_field_content(
    value: &str,
    placeholder: &str,
    is_active: bool,
    is_selected_all: bool,
    cursor_visible: bool,
    theme: &Theme,
) -> Div {
    let mut row = div().flex().items_center().h(px(18.0)).w_full().overflow_hidden();

    if value.is_empty() {
        if is_active {
            row = row
                .child(
                    div()
                        .w(px(1.5))
                        .h(px(14.0))
                        .rounded_xs()
                        .bg(if cursor_visible { rgb(0x58a6ff) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                )
                .child(
                    div()
                        .pl_1()
                        .text_color(theme.text_muted)
                        .child(placeholder.to_string())
                );
        } else {
            row = row.child(
                div()
                    .text_color(theme.text_muted)
                    .child(placeholder.to_string())
            );
        }
    } else {
        if is_active && is_selected_all {
            row = row.child(
                div()
                    .px_1()
                    .py_0p5()
                    .rounded_xs()
                    .bg(rgb(0x264f78))
                    .text_color(rgb(0xffffff))
                    .child(value.to_string())
            );
        } else if is_active {
            row = row
                .child(
                    div()
                        .text_color(theme.text_primary)
                        .child(value.to_string())
                )
                .child(
                    div()
                        .pl(px(1.0))
                        .w(px(1.5))
                        .h(px(14.0))
                        .rounded_xs()
                        .bg(if cursor_visible { rgb(0x58a6ff) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                );
        } else {
            row = row.child(
                div()
                    .text_color(theme.text_primary)
                    .child(value.to_string())
            );
        }
    }

    row
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
