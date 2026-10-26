use super::*;

impl Workspace {
    /// Data sub-tab: result grid or text view, with the optional record side panel
    pub(super) fn render_data_panel(&mut self, active_tab_idx: usize, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
        let tab = &self.tabs[active_tab_idx];
        let view_mode = tab.view_mode;
        let show_right_panels = self.show_right_panels;
        let mut data_row = div()
            .flex()
            .flex_row()
            .flex_1()
            .w_full()
            .overflow_hidden();

        // Left vertical toolstrip (Grid / Text)
        data_row = data_row.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .w(px(26.0))
                .h_full()
                .bg(theme.bg_surface)
                .border_r_1()
                .border_color(theme.border)
                .py_2()
                .gap_2()
                .child({
                    let is_grid = view_mode == DataViewMode::Grid;
                    div()
                        .id("toolstrip-grid")
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .w(px(22.0))
                        .py_2()
                        .rounded_xs()
                        .bg(if is_grid { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                        .border_l_2()
                        .border_color(if is_grid { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                t.view_mode = DataViewMode::Grid;
                                cx.notify();
                            }
                        }))
                        .child(img("icons/grid.svg").w(px(14.0)).h(px(14.0)))
                })
                .child({
                    let is_text = view_mode == DataViewMode::Text;
                    div()
                        .id("toolstrip-text")
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .w(px(22.0))
                        .py_2()
                        .rounded_xs()
                        .bg(if is_text { theme.bg_surface_selected } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                        .border_l_2()
                        .border_color(if is_text { rgb(0x388bfd) } else { hsla(0.0, 0.0, 0.0, 0.0).into() })
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                t.view_mode = DataViewMode::Text;
                                cx.notify();
                            }
                        }))
                        .child(img("icons/sql_script.svg").w(px(14.0)).h(px(14.0)))
                })
        );

        // Center Grid / Text
        data_row = data_row.child(
            div()
                .id("center-data-container")
                .flex()
                .flex_col()
                .flex_1()
                .h_full()
                .overflow_hidden()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.is_filter_focused = false;
                    this.blur_fetch_size();
                    cx.notify();
                }))
                .child({
                    match view_mode {
                        DataViewMode::Grid => {
                            let sidebar_w = self.navigator_width;
                            let panels_w = if show_right_panels { self.value_panel_width } else { 0.0 };
                            let available_viewport_w = (f32::from(window.viewport_size().width) - sidebar_w - CENTER_CHROME_WIDTH - panels_w).max(200.0);
                            let available_viewport_h = (f32::from(window.viewport_size().height) - 180.0).max(200.0);
                            let is_running_query = self.is_running_query;
                            let tab = &mut self.tabs[active_tab_idx];
                            tab.grid.update_viewport_width(available_viewport_w);
                            tab.grid.update_viewport_height(available_viewport_h);
                            tab.grid.measure_widths(window);

                            // Scrolling onto the last row reads the next chunk, as DBeaver does
                            if tab.fetch.has_more && !is_running_query && is_scrolled_to_bottom(&tab.grid) {
                                cx.defer_in(window, |this, _, cx| this.fetch_next_rows(cx));
                            }

                            let active_resizing_col = self.resizing_column.and_then(|(t, c, _, _)| {
                                if t == active_tab_idx { Some(c) } else { None }
                            });

                            let theme_border_focused = theme.border_focused;
                            let theme_border = theme.border;
                            let view = cx.entity().downgrade();
                            let cell_view = view.clone();
                            let on_cell_mouse_down: ui_grid::CellMouseDownHandler = std::rc::Rc::new(move |row, col, event, _window, app| {
                                let _ = cell_view.update(app, |this, cx| {
                                    this.select_grid_cell(row, col, event.modifiers.shift, cx);
                                    if event.click_count >= 2 && !event.modifiers.shift {
                                        this.begin_cell_edit(row, col);
                                    }
                                });
                            });
                            // Ordering is added to the generated SELECT, so only table tabs offer it
                            let on_header_menu: Option<ui_grid::HeaderMenuHandler> =
                                self.tabs[active_tab_idx].table_name.is_some().then(|| {
                                    let menu_view = view.clone();
                                    std::rc::Rc::new(move |col: usize, event: &MouseDownEvent, _window: &mut Window, app: &mut App| {
                                        let _ = menu_view.update(app, |this, cx| {
                                            this.open_column_menu(active_tab_idx, col, event.position);
                                            cx.notify();
                                        });
                                    }) as ui_grid::HeaderMenuHandler
                                });
                            let cell_editor = self.cell_editor_renderer(cx);
                            let tab = &mut self.tabs[active_tab_idx];

                            tab.grid.render_grid_interactive(active_resizing_col, Some(on_cell_mouse_down), on_header_menu, cell_editor, move |col_idx, cur_width| {
                                let is_active = active_resizing_col == Some(col_idx);
                                let bg_color = if is_active {
                                    Rgba { a: 0.85, ..theme_border_focused }
                                } else {
                                    Rgba { a: 0.0, ..theme_border_focused }
                                };
                                let hover_bg = Rgba { a: 0.35, ..theme_border_focused };
                                let grip_color = if is_active {
                                    theme_border_focused
                                } else {
                                    Rgba { a: 0.7, ..theme_border }
                                };

                                let view_clone = view.clone();

                                div()
                                    .id(("col-resizer", col_idx))
                                    .w(px(8.0))
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .cursor_col_resize()
                                    .bg(bg_color)
                                    .hover(move |s| s.bg(hover_bg))
                                    .child(
                                        div()
                                            .w(px(1.5))
                                            .h(px(14.0))
                                            .rounded_xs()
                                            .bg(grip_color)
                                    )
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |event: &MouseDownEvent, _window: &mut Window, app: &mut App| {
                                            let _ = view_clone.update(app, |this, cx| {
                                                if event.button == MouseButton::Left {
                                                    if event.click_count >= 2 {
                                                        if let Some(tab) = this.tabs.get_mut(active_tab_idx) {
                                                            tab.grid.auto_fit_single_column(col_idx);
                                                            this.resizing_column = None;
                                                            cx.notify();
                                                        }
                                                    } else {
                                                        this.resizing_column = Some((
                                                            active_tab_idx,
                                                            col_idx,
                                                            f32::from(event.position.x),
                                                            cur_width,
                                                        ));
                                                        cx.notify();
                                                    }
                                                }
                                            });
                                        },
                                    )
                                    .into_any_element()
                            }).into_any_element()
                        }
                        DataViewMode::Text => {
                            let tab = &self.tabs[active_tab_idx];
                            let mut text_lines = Vec::new();
                            if let Some(ref pager) = tab.grid.pager {
                                let col_names: Vec<String> = pager.columns.iter().map(|c| c.name.clone()).collect();
                                text_lines.push(col_names.join(" | "));
                                text_lines.push(col_names.iter().map(|c| "-".repeat(c.len().max(6))).collect::<Vec<_>>().join("-+-"));
                                let limit_render = pager.total_rows().min(200);
                                for r in 0..limit_render {
                                    let mut row_vals = Vec::new();
                                    for c in 0..pager.num_cols() as usize {
                                        let val = if let Some(cell) = pager.get_cell(r, c) {
                                            String::from_utf8_lossy(&cell).to_string()
                                        } else {
                                            "NULL".to_string()
                                        };
                                        row_vals.push(val);
                                    }
                                    text_lines.push(row_vals.join(" | "));
                                }
                            }

                            div()
                                .id("text-view-scroll")
                                .flex()
                                .flex_col()
                                .flex_1()
                                .p_3()
                                .overflow_y_scroll()
                                .bg(theme.bg_editor)
                                .text_size(px(12.0))
                                .text_color(theme.text_primary)
                                .child(
                                    if text_lines.is_empty() {
                                        "(No query result data to display in text mode)".to_string()
                                    } else {
                                        text_lines.join("\n")
                                    }
                                )
                                .into_any_element()
                        }
                    }
                })
        );

        // Right vertical toolstrip (Panels)
        data_row = data_row.child(
            div()
                .id("btn-toggle-panels-strip")
                .flex()
                .flex_col()
                .items_center()
                .w(px(22.0))
                .h_full()
                .bg(theme.bg_surface)
                .border_l_1()
                .border_color(theme.border)
                .py_3()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.show_right_panels = !this.show_right_panels;
                    cx.notify();
                }))
                .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("P"))
                .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("A"))
                .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("N"))
                .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("E"))
                .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("L"))
                .child(div().text_size(px(9.5)).font_weight(FontWeight::BOLD).text_color(theme.text_muted).hover(|s| s.text_color(theme.text_primary)).child("S"))
        );

        if show_right_panels {
            data_row = data_row.child(self.render_value_panel(active_tab_idx, cx));
        }

        data_row
    }
}

/// Whether the grid body is scrolled within one row of its end. A result that fits the
/// viewport has nothing to scroll, so it never counts as scrolled to the bottom.
fn is_scrolled_to_bottom(grid: &VirtualDataGrid) -> bool {
    let state = grid.vertical_scroll_handle.0.borrow();
    let max_y = f32::from(state.base_handle.max_offset().height);
    let scrolled_y = -f32::from(state.base_handle.offset().y);
    max_y > 0.0 && scrolled_y >= max_y - grid.theme.row_height
}
