use std::collections::HashSet;
use std::sync::Arc;
use gpui::*;
use mem_engine::QueryResultPager;
use ui_core::theme::Theme;

use crate::changeset::Changeset;
use crate::selection::GridSelection;

pub const MIN_COL_WIDTH: f32 = 75.0;
pub const MAX_AUTO_COL_WIDTH: f32 = 1200.0;
pub const MAX_EXPANDED_COL_WIDTH: f32 = 2500.0;
pub const GUTTER_WIDTH: f32 = 52.0;
/// Number of leading rows measured when auto-fitting column widths
pub const WIDTH_SAMPLE_ROWS: u64 = 100;

pub fn dbeaver_type_badge(data_type: mem_engine::DataType) -> &'static str {
    match data_type {
        mem_engine::DataType::Text | mem_engine::DataType::Varchar => "AZ",
        mem_engine::DataType::Json => "{ }",
        mem_engine::DataType::Int2
        | mem_engine::DataType::Int4
        | mem_engine::DataType::Int8
        | mem_engine::DataType::Float4
        | mem_engine::DataType::Float8
        | mem_engine::DataType::Numeric => "123",
        mem_engine::DataType::Date
        | mem_engine::DataType::Time
        | mem_engine::DataType::Timestamp
        | mem_engine::DataType::TimestampTz => "📅",
        mem_engine::DataType::Bool => "0/1",
        mem_engine::DataType::Uuid => "#",
        mem_engine::DataType::Bytea => "0x",
        mem_engine::DataType::Unknown => "?",
    }
}

pub struct VirtualDataGrid {
    pub pager: Option<Arc<QueryResultPager>>,
    pub theme: Theme,
    pub table_name: String,
    pub scroll_top: f32,
    pub scroll_left: f32,
    pub viewport_height: f32,
    pub viewport_width: f32,
    pub content_widths: Vec<f32>,
    pub natural_widths: Vec<f32>,
    pub column_widths: Vec<f32>,
    pub manual_column_widths: HashSet<usize>,
    /// Rows already measured for column widths; grows while results stream in
    pub width_sampled_rows: u64,
    pub selection: GridSelection,
    pub changeset: Changeset,
    pub editing_cell: Option<(u64, usize, String)>,
    pub horizontal_scroll_handle: ScrollHandle,
    pub vertical_scroll_handle: UniformListScrollHandle,
}

impl VirtualDataGrid {
    pub fn new(pager: Option<Arc<QueryResultPager>>, theme: Theme) -> Self {
        let mut grid = Self {
            pager,
            theme,
            table_name: "query_result".to_string(),
            scroll_top: 0.0,
            scroll_left: 0.0,
            viewport_height: 600.0,
            viewport_width: 1000.0,
            content_widths: Vec::new(),
            natural_widths: Vec::new(),
            column_widths: Vec::new(),
            manual_column_widths: HashSet::new(),
            width_sampled_rows: 0,
            selection: GridSelection::new(),
            changeset: Changeset::new(),
            editing_cell: None,
            horizontal_scroll_handle: ScrollHandle::new(),
            vertical_scroll_handle: UniformListScrollHandle::new(),
        };
        grid.recalculate_all_widths();
        grid
    }

    pub fn set_pager(&mut self, pager: Arc<QueryResultPager>) {
        self.pager = Some(pager);
        self.manual_column_widths.clear();
        self.recalculate_all_widths();
        self.selection.clear();
        self.changeset.clear();
        self.editing_cell = None;
        self.scroll_top = 0.0;
        self.scroll_left = 0.0;
        self.horizontal_scroll_handle = ScrollHandle::new();
        self.vertical_scroll_handle = UniformListScrollHandle::new();
    }

    pub fn set_table_name(&mut self, name: impl Into<String>) {
        self.table_name = name.into();
    }

    pub fn update_viewport_height(&mut self, height: f32) {
        if (self.viewport_height - height).abs() > 2.0 {
            self.viewport_height = height;
        }
    }

