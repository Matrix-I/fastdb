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
                            let text_lines = if let Some(ref pager) = tab.grid.pager {
                                format_text_presentation(
                                    pager,
                                    &tab.grid.changeset,
                                    tab.grid.display_columns(),
                                    tab.grid.row_count() as usize,
                                )
                            } else {
                                Vec::new()
                            };

                            let total_rows = tab.grid.row_count();
                            let max_line_len = text_lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
                            let table_width_px = (max_line_len as f32 * 7.5 + 40.0).max(100.0);
                            let text_copy = text_lines.join("\n");

                            let header = div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .h(px(28.0))
                                .px_3()
                                .bg(theme.bg_surface)
                                .border_b_1()
                                .border_color(theme.border)
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(theme.text_secondary)
                                        .child(format!("{total_rows} row(s) · Plain text presentation")),
                                )
                                .child(
                                    div()
                                        .id("btn-copy-text-view")
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_xs()
                                        .bg(theme.bg_editor)
                                        .border_1()
                                        .border_color(theme.border)
                                        .text_size(px(11.0))
                                        .text_color(theme.text_primary)
                                        .cursor_pointer()
                                        .hover(|s| s.bg(theme.bg_surface_hover))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.clipboard_request_id += 1;
                                            cx.write_to_clipboard(ClipboardItem::new_string(text_copy.clone()));
                                            cx.notify();
                                        }))
                                        .child("📋 Copy"),
                                );

                            let text_body = if text_lines.is_empty() {
                                div()
                                    .p_3()
                                    .text_size(px(12.0))
                                    .text_color(theme.text_muted)
                                    .child("(No query result data to display in text mode)")
                                    .into_any_element()
                            } else {
                                text_table_body(text_lines.join("\n"), table_width_px)
                                    .text_color(theme.text_primary)
                                    .into_any_element()
                            };

                            div()
                                .id("text-view-container")
                                .flex()
                                .flex_col()
                                .flex_1()
                                .size_full()
                                .min_h_0()
                                .overflow_hidden()
                                .child(header)
                                .child(
                                    div()
                                        .id("text-view-scroll")
                                        .flex()
                                        .flex_col()
                                        .flex_1()
                                        .min_h_0()
                                        .overflow_scroll()
                                        .bg(theme.bg_editor)
                                        .child(text_body),
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

// GPUI accepts one family name; CSS-style lists silently fall back to a proportional font.
#[cfg(target_os = "macos")]
const MONO_FONT: &str = "Menlo";
#[cfg(not(target_os = "macos"))]
const MONO_FONT: &str = "DejaVu Sans Mono";

/// Shared by table tabs and SQL result panels through `render_data_panel`.
fn text_table_body(text: String, width: f32) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .min_w_full()
        .w(px(width))
        .whitespace_nowrap()
        .p_3()
        .font_family(MONO_FONT)
        .text_size(px(12.0))
        .line_height(px(18.0))
        .child(text)
}
pub const MAX_TEXT_COLUMN_WIDTH: usize = 255;

