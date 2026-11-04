use super::*;
impl VirtualDataGrid {
    pub(super) fn render_header<F>(
        &self,
        total_width: f32,
        resizing_col: Option<usize>,
        on_header_menu: Option<HeaderMenuHandler>,
        mut make_resizer: F,
    ) -> AnyElement
    where
        F: FnMut(usize, f32) -> AnyElement,
    {
        let theme = &self.theme;
        let header_height = theme.header_height;
        let gutter_width = GUTTER_WIDTH;
        // 1. Sticky Header Row
        let mut header_row = div()
            .id("grid-header-row")
            .flex()
            .flex_none()
            .w(px(total_width))
            .h(px(header_height))
            .bg(theme.bg_grid_header)
            .border_b_1()
            .border_color(theme.border)
            .child(
                // Gutter header corner
                div()
                    .w(px(gutter_width))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_r_1()
                    .border_color(theme.border)
                    .text_size(px(11.0))
                    .text_color(theme.text_muted)
                    .child("#"),
            );

        if let Some(ref pager) = self.pager {
            for &col_idx in self.display_columns() {
                let col = &pager.columns[col_idx];
                let width = self
                    .column_widths
                    .get(col_idx)
                    .copied()
                    .unwrap_or(MIN_COL_WIDTH);
                let is_resizing = resizing_col == Some(col_idx);
                let resizer = make_resizer(col_idx, width);
                let badge_str = dbeaver_type_badge(col.data_type);
                let is_pk = col.is_primary_key || col.name.eq_ignore_ascii_case("id");
                let sort_glyph = self
                    .ordering
                    .iter()
                    .find(|o| o.column == col.name)
                    .map(|o| if o.descending { "↓" } else { "↑" });
                let mut arrow = div()
                    .id(("header-menu-arrow", col_idx))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(HEADER_ARROW_WIDTH))
                    .h(px(14.0))
                    .rounded_xs()
                    .text_size(px(8.5))
                    .text_color(rgb(0x58a6ff))
                    .child("▼");
                if let Some(handler) = on_header_menu.clone() {
                    arrow = arrow
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.bg_surface_hover))
                        .on_mouse_down(MouseButton::Left, move |event, window, app| {
                            app.stop_propagation();
                            handler(col_idx, event, window, app);
                        });
                }

                header_row = header_row.child(
                    div()
                        .id(("header-col", col_idx))
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .w(px(width))
                        .h_full()
                        .border_r_1()
                        .border_color(if is_resizing {
                            theme.border_focused
                        } else {
                            theme.border
                        })
                        .child(
                            // Badge, key and arrow keep their size; only the name shrinks,
                            // ending in an ellipsis once the column is too narrow for it.
                            div()
                                .flex()
                                .flex_1()
                                .min_w_0()
                                .items_center()
                                // Spacing in px: gpui's `gap_1_5` is 20% of the width, not Tailwind's 6px
                                .gap(px(HEADER_GAP))
                                .h_full()
                                .px(px(HEADER_PADDING_X))
                                .overflow_hidden()
                                .child(
                                    div()
                                        .flex_none()
                                        .font_weight(FontWeight::BOLD)
                                        .text_size(px(10.0))
                                        .text_color(rgb(0x58a6ff))
                                        .child(badge_str),
                                )
                                .children(is_pk.then(|| {
                                    img("icons/key.svg")
                                        .flex_none()
                                        .w(px(HEADER_KEY_WIDTH))
                                        .h(px(11.0))
                                }))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_size(px(12.0))
                                        .text_color(theme.text_primary)
                                        .truncate()
                                        .child(col.name.clone()),
                                )
                                .children(sort_glyph.map(|g| {
                                    div()
                                        .flex_none()
                                        .font_weight(FontWeight::BOLD)
                                        .text_size(px(12.0))
                                        .text_color(rgb(0x58a6ff))
                                        .child(g)
                                }))
                                .child(arrow),
                        )
                        .child(resizer),
                );
            }
        }

        header_row.into_any_element()
    }
}