    /// Accurately estimate rendered text width for GPUI proportional fonts (12px standard, scales with size)
    pub fn estimate_text_width(text: &str, font_size: f32) -> f32 {
        let scale = font_size / 12.0;
        let mut width = 0.0f32;
        for ch in text.chars() {
            let char_w = match ch {
                '.' | ':' | ',' | ';' | '\'' | '"' | '!' | '|' | 'i' | 'l' | 'j' | '1' | '`' => 3.8,
                'r' | 't' | 'f' | 'I' | ' ' | '-' | '(' | ')' | '[' | ']' | '{' | '}' | '/' | '\\' => 5.2,
                'm' | 'w' | 'M' | 'W' | '@' | '%' | '&' => 9.5,
                'A'..='Z' => 7.8,
                '0'..='9' => 6.8,
                _ => 6.5,
            };
            width += char_w * scale;
        }
        width
    }

    /// Calculate content-fit widths for each column based on header text & sampled cell data.
    /// Returns (snug_widths, natural_widths)
    pub fn calculate_content_and_natural_widths(&self) -> (Vec<f32>, Vec<f32>) {
        let Some(ref pager) = self.pager else {
            return (Vec::new(), Vec::new());
        };

        let num_cols = pager.num_cols() as usize;
        let mut snug_widths = Vec::with_capacity(num_cols);
        let mut natural_widths = Vec::with_capacity(num_cols);
        let sample_rows = pager.total_rows().min(WIDTH_SAMPLE_ROWS);

        for (col_idx, col) in pager.columns.iter().enumerate() {
            // 1. Column header width: name + DBeaver data type badge + key icon + dropdown arrow + padding
            let name_w = Self::estimate_text_width(&col.name, 12.0);
            let badge_str = dbeaver_type_badge(col.data_type);
            let badge_w = Self::estimate_text_width(badge_str, 10.0);
            let header_w = name_w + badge_w + 50.0;

            // 2. Data sample width across up to 100 rows
            let mut max_cell_w = 0.0f32;
            for r in 0..sample_rows {
                let cell_str = if let Some(mod_val) = self.changeset.get_modified_value(r, col_idx) {
                    mod_val.to_string()
                } else if let Some(cell_bytes) = pager.get_cell(r, col_idx) {
                    String::from_utf8_lossy(&cell_bytes).into_owned()
                } else {
                    "NULL".to_string()
                };

                let sample = if cell_str.len() > 500 {
                    let idx = cell_str.char_indices().nth(500).map(|(i, _)| i).unwrap_or(cell_str.len());
                    &cell_str[..idx]
                } else {
                    &cell_str
                };

                let cell_w = Self::estimate_text_width(sample, 12.0) + 26.0;
                if cell_w > max_cell_w {
                    max_cell_w = cell_w;
                }
            }

            let natural_w = header_w.max(max_cell_w).max(MIN_COL_WIDTH);
            let snug_w = natural_w.clamp(MIN_COL_WIDTH, MAX_AUTO_COL_WIDTH);
            natural_widths.push(natural_w);
            snug_widths.push(snug_w);
        }

        (snug_widths, natural_widths)
    }

    /// Calculate content-fit widths for each column based on header text & sampled cell data
    pub fn calculate_content_widths(&self) -> Vec<f32> {
        self.calculate_content_and_natural_widths().0
    }

    /// Recalculates intrinsic content widths and applies viewport expansion distribution
    pub fn recalculate_all_widths(&mut self) {
        self.width_sampled_rows = self
            .pager
            .as_ref()
            .map(|p| p.total_rows().min(WIDTH_SAMPLE_ROWS))
            .unwrap_or(0);
        let (snug, natural) = self.calculate_content_and_natural_widths();
        self.content_widths = snug;
        self.natural_widths = natural;
        self.apply_viewport_distribution();
    }