pub(super) fn format_text_presentation(
    pager: &db_wire::QueryResultPager,
    changeset: &ui_grid::Changeset,
    display_columns: &[usize],
    row_count: usize,
) -> Vec<String> {
    if pager.columns.is_empty() {
        return Vec::new();
    }

    let columns: Vec<usize> = if display_columns.is_empty() {
        (0..pager.columns.len()).collect()
    } else {
        display_columns.to_vec()
    };

    if columns.is_empty() {
        return Vec::new();
    }

    let num_rows = row_count.min(2000);

    // Initial width based on column header name
    let mut col_widths: Vec<usize> = columns
        .iter()
        .map(|&c| {
            pager
                .columns
                .get(c)
                .map(|col| col.name.chars().count().max(1))
                .unwrap_or(1)
        })
        .collect();

    // 1. First pass: sanitize cell values and calculate column widths
    let mut formatted_rows: Vec<Vec<String>> = Vec::with_capacity(num_rows);
    for r in 0..num_rows {
        let mut row_vals = Vec::with_capacity(columns.len());
        for (i, &c) in columns.iter().enumerate() {
            let cell = ui_grid::resolve_cell(pager, changeset, r as u64, c);
            let val_str = match cell {
                Some(s) => sanitize_text_cell(&s),
                None => "[NULL]".to_string(),
            };
            col_widths[i] = col_widths[i].max(val_str.chars().count());
            row_vals.push(val_str);
        }
        formatted_rows.push(row_vals);
    }

    // Cap column widths at MAX_TEXT_COLUMN_WIDTH (255),
    // while keeping at least the column header width (or capped if header itself > 255).
    for (i, &c) in columns.iter().enumerate() {
        let header_len = pager
            .columns
            .get(c)
            .map(|col| col.name.chars().count())
            .unwrap_or(1);
        col_widths[i] = col_widths[i]
            .min(MAX_TEXT_COLUMN_WIDTH)
            .max(header_len.min(MAX_TEXT_COLUMN_WIDTH))
            .max(1);
    }

    let mut lines = Vec::with_capacity(num_rows + 2);

    // 2. Format header line
    let mut header_line = String::new();
    for (i, &c) in columns.iter().enumerate() {
        let col_name = pager
            .columns
            .get(c)
            .map(|col| col.name.as_str())
            .unwrap_or("");
        let width = col_widths[i];
        let name_chars: Vec<char> = col_name.chars().collect();
        let take_len = name_chars.len().min(width);
        let name_str: String = name_chars[..take_len].iter().collect();
        header_line.push_str(&name_str);
        let pad = width.saturating_sub(take_len);
        for _ in 0..pad {
            header_line.push(' ');
        }
        header_line.push('|');
    }
    lines.push(header_line);

    // 3. Format divider line
    let mut divider_line = String::new();
    for &width in &col_widths {
        for _ in 0..width {
            divider_line.push('-');
        }
        divider_line.push('+');
    }
    lines.push(divider_line);

    // 4. Format data rows
    for row in formatted_rows {
        let mut row_line = String::new();
        for (i, val) in row.into_iter().enumerate() {
            let width = col_widths[i];
            let val_chars: Vec<char> = val.chars().collect();
            let take_len = val_chars.len().min(width);
            let val_str: String = val_chars[..take_len].iter().collect();
            row_line.push_str(&val_str);
            let pad = width.saturating_sub(take_len);
            for _ in 0..pad {
                row_line.push(' ');
            }
            row_line.push('|');
        }
        lines.push(row_line);
    }

    lines
}

fn sanitize_text_cell(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\r' | '\n' | '\t' => ' ',
            _ => c,
        })
        .collect()
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
mod tests {
    // Not a glob: `super::*` brings in `gpui::test`, which shadows the built-in `#[test]`.
    use super::format_text_presentation;
    use db_wire::{ColumnMeta, DataType, QueryResultPager};
    use ui_grid::Changeset;

    #[test]
    fn text_table_uses_a_single_monospace_family() {
        // GPUI treats this as one family name, not a CSS fallback list.
        use gpui::Styled;
        let mut body =
            super::text_table_body("key  |value|\n-----+-----+\nshort|text |".into(), 200.0);
        let style = body.text_style().as_ref().unwrap();
        let family = style.font_family.as_ref().unwrap();
        assert!(!family.contains(','));
        assert_eq!(family.as_ref(), super::MONO_FONT);
        #[cfg(target_os = "macos")]
        assert_eq!(super::MONO_FONT, "Menlo");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(super::MONO_FONT, "DejaVu Sans Mono");
    }

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

