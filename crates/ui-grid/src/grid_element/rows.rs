use super::*;
impl VirtualDataGrid {
    pub(super) fn render_rows(
        &self,
        total_width: f32,
        on_cell_mouse_down: Option<CellMouseDownHandler>,
        cell_editor: Option<(u64, usize, CellEditorRenderer)>,
    ) -> AnyElement {
        let theme = &self.theme;
        let row_height = theme.row_height;
        let gutter_width = GUTTER_WIDTH;
        let total_rows = self.row_count();
        // 2. Data Rows with full virtualization using uniform_list
        let total_rows_usize = total_rows as usize;
        let vertical_scroll_handle = self.vertical_scroll_handle.clone();
        let pager_clone = self.pager.clone();
        let column_widths_clone = self.column_widths.clone();
        let selection_clone = self.selection.clone();
        let changeset_clone = self.changeset.clone();
        let theme_bg_grid_alt = theme.bg_grid_alt;
        let theme_bg_grid = theme.bg_grid;
        let theme_border = theme.border;
        let theme_text_muted = theme.text_muted;
        let theme_text_primary = theme.text_primary;
        let theme_bg_cell_selected = theme.bg_grid_cell_selected;
        let theme_bg_cell_modified = theme.bg_grid_cell_modified;
        let theme_border_focused = theme.border_focused;
        let theme_bg_editor = theme.bg_editor;
        let theme_text_success = theme.text_success;
        let theme_text_error = theme.text_error;
        let scroll_x = -f32::from(self.horizontal_scroll_handle.offset().x);
        let (visible_cols, cols_offset) =
            visible_columns(&self.column_widths, scroll_x, self.viewport_width);

        let body = div()
            .id("grid-data-body-container")
            .flex()
            .flex_col()
            .flex_1()
            .w(px(total_width))
            .h_full()
            .overflow_hidden()
            .child(
                uniform_list(
                    "grid-data-body",
                    total_rows_usize,
                    move |range: std::ops::Range<usize>, _window: &mut Window, _cx: &mut App| {
                        let mut rows = Vec::with_capacity(range.len());
                        for r in range {
                            let r_u64 = r as u64;
                            let is_alt = r % 2 == 1;
                            let row_ref = changeset_clone.resolve_row(r_u64);
                            let is_new = matches!(row_ref, RowRef::New(_));
                            let is_deleted = matches!(row_ref, RowRef::Fetched(f) if changeset_clone.is_deleted(f));
                            let row_bg = if is_new {
                                Rgba { a: 0.14, ..theme_text_success }
                            } else if is_deleted {
                                Rgba { a: 0.16, ..theme_text_error }
                            } else if is_alt {
                                theme_bg_grid_alt
                            } else {
                                theme_bg_grid
                            };

                            let mut row_div = div()
                                .id(("grid-row", r))
                                .flex()
                                .flex_row()
                                .flex_none()
                                .w(px(total_width))
                                .h(px(row_height))
                                .bg(row_bg)
                                .border_b_1()
                                .border_color(theme_border)
                                .child(
                                    // Gutter row index
                                    div()
                                        .w(px(gutter_width))
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .border_r_1()
                                        .border_color(theme_border)
                                        .text_size(px(10.0))
                                        .text_color(if is_new { theme_text_success } else if is_deleted { theme_text_error } else { theme_text_muted })
                                        .child(match row_ref {
                                            RowRef::New(_) => "+".to_string(),
                                            RowRef::Fetched(f) => format!("{}", f + 1),
                                        })
                                )
                                .child(div().flex_none().w(px(cols_offset)).h_full());

                            for c in visible_cols.clone() {
                                let width = column_widths_clone.get(c).copied().unwrap_or(MIN_COL_WIDTH);
                                let is_selected = selection_clone.is_selected(r_u64, c);
                                let is_modified = matches!(row_ref, RowRef::Fetched(f) if changeset_clone.is_modified(f, c));

                                let mut cell_div = div()
                                    .flex()
                                    .items_center()
                                    .w(px(width))
                                    .h_full()
                                    .px(px(CELL_PADDING_X))
                                    .border_r_1()
                                    .border_color(theme_border)
                                    .text_size(px(12.0))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis();

                                if let Some(handler) = on_cell_mouse_down.clone() {
                                    cell_div = cell_div.on_mouse_down(MouseButton::Left, move |event, window, cx| {
                                        handler(r_u64, c, event, window, cx);
                                    });
                                }

                                if let Some((_, _, render_editor)) = cell_editor.as_ref().filter(|(er, ec, _)| *er == r_u64 && *ec == c) {
                                    cell_div = cell_div
                                        .bg(theme_bg_editor)
                                        .border_1()
                                        .border_color(theme_border_focused)
                                        .text_color(theme_text_primary)
                                        .child(render_editor());
                                    row_div = row_div.child(cell_div);
                                    continue;
                                }

                                if is_selected {
                                    cell_div = cell_div.bg(theme_bg_cell_selected);
                                } else if is_modified {
                                    cell_div = cell_div.bg(theme_bg_cell_modified);
                                }

                                let value = pager_clone.as_ref().and_then(|p| resolve_cell(p, &changeset_clone, r_u64, c));
                                let (display_text, is_null) = match value {
                                    Some(s) => (clip_cell_text(s, width), false),
                                    None => ("NULL".to_string(), true),
                                };

                                // The cell itself clips the text with an ellipsis; an inner element
                                // per cell would double what is laid out on every scrolled frame
                                if is_deleted {
                                    cell_div = cell_div.line_through();
                                }
                                if is_null || is_deleted {
                                    cell_div = cell_div.text_color(theme_text_muted);
                                }
                                cell_div = cell_div.child(display_text);

                                row_div = row_div.child(cell_div);
                            }

                            rows.push(row_div);
                        }
                        rows
                    },
                )
                .size_full()
                .track_scroll(vertical_scroll_handle)
            );

        body.into_any_element()
    }
}