    /// Re-measures column widths while the leading sample rows are still streaming in.
    /// Cheap once the sample is complete, so it can be called on every appended batch.
    pub fn on_rows_appended(&mut self) {
        let total_rows = self.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0);
        if self.width_sampled_rows < WIDTH_SAMPLE_ROWS && total_rows > self.width_sampled_rows {
            self.recalculate_all_widths();
        }
    }

    /// Update the grid's available viewport width and distribute columns if width changed
    pub fn update_viewport_width(&mut self, width: f32) {
        if (self.viewport_width - width).abs() > 2.0 {
            self.viewport_width = width;
            self.apply_viewport_distribution();
        }
    }

    /// Preserves snug content fit for all columns without adding artificial blank whitespace,
    /// and allows columns with clipped/truncated data to expand into available window space.
    pub fn apply_viewport_distribution(&mut self) {
        let num_cols = self.pager.as_ref().map(|p| p.num_cols() as usize).unwrap_or(0);
        if num_cols == 0 {
            self.column_widths.clear();
            self.content_widths.clear();
            self.natural_widths.clear();
            return;
        }

        if self.content_widths.len() != num_cols || self.natural_widths.len() != num_cols {
            let (snug, natural) = self.calculate_content_and_natural_widths();
            self.content_widths = snug;
            self.natural_widths = natural;
        }

        if self.column_widths.len() != num_cols {
            self.column_widths = self.content_widths.clone();
        }

        let mut working_widths = Vec::with_capacity(num_cols);
        let mut manual_sum = 0.0f32;
        let mut non_manual_content_sum = 0.0f32;
        let mut capped_cols = Vec::new();

        for c in 0..num_cols {
            if self.manual_column_widths.contains(&c) {
                let w = self.column_widths.get(c).copied().unwrap_or(MIN_COL_WIDTH).max(MIN_COL_WIDTH);
                manual_sum += w;
                working_widths.push(w);
            } else {
                let snug_w = self.content_widths.get(c).copied().unwrap_or(MIN_COL_WIDTH);
                let nat_w = self.natural_widths.get(c).copied().unwrap_or(snug_w);
                non_manual_content_sum += snug_w;
                if nat_w > snug_w + 1.0 {
                    capped_cols.push(c);
                }
                working_widths.push(snug_w);
            }
        }

        let avail_for_cols = (self.viewport_width - GUTTER_WIDTH).max(0.0);
        let avail_for_non_manual = (avail_for_cols - manual_sum).max(0.0);

        // If available window width exceeds non-manual content sum:
        // Only expand columns whose content was actually capped/truncated!
        // Columns whose content is already fully visible (e.g. key) stay snug ("khít")!
        if !capped_cols.is_empty() && avail_for_non_manual > non_manual_content_sum {
            let extra = avail_for_non_manual - non_manual_content_sum;
            let per_capped = extra / capped_cols.len() as f32;
            for &c in &capped_cols {
                let base_w = working_widths[c];
                let nat_w = self.natural_widths.get(c).copied().unwrap_or(base_w);
                let max_needed = (nat_w - base_w).max(0.0);
                let added = per_capped.min(max_needed).min(MAX_EXPANDED_COL_WIDTH - base_w);
                working_widths[c] = (base_w + added).clamp(MIN_COL_WIDTH, MAX_EXPANDED_COL_WIDTH);
            }
        }

        self.column_widths = working_widths;
    }

    /// Manually resize a column with the mouse, pinning its width
    pub fn set_column_width(&mut self, col: usize, width: f32) {
        if col < self.column_widths.len() {
            self.column_widths[col] = width.max(MIN_COL_WIDTH);
            self.manual_column_widths.insert(col);
        }
    }

    /// Double-click action: resets a column to auto-fit its content and unpins it
    pub fn auto_fit_single_column(&mut self, col: usize) {
        self.manual_column_widths.remove(&col);
        let (snug, natural) = self.calculate_content_and_natural_widths();
        self.content_widths = snug;
        self.natural_widths = natural;
        self.apply_viewport_distribution();
    }

    pub fn scroll_by(&mut self, delta_y: f32, delta_x: f32) {
        let total_rows = self.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0);
        let max_scroll_y = (total_rows as f32 * self.theme.row_height - self.viewport_height).max(0.0);

        self.scroll_top = (self.scroll_top + delta_y).clamp(0.0, max_scroll_y);
        self.scroll_left = (self.scroll_left + delta_x).max(0.0);
    }

    pub fn render_grid(&mut self) -> impl IntoElement {
        self.render_grid_interactive(None, |_col_idx, _width| {
            div()
                .w(px(8.0))
                .h_full()
                .cursor_col_resize()
                .into_any_element()
        })
    }

    pub fn render_grid_interactive<F>(&mut self, resizing_col: Option<usize>, mut make_resizer: F) -> impl IntoElement
    where
        F: FnMut(usize, f32) -> AnyElement,
    {
        let theme = &self.theme;
        let row_height = theme.row_height;
        let header_height = theme.header_height;
        let gutter_width = GUTTER_WIDTH;

        let total_rows = self.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0);
        let num_cols = self.pager.as_ref().map(|p| p.num_cols() as usize).unwrap_or(0);

        let total_cols_width: f32 = self.column_widths.iter().sum();
        let content_total_width = gutter_width + total_cols_width;
        let total_width = content_total_width.max(self.viewport_width);
        let has_horizontal_overflow = content_total_width > self.viewport_width + 1.0;

        let total_content_height = total_rows as f32 * row_height;
        let has_vertical_overflow = total_content_height > self.viewport_height + 1.0;

        let grid_container = div()
            .flex()
            .flex_col()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .bg(theme.bg_grid)
            .text_color(theme.text_primary);

        // Horizontal scroller container
        let mut scroller = div()
            .id("grid-horizontal-scroller")
            .track_scroll(&self.horizontal_scroll_handle)
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .h_full()
            .overflow_x_scroll();

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
                    .child("#")
            );

        if let Some(ref pager) = self.pager {
            for (col_idx, col) in pager.columns.iter().enumerate() {
                let width = self.column_widths.get(col_idx).copied().unwrap_or(MIN_COL_WIDTH);
                let is_resizing = resizing_col == Some(col_idx);
                let resizer = make_resizer(col_idx, width);
                let badge_str = dbeaver_type_badge(col.data_type);
                let is_pk = col.is_primary_key || col.name.eq_ignore_ascii_case("id");

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
                        .border_color(if is_resizing { theme.border_focused } else { theme.border })
                        .child(
                            div()
                                .flex()
                                .flex_1()
                                .items_center()
                                .justify_between()
                                .h_full()
                                .px_2()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1_5()
                                        .truncate()
                                        .child(
                                            div()
                                                .font_weight(FontWeight::BOLD)
                                                .text_size(px(10.0))
                                                .text_color(rgb(0x58a6ff))
                                                .child(badge_str)
                                        )
                                        .child(if is_pk {
                                            div()
                                                .flex()
                                                .items_center()
                                                .child(img("icons/key.svg").w(px(11.0)).h(px(11.0)))
                                                .into_any_element()
                                        } else {
                                            div().into_any_element()
                                        })
                                        .child(
                                            div()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_size(px(12.0))
                                                .text_color(theme.text_primary)
                                                .truncate()
                                                .child(col.name.clone())
                                        )
                                )
                                .child(
                                    div()
                                        .text_size(px(8.5))
                                        .text_color(rgb(0x58a6ff))
                                        .child("▼")
                                )
                        )
                        .child(resizer)
                );
            }
        }

        scroller = scroller.child(header_row);

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
        let theme_bg_cell_selected = theme.bg_grid_cell_selected;
        let theme_bg_cell_modified = theme.bg_grid_cell_modified;

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
                            let row_bg = if is_alt { theme_bg_grid_alt } else { theme_bg_grid };

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
                                        .text_color(theme_text_muted)
                                        .child(format!("{}", r + 1))
                                );

                            for c in 0..num_cols {
                                let width = column_widths_clone.get(c).copied().unwrap_or(MIN_COL_WIDTH);
                                let is_selected = selection_clone.is_selected(r_u64, c);
                                let is_modified = changeset_clone.is_modified(r_u64, c);

                                let mut cell_div = div()
                                    .flex()
                                    .items_center()
                                    .w(px(width))
                                    .h_full()
                                    .px_2()
                                    .border_r_1()
                                    .border_color(theme_border)
                                    .text_size(px(12.0))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis();

                                if is_selected {
                                    cell_div = cell_div.bg(theme_bg_cell_selected);
                                } else if is_modified {
                                    cell_div = cell_div.bg(theme_bg_cell_modified);
                                }

                                let (display_text, is_null) = if let Some(mod_val) = changeset_clone.get_modified_value(r_u64, c) {
                                    (mod_val.to_string(), false)
                                } else if let Some(ref p) = pager_clone {
                                    if let Some(cell_bytes) = p.get_cell(r_u64, c) {
                                        let s = String::from_utf8_lossy(&cell_bytes);
                                        if s.len() > 3000 {
                                            let truncate_idx = s.char_indices().nth(2000).map(|(i, _)| i).unwrap_or(s.len());
                                            (format!("{}…", &s[..truncate_idx]), false)
                                        } else {
                                            (s.into_owned(), false)
                                        }
                                    } else {
                                        ("NULL".to_string(), true)
                                    }
                                } else {
                                    ("NULL".to_string(), true)
                                };

                                let text_el = div().truncate().child(display_text);

                                if is_null {
                                    cell_div = cell_div
                                        .text_color(theme_text_muted)
                                        .child(text_el);
                                } else {
                                    cell_div = cell_div.child(text_el);
                                }

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

        scroller = scroller.child(body);

        let mut main_area = div()
            .flex()
            .flex_row()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .child(scroller);

        // Vertical scrollbar indicator when content exceeds viewport
        if has_vertical_overflow {
            let scroll_y = -f32::from(self.vertical_scroll_handle.0.borrow().base_handle.offset().y).max(0.0);
            let max_scroll_y = (total_content_height - self.viewport_height).max(1.0);
            let scroll_ratio = (scroll_y / max_scroll_y).clamp(0.0, 1.0);

            let scrollbar_w = 14.0f32;
            let btn_h = 16.0f32;
            let track_h = (self.viewport_height - btn_h * 2.0).max(40.0);
            let thumb_ratio = (self.viewport_height / total_content_height).clamp(0.08, 0.85);
            let thumb_height = (track_h * thumb_ratio).max(30.0);
            let max_travel = (track_h - thumb_height).max(0.0);
            let thumb_top = scroll_ratio * max_travel;

            let theme_border = theme.border;
            let theme_bg_header = theme.bg_grid_header;
            let theme_text_muted = theme.text_muted;
            let thumb_bg = Rgba { a: 0.4, ..theme_text_muted };
            let thumb_hover = Rgba { a: 0.8, ..theme_text_muted };

            let handle_up = self.vertical_scroll_handle.clone();
            let handle_down = self.vertical_scroll_handle.clone();
            let handle_track = self.vertical_scroll_handle.clone();

            let v_scrollbar = div()
                .id("grid-vertical-scrollbar")
                .flex()
                .flex_col()
                .flex_none()
                .items_center()
                .w(px(scrollbar_w))
                .h_full()
                .bg(theme_bg_header)
                .border_l_1()
                .border_color(theme_border)
                .child(
                    div()
                        .id("scroll-btn-up")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w_full()
                        .h(px(btn_h))
                        .cursor_pointer()
                        .text_size(px(8.0))
                        .text_color(theme_text_muted)
                        .hover(|s| s.bg(Rgba { a: 0.15, ..theme_text_muted }))
                        .child("▲")
                        .on_mouse_down(MouseButton::Left, move |_event, window, _app| {
                            let cur_offset = handle_up.0.borrow().base_handle.offset();
                            let cur_y = f32::from(cur_offset.y);
                            let new_y = (cur_y + 100.0).min(0.0);
                            handle_up.0.borrow().base_handle.set_offset(point(cur_offset.x, px(new_y)));
                            window.refresh();
                        })
                )
                .child(
                    div()
                        .id("scroll-track-v")
                        .flex()
                        .flex_1()
                        .w_full()
                        .relative()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, _app| {
                            let click_y = f32::from(event.position.y) - btn_h;
                            let target_ratio = (click_y / track_h).clamp(0.0, 1.0);
                            let target_row = (target_ratio * total_rows as f32) as usize;
                            handle_track.scroll_to_item_strict(target_row, ScrollStrategy::Center);
                            window.refresh();
                        })
                        .child(
                            div()
                                .id("scroll-thumb-v")
                                .absolute()
                                .top(px(thumb_top))
                                .left(px(2.0))
                                .w(px(scrollbar_w - 4.0))
                                .h(px(thumb_height))
                                .rounded_xs()
                                .bg(thumb_bg)
                                .hover(move |s| s.bg(thumb_hover))
                        )
                )
                .child(
                    div()
                        .id("scroll-btn-down")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w_full()
                        .h(px(btn_h))
                        .cursor_pointer()
                        .text_size(px(8.0))
                        .text_color(theme_text_muted)
                        .hover(|s| s.bg(Rgba { a: 0.15, ..theme_text_muted }))
                        .child("▼")
                        .on_mouse_down(MouseButton::Left, move |_event, window, _app| {
                            let cur_offset = handle_down.0.borrow().base_handle.offset();
                            let cur_y = f32::from(cur_offset.y);
                            let new_y = (cur_y - 100.0).max(-max_scroll_y);
                            handle_down.0.borrow().base_handle.set_offset(point(cur_offset.x, px(new_y)));
                            window.refresh();
                        })
                );

            main_area = main_area.child(v_scrollbar);
        }

        let mut grid_container = grid_container.child(main_area);

        // 3. Horizontal Scrollbar indicator when content exceeds viewport
        if has_horizontal_overflow {
            let scroll_x = -f32::from(self.horizontal_scroll_handle.offset().x).max(0.0);
            let max_scroll_x = (content_total_width - self.viewport_width).max(1.0);
            let scroll_ratio = (scroll_x / max_scroll_x).clamp(0.0, 1.0);

            let scrollbar_h = 14.0f32;
            let btn_w = 20.0f32;
            let track_w = (self.viewport_width - btn_w * 2.0).max(40.0);
            let thumb_ratio = (self.viewport_width / content_total_width).clamp(0.08, 0.85);
            let thumb_width = (track_w * thumb_ratio).max(36.0);
            let max_travel = (track_w - thumb_width).max(0.0);
            let thumb_left = scroll_ratio * max_travel;

            let theme_border = theme.border;
            let theme_bg_header = theme.bg_grid_header;
            let theme_text_muted = theme.text_muted;
            let thumb_bg = Rgba { a: 0.4, ..theme_text_muted };
            let thumb_hover = Rgba { a: 0.8, ..theme_text_muted };

            let handle_left = self.horizontal_scroll_handle.clone();
            let handle_right = self.horizontal_scroll_handle.clone();
            let handle_track = self.horizontal_scroll_handle.clone();

            let scrollbar_bar = div()
                .id("grid-horizontal-scrollbar")
                .flex()
                .flex_row()
                .flex_none()
                .items_center()
                .w_full()
                .h(px(scrollbar_h))
                .bg(theme_bg_header)
                .border_t_1()
                .border_color(theme_border)
                .child(
                    div()
                        .id("scroll-btn-left")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(btn_w))
                        .h_full()
                        .cursor_pointer()
                        .text_size(px(9.0))
                        .text_color(theme_text_muted)
                        .hover(|s| s.bg(Rgba { a: 0.15, ..theme_text_muted }))
                        .child("◀")
                        .on_mouse_down(MouseButton::Left, move |_event, window, _app| {
                            let cur_x = f32::from(handle_left.offset().x);
                            let new_x = (cur_x + 250.0).min(0.0);
                            handle_left.set_offset(point(px(new_x), px(0.0)));
                            window.refresh();
                        })
                )
                .child(
                    div()
                        .id("scroll-track")
                        .flex()
                        .flex_1()
                        .items_center()
                        .h_full()
                        .relative()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |event: &MouseDownEvent, window, _app| {
                            let click_x = f32::from(event.position.x) - btn_w;
                            let target_ratio = (click_x / track_w).clamp(0.0, 1.0);
                            let new_x = -target_ratio * max_scroll_x;
                            handle_track.set_offset(point(px(new_x), px(0.0)));
                            window.refresh();
                        })
                        .child(
                            div()
                                .id("scroll-thumb")
                                .absolute()
                                .left(px(thumb_left))
                                .w(px(thumb_width))
                                .h(px(8.0))
                                .rounded_xs()
                                .bg(thumb_bg)
                                .hover(move |s| s.bg(thumb_hover))
                        )
                )
                .child(
                    div()
                        .id("scroll-btn-right")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(btn_w))
                        .h_full()
                        .cursor_pointer()
                        .text_size(px(9.0))
                        .text_color(theme_text_muted)
                        .hover(|s| s.bg(Rgba { a: 0.15, ..theme_text_muted }))
                        .child("▶")
                        .on_mouse_down(MouseButton::Left, move |_event, window, _app| {
                            let cur_x = f32::from(handle_right.offset().x);
                            let new_x = (cur_x - 250.0).max(-max_scroll_x);
                            handle_right.set_offset(point(px(new_x), px(0.0)));
                            window.refresh();
                        })
                );

            grid_container = grid_container.child(scrollbar_bar);
        }

        grid_container
    }
}

