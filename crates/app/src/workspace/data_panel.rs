use super::*;

fn strip_label(path: &'static str, height: f32) -> gpui::Svg {
    svg().path(path).w(px(10.0)).h(px(height)).flex_shrink_0()
}

type StripAction = Box<dyn Fn(&mut Workspace, &mut Context<Workspace>) + 'static>;

impl Workspace {
    /// Data sub-tab: result grid or text view, with the optional record side panel
    pub(super) fn render_data_panel(
        &mut self,
        active_tab_idx: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let theme = &theme;
        let tab = &self.tabs[active_tab_idx];
        let view_mode = tab.view_mode;
        let show_right_panels = self.layout.show_right_panels
            && !self.layout.focus_data
            && view_mode != DataViewMode::Record;
        let mut data_row = div().flex().flex_row().flex_1().w_full().overflow_hidden();

        let tab_id = tab.id;

        // Left vertical toolstrip matching DBeaver: Grid / Text / Record
        let strip_item = |id: &'static str,
                          icon: &'static str,
                          label_icon: &'static str,
                          label_height: f32,
                          is_active: bool,
                          on_click: StripAction,
                          cx: &mut Context<Self>| {
            div()
                .id(id)
                .flex()
                .flex_col()
                .items_center()
                .w(px(24.0))
                .py_1p5()
                .gap_1p5()
                .rounded_xs()
                .border_1()
                .border_color(if is_active {
                    theme.border
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0).into()
                })
                .bg(if is_active {
                    theme.bg_surface_selected
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0).into()
                })
                .cursor_pointer()
                .hover(|s| s.bg(theme.bg_surface_hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    on_click(this, cx);
                    cx.notify();
                }))
                .child(
                    strip_label(label_icon, label_height).text_color(if is_active {
                        theme.text_primary
                    } else {
                        theme.text_muted
                    }),
                )
                .child(img(icon).w(px(14.0)).h(px(14.0)))
        };

        data_row = data_row.child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .w(px(28.0))
                .h_full()
                .bg(theme.bg_surface)
                .border_r_1()
                .border_color(theme.border)
                .py_2()
                .gap_2()
                .child(strip_item(
                    "toolstrip-grid",
                    "icons/grid.svg",
                    "icons/label_grid.svg",
                    23.0,
                    view_mode == DataViewMode::Grid,
                    Box::new(|this, _| {
                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                            t.view_mode = DataViewMode::Grid;
                            t.record_view = None;
                        }
                    }),
                    cx,
                ))
                .child(strip_item(
                    "toolstrip-text",
                    "icons/sql_script.svg",
                    "icons/label_text.svg",
                    24.0,
                    view_mode == DataViewMode::Text,
                    Box::new(|this, _| {
                        if let Some(t) = this.tabs.get_mut(this.active_tab) {
                            t.view_mode = DataViewMode::Text;
                            t.record_view = None;
                        }
                    }),
                    cx,
                ))
                .child(strip_item(
                    "toolstrip-record",
                    "icons/columns.svg",
                    "icons/label_record.svg",
                    37.0,
                    view_mode == DataViewMode::Record,
                    Box::new(move |this, cx| {
                        if let Some(t) = this.tabs.get(this.active_tab) {
                            if t.view_mode == DataViewMode::Record {
                                if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                    t.view_mode = DataViewMode::Grid;
                                    t.record_view = None;
                                }
                            } else if t.grid.row_count() > 0 && t.grid.pager.is_some() {
                                this.open_record_view(tab_id, cx);
                            } else if let Some(t) = this.tabs.get_mut(this.active_tab) {
                                t.view_mode = DataViewMode::Record;
                            }
                        }
                    }),
                    cx,
                )),
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
                    this.input.is_filter_focused = false;
                    this.blur_fetch_size();
                    cx.notify();
                }))
                .child({
                    match view_mode {
                        DataViewMode::Grid => {
                            let sidebar_w = self.layout.navigator_width;
                            let panels_w = if show_right_panels { self.layout.value_panel_width } else { 0.0 };
                            let available_viewport_w = (f32::from(window.viewport_size().width) - sidebar_w - CENTER_CHROME_WIDTH - panels_w).max(200.0);
                            let available_viewport_h = (f32::from(window.viewport_size().height) - 180.0).max(200.0);
                            let is_running_query = self.operations.is_running();
                            let search_open = self.result_search.is_some();
                            let tab = &mut self.tabs[active_tab_idx];
                            tab.grid.update_viewport_width(available_viewport_w);
                            tab.grid.update_viewport_height(available_viewport_h);
                            tab.grid.measure_widths(window);

                            // Scrolling onto the last row reads the next chunk, as DBeaver does
                            if auto_fetch_allowed(tab.fetch.has_more, is_running_query, is_scrolled_to_bottom(&tab.grid), tab.grid.auto_fetch_paused.get(), search_open) {
                                let fetch_tab = tab.id;
                                let fetch_pager = tab.grid.pager.clone();
                                cx.defer_in(window, move |this, _, cx| {
                                    let allowed = this.tabs.get(this.active_tab).is_some_and(|current| {
                                        current.id == fetch_tab
                                            && current.grid.pager.as_ref().zip(fetch_pager.as_ref()).is_some_and(|(current, original)| Arc::ptr_eq(current, original))
                                            && auto_fetch_allowed(current.fetch.has_more, this.operations.is_running(), is_scrolled_to_bottom(&current.grid), current.grid.auto_fetch_paused.get(), this.result_search.is_some())
                                    });
                                    if allowed { this.fetch_next_rows(cx); }
                                });
                            }

                            let active_resizing_col = self.layout.resizing_column.and_then(|(t, c, _, _)| {
                                if t == active_tab_idx { Some(c) } else { None }
                            });

                            let theme_border_focused = theme.border_focused;
                            let theme_border = theme.border;
                            let view = cx.entity().downgrade();
                            let cell_view = view.clone();
                            let on_cell_mouse_down: ui_grid::CellMouseDownHandler = std::rc::Rc::new(move |row, col, event, window, app| {
                                let _ = cell_view.update(app, |this, cx| {
                                    this.input.grid_drag = None;
                                    window.focus(&this.focus_handle);
                                    if event.button == MouseButton::Right {
                                        this.open_grid_context_menu(active_tab_idx, row, col, event.position, cx);
                                        return;
                                    }
                                    this.select_grid_cell(row, col, event.modifiers.shift, cx);
                                    if event.click_count >= 2 && !event.modifiers.shift {
                                        this.begin_cell_edit(row, col);
                                    } else {
                                        this.input.grid_drag = super::grid_selection::GridDragSelection::begin(&this.tabs[active_tab_idx]);
                                    }
                                });
                            });
                            let drag_view = view.clone();
                            let on_cell_mouse_move: ui_grid::CellMouseMoveHandler = std::rc::Rc::new(move |row, col, _event, _window, app| {
                                let _ = drag_view.update(app, |this, cx| {
                                    this.extend_grid_drag(row, col, cx);
                                });
                            });
                            // Both table and SQL results offer column layout actions.
                            let on_header_menu: Option<ui_grid::HeaderMenuHandler> =
                                self.tabs[active_tab_idx].grid.pager.is_some().then(|| {
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

                            tab.grid.render_grid_interactive(active_resizing_col, Some(on_cell_mouse_down), Some(on_cell_mouse_move), on_header_menu, cell_editor, move |col_idx, cur_width| {
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
                                                            this.layout.resizing_column = None;
                                                            cx.notify();
                                                        }
                                                    } else {
                                                        this.layout.resizing_column = Some((
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
                        DataViewMode::Record => self.render_record_view(active_tab_idx, cx).into_any_element(),
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
                .flex()
                .flex_col()
                .items_center()
                .w(px(28.0))
                .h_full()
                .bg(theme.bg_surface)
                .border_l_1()
                .border_color(theme.border)
                .py_2()
                .child(
                    div()
                        .id("btn-toggle-panels-strip")
                        .flex()
                        .flex_col()
                        .items_center()
                        .w(px(24.0))
                        .py_1p5()
                        .gap_1p5()
                        .rounded_xs()
                        .border_1()
                        .border_color(if show_right_panels {
                            theme.border
                        } else {
                            hsla(0.0, 0.0, 0.0, 0.0).into()
                        })
                        .bg(if show_right_panels {
                            theme.bg_surface_selected
                        } else {
                            hsla(0.0, 0.0, 0.0, 0.0).into()
                        })
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.bg_surface_hover))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.layout.show_right_panels = !this.layout.show_right_panels;
                            cx.notify();
                        }))
                        .child(img("icons/panels.svg").w(px(14.0)).h(px(14.0)))
                        .child(strip_label("icons/label_panels.svg", 35.0).text_color(
                            if show_right_panels {
                                theme.text_primary
                            } else {
                                theme.text_muted
                            },
                        )),
                ),
        );

        if show_right_panels {
            data_row = data_row.child(self.render_value_panel(active_tab_idx, cx));
        }

        let tab_id = self.tabs[active_tab_idx].id;
        let finder = super::script_tools::tool("Find in result…");
        let finder = if self.tabs[active_tab_idx].grid.pager.is_some() {
            finder.on_click(cx.listener(move |this, _, _, cx| this.open_result_search(tab_id, cx)))
        } else {
            finder.opacity(0.4)
        };
        let record = super::script_tools::tool("Record");
        let record = if self.tabs[active_tab_idx].grid.row_count() > 0 {
            record.on_click(cx.listener(move |this, _, _, cx| this.open_record_view(tab_id, cx)))
        } else {
            record.opacity(0.4)
        };
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .flex()
                    .flex_none()
                    .px_2()
                    .py_1()
                    .bg(theme.bg_surface)
                    .child(finder)
                    .child(record)
                    .child(
                        super::script_tools::tool("Value editor")
                            .on_click(cx.listener(|this, _, _, cx| this.open_value_editor(cx))),
                    )
                    .children(
                        (self.tabs[active_tab_idx].is_script()
                            && self.tab_provider(active_tab_idx) == DbDriverKind::Postgres)
                            .then(|| {
                                super::script_tools::tool("Load editable result").on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        this.load_editable_result(tab_id, cx)
                                    }),
                                )
                            }),
                    )
                    .children(
                        (self.tabs[active_tab_idx].is_script()
                            && (self.editable_query_result(active_tab_idx)
                                || self.tabs[active_tab_idx].grid.changeset.is_dirty()
                                || self.tabs[active_tab_idx].grid.editing_cell.is_some()))
                        .then(|| self.render_edit_toolbar(cx)),
                    )
                    .children(
                        self.tabs[active_tab_idx]
                            .reference_picker
                            .is_some()
                            .then(|| {
                                super::script_tools::tool("Use selected key").on_click(
                                    cx.listener(|this, _, _, cx| this.accept_grid_reference(cx)),
                                )
                            }),
                    ),
            )
            .child(data_row)
    }
}