    #[test]
    fn test_text_presentation_matches_dbeaver_format() {
        let cols = vec![
            ColumnMeta::new("key", DataType::Text),
            ColumnMeta::new("value", DataType::Text),
        ];
        let pager = QueryResultPager::new(cols).unwrap();

        let long_sws = concat!(
            "[{\"errorCode\": 0, \"errorText\": \"No errors found\", \"errorLanguage\": \"EN\", \"errorLanguage2\": \"ENG\"}, ",
            "{\"errorCode\": 1, \"errorText\": \"Generic error\", \"errorLanguage\": \"EN\", \"errorLanguage2\": \"ENG\"}, ",
            "{\"errorCode\": 3, \"errorText\": \"Virtual device locked\", \"errorLanguage\": \"EN\", \"errorLanguage2\": \"ENG\"}, ",
            "{\"errorCode\": 4, \"errorText\": \"Wrong credentials\", \"errorLanguage\": \"EN\"}]"
        );
        let phone_rules = "{\"41\": {\"maxLength\": 9, \"minLength\": 9, \"leadingZero\": true, \"allowedPrefixes\": [\"76\", \"77\", \"78\", \"79\"]}, \"99999\": {\"maxLength\": 50, \"minLength\": 1, \"leadingZero\": true}}";

        pager
            .append_row(vec![
                Some(b"io.ubitec.namirial.service.SWS_ERROR_CODES".as_slice()),
                Some(long_sws.as_bytes()),
            ])
            .unwrap();
        pager
            .append_row(vec![
                Some(b"io.ubitec.id.service.PHONE_NUMBER_VALIDATION_RULES".as_slice()),
                Some(phone_rules.as_bytes()),
            ])
            .unwrap();

        let changeset = Changeset::default();
        let lines = format_text_presentation(&pager, &changeset, &[0, 1], 2);

        assert_eq!(lines.len(), 4);

        // Header: "key" padded to 50 + "|" + "value" padded to 255 + "|" -> len 307
        assert_eq!(lines[0].len(), 307);
        assert!(lines[0].starts_with("key                                               |value"));
        assert!(lines[0].ends_with('|'));

        // Divider: 50 dashes + "+" + 255 dashes + "+" -> len 307
        assert_eq!(lines[1].len(), 307);
        let expected_divider = format!("{}+{}+", "-".repeat(50), "-".repeat(255));
        assert_eq!(lines[1], expected_divider);

        // Row 0: key padded to 50 (42 chars + 8 spaces) + "|" + value truncated to 255 + "|" -> len 307
        assert_eq!(lines[2].len(), 307);
        assert!(
            lines[2].starts_with(
                "io.ubitec.namirial.service.SWS_ERROR_CODES        |[{\"errorCode\": 0,"
            )
        );
        assert!(lines[2].ends_with('|'));

        // Row 1: key exact 50 chars + "|" + value padded to 255 + "|" -> len 307
        assert_eq!(lines[3].len(), 307);
        assert!(
            lines[3].starts_with("io.ubitec.id.service.PHONE_NUMBER_VALIDATION_RULES|{\"41\":")
        );
        assert!(lines[3].ends_with('|'));
    }

    #[test]
    fn test_text_presentation_null_and_newline_handling() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int4),
            ColumnMeta::new("notes", DataType::Text),
        ];
        let pager = QueryResultPager::new(cols).unwrap();

        pager
            .append_row(vec![
                Some(b"1".as_slice()),
                Some(b"line 1\nline 2\r\nline 3\ttab".as_slice()),
            ])
            .unwrap();
        pager.append_row(vec![Some(b"2".as_slice()), None]).unwrap();

        let changeset = Changeset::default();
        let lines = format_text_presentation(&pager, &changeset, &[0, 1], 2);

        assert_eq!(lines.len(), 4);
        // Header
        assert_eq!(lines[0], "id|notes                    |");
        // Divider
        assert_eq!(lines[1], "--+-------------------------+");
        // Row 0: newlines and tabs replaced with space
        assert_eq!(lines[2], "1 |line 1 line 2  line 3 tab|");
        // Row 1: NULL displayed as [NULL]
        assert_eq!(lines[3], "2 |[NULL]                   |");
    }

    #[test]
    fn test_text_presentation_empty_result() {
        let cols = vec![
            ColumnMeta::new("col_a", DataType::Text),
            ColumnMeta::new("col_b", DataType::Text),
        ];
        let pager = QueryResultPager::new(cols).unwrap();
        let changeset = Changeset::default();
        let lines = format_text_presentation(&pager, &changeset, &[0, 1], 0);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "col_a|col_b|");
        assert_eq!(lines[1], "-----+-----+");
    }

    #[test]
    fn test_text_presentation_changeset_edits() {
        let cols = vec![ColumnMeta::new("name", DataType::Text)];
        let pager = QueryResultPager::new(cols).unwrap();
        pager.append_row(vec![Some(b"Alice".as_slice())]).unwrap();

        let mut changeset = Changeset::default();
        changeset.set_cell(0, 0, Some(b"Alice".to_vec()), Some("Bob".to_string()));

        let lines = format_text_presentation(&pager, &changeset, &[0], 1);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "name|");
        assert_eq!(lines[1], "----+");
        assert_eq!(lines[2], "Bob |");
    }
}