impl Render for VirtualDataGrid {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.render_grid()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mem_engine::{ColumnMeta, DataType};
    use core::prelude::v1::test;

    #[test]
    fn test_widths_follow_rows_streamed_after_schema() {
        let cols = vec![ColumnMeta::new("id", DataType::Int8), ColumnMeta::new("note", DataType::Text)];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());

        // The grid receives the pager as soon as the schema is known, before any rows
        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&pager));
        let header_only_width = grid.content_widths[1];

        pager.append_row(vec![
            Some(&b"1"[..]),
            Some(&b"A streamed note that is much wider than the column header text"[..]),
        ]).unwrap();
        grid.on_rows_appended();
        assert!(grid.content_widths[1] > header_only_width, "widths must grow with streamed rows");

        // A column the user resized keeps its width while more rows stream in
        grid.set_column_width(1, 90.0);
        pager.append_row(vec![Some(&b"2"[..]), Some(&b"another long streamed note value here"[..])]).unwrap();
        grid.on_rows_appended();
        assert_eq!(grid.column_widths[1], 90.0);
    }

    #[test]
    fn test_auto_fit_content_widths() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("email", DataType::Text),
            ColumnMeta::new("long_description", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager.append_row(vec![
            Some(&b"1"[..]),
            Some(&b"alex@domain.org"[..]),
            Some(&b"This is a substantially longer text description that requires a wider column to display nicely."[..]),
        ]).unwrap();

        let grid = VirtualDataGrid::new(Some(pager), Theme::dark());
        assert_eq!(grid.content_widths.len(), 3);

        // id should be smallest, email medium, long_description widest
        assert!(grid.content_widths[0] < grid.content_widths[1], "id should be narrower than email");
        assert!(grid.content_widths[1] < grid.content_widths[2], "email should be narrower than long_description");

        // Bounds enforcement
        assert!(grid.content_widths[0] >= MIN_COL_WIDTH);
        assert!(grid.content_widths[2] <= MAX_AUTO_COL_WIDTH);
    }

    #[test]
    fn test_short_columns_stay_snug_without_whitespace() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("key", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager.append_row(vec![
            Some(&b"1"[..]),
            Some(&b"io.ubitec.id.service.PHONE_NUMBER_VALIDATION_RULES"[..]),
        ]).unwrap();

        let mut grid = VirtualDataGrid::new(Some(pager), Theme::dark());
        grid.viewport_width = 1400.0;
        grid.apply_viewport_distribution();

        // Neither column was capped, so both should stay snug ("khít") to their content
        let key_w = grid.column_widths[1];
        assert!(key_w >= 340.0 && key_w <= 420.0, "key width ({}) should be snug to 47 chars, not inflated to 600+", key_w);
        assert!(grid.column_widths[0] < 120.0, "id width ({}) should be snug", grid.column_widths[0]);
    }

    #[test]
    fn test_long_content_horizontal_scrolling() {
        let cols = vec![
            ColumnMeta::new("key", DataType::Text),
            ColumnMeta::new("value", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        let long_json = "{\"errorCode\":9001,\"description\":\"Invalid phone number format\",\"regex\":\"^\\\\+?[0-9]{10,14}$\",\"details\":{\"service\":\"validation\",\"version\":\"2.4.1\",\"rules\":[\"E164\",\"NANP\",\"ITU-T\"]}}";
        pager.append_row(vec![
            Some(&b"phone_rules"[..]),
            Some(long_json.as_bytes()),
        ]).unwrap();

        let mut grid = VirtualDataGrid::new(Some(pager), Theme::dark());
        grid.viewport_width = 800.0;
        grid.apply_viewport_distribution();

        let total_cols_w: f32 = grid.column_widths.iter().sum();
        let total_w = total_cols_w + GUTTER_WIDTH;

        // Long data should naturally expand and total width should exceed viewport width of 800px
        assert!(grid.column_widths[1] > 600.0, "long JSON column should expand to show content");
        assert!(total_w > 800.0, "total content width ({}) should exceed viewport (800) enabling horizontal scroll", total_w);
    }

    #[test]
    fn test_manual_column_resize_and_auto_fit() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("name", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager.append_row(vec![Some(&b"1"[..]), Some(&b"Bob"[..])]).unwrap();

        let mut grid = VirtualDataGrid::new(Some(pager), Theme::dark());
        grid.viewport_width = 1000.0;
        grid.apply_viewport_distribution();

        // User drags column 0 to 250px
        grid.set_column_width(0, 250.0);
        assert_eq!(grid.column_widths[0], 250.0);
        assert!(grid.manual_column_widths.contains(&0));

        // Viewport resize shouldn't override manual column 0 width
        grid.update_viewport_width(1400.0);
        assert_eq!(grid.column_widths[0], 250.0);

        // Double-click auto-fit resets manual width
        grid.auto_fit_single_column(0);
        assert!(!grid.manual_column_widths.contains(&0));
    }
}