pub(super) fn auto_fetch_allowed(
    has_more: bool,
    running: bool,
    bottom: bool,
    paused: bool,
    search_open: bool,
) -> bool {
    has_more && !running && bottom && !paused && !search_open
}

/// Whether the grid body is scrolled within one row of its end. A result that fits the
/// viewport has nothing to scroll, so it never counts as scrolled to the bottom.
fn is_scrolled_to_bottom(grid: &VirtualDataGrid) -> bool {
    let state = grid.vertical_scroll_handle.0.borrow();
    let max_y = f32::from(state.base_handle.max_offset().height);
    let scrolled_y = -f32::from(state.base_handle.offset().y);
    max_y > 0.0 && scrolled_y >= max_y - grid.theme.row_height
}

#[cfg(test)]
mod strip_label_tests {
    #[test]
    fn strip_labels_reserve_visible_layout_space() {
        use gpui::{Styled, px};
        for (path, height) in [
            ("icons/label_grid.svg", 23.0),
            ("icons/label_text.svg", 24.0),
            ("icons/label_record.svg", 37.0),
            ("icons/label_panels.svg", 35.0),
        ] {
            let mut label = super::strip_label(path, height);
            assert_eq!(label.style().size.width, Some(px(10.0).into()));
            assert_eq!(label.style().size.height, Some(px(height).into()));
        }
    }
}
