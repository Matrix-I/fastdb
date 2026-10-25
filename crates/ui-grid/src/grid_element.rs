use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use gpui::*;
use mem_engine::QueryResultPager;
use ui_core::history::{EditKind, History};
use ui_core::theme::Theme;

use crate::changeset::{Changeset, RowRef};
use crate::selection::GridSelection;

pub const MIN_COL_WIDTH: f32 = 75.0;
pub const MAX_AUTO_COL_WIDTH: f32 = 1200.0;
pub const MAX_EXPANDED_COL_WIDTH: f32 = 2500.0;
pub const GUTTER_WIDTH: f32 = 52.0;
/// Number of leading rows measured when auto-fitting column widths
pub const WIDTH_SAMPLE_ROWS: u64 = 100;

/// Called with (row, col) when a data cell is pressed
pub type CellMouseDownHandler = Rc<dyn Fn(u64, usize, &MouseDownEvent, &mut Window, &mut App)>;

/// Called with the column index when the dropdown arrow of a column header is pressed
pub type HeaderMenuHandler = Rc<dyn Fn(usize, &MouseDownEvent, &mut Window, &mut App)>;

/// Server-side ordering of the result by one column, picked from the header menu
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnOrder {
    pub column: String,
    pub descending: bool,
}

/// A cell being edited in place: its text and caret, as a char index
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CellEdit {
    pub row: u64,
    pub col: usize,
    pub text: String,
    pub caret: usize,
    /// Other end of the text selection (a char index), if any
    pub anchor: Option<usize>,
}

/// Draws the content of the cell being edited; the grid supplies the cell box around it
pub type CellEditorRenderer = Rc<dyn Fn() -> AnyElement>;

/// Text of the cell at grid row `row`, staged edits and new rows included; `None` for SQL NULL
pub fn resolve_cell(pager: &QueryResultPager, changeset: &Changeset, row: u64, col: usize) -> Option<String> {
    match changeset.resolve_row(row) {
        RowRef::New(i) => changeset.inserted_rows.get(i).and_then(|r| r.values.get(col).cloned().flatten()),
        RowRef::Fetched(f) => match changeset.edited_value(f, col) {
            Some(v) => v.map(str::to_string),
            None => pager.get_cell(f, col).map(|b| String::from_utf8_lossy(&b).into_owned()),
        },
    }
}

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
    /// Undo history of the staged edits; each change made through `stage` is one step
    pub changeset_history: History<Changeset>,
    pub editing_cell: Option<CellEdit>,
    /// Ordering applied to the query; kept when a new result replaces the pager
    pub ordering: Option<ColumnOrder>,
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
            changeset_history: History::new(),
            editing_cell: None,
            ordering: None,
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
        self.changeset_history.clear();
        self.editing_cell = None;
        self.scroll_top = 0.0;
        self.scroll_left = 0.0;
        self.horizontal_scroll_handle = ScrollHandle::new();
        self.vertical_scroll_handle = UniformListScrollHandle::new();
    }

    /// Swaps in a re-read of the same columns (re-ordered, refreshed or re-filtered): column
    /// widths and the horizontal scroll stay as they are, only the rows start over from the top.
    /// A result with other columns is laid out from scratch, as `set_pager` does.
    pub fn refill_pager(&mut self, pager: Arc<QueryResultPager>) {
        let same_columns = self.pager.as_ref().is_some_and(|old| {
            old.columns.len() == pager.columns.len()
                && old.columns.iter().zip(&pager.columns).all(|(a, b)| a.name == b.name && a.data_type == b.data_type)
        });
        if !same_columns {
            self.set_pager(pager);
            return;
        }
        self.pager = Some(pager);
        // The widths already fit the columns; re-measuring the new rows would make them jump
        self.width_sampled_rows = WIDTH_SAMPLE_ROWS;
        self.selection.clear();
        self.changeset.clear();
        self.changeset_history.clear();
        self.editing_cell = None;
        self.scroll_top = 0.0;
        self.vertical_scroll_handle = UniformListScrollHandle::new();
    }

    /// Applies `change` to the staged edits as one undo step; a change that leaves them
    /// as they were records nothing
    pub fn stage<R>(&mut self, change: impl FnOnce(&mut Changeset) -> R) -> R {
        let before = self.changeset.clone();
        let result = change(&mut self.changeset);
        if self.changeset != before {
            self.changeset_history.record(before, EditKind::Other);
        }
        result
    }

    /// Steps the staged edits back (`redo == false`) or forward through their history.
    /// Returns `false` when there is no step to take.
    pub fn step_staged_edits(&mut self, redo: bool) -> bool {
        let current = self.changeset.clone();
        let restored = if redo { self.changeset_history.redo(current) } else { self.changeset_history.undo(current) };
        let Some(restored) = restored else { return false };
        let rows_moved = restored.inserted_rows.len() != self.changeset.inserted_rows.len();
        self.changeset = restored;
        self.editing_cell = None;
        // Added or dropped rows shift the rows below them, so the selection no longer fits
        if rows_moved {
            self.selection.clear();
        }
        true
    }

    /// Rows shown in the grid: the fetched rows and the rows added between them
    pub fn row_count(&self) -> u64 {
        self.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0) + self.changeset.inserted_rows.len() as u64
    }

    /// Text of a cell as shown, staged edits included; `None` for NULL or out of range
    pub fn cell_value(&self, row: u64, col: usize) -> Option<String> {
        resolve_cell(self.pager.as_ref()?, &self.changeset, row, col)
    }

    /// Text of the selected cells for the clipboard: one cell as its bare value,
    /// a range as tab-separated columns and newline-separated rows. Edited cells
    /// copy their new value, NULL copies as empty. `None` without a selection.
    pub fn selection_text(&self) -> Option<String> {
        let pager = self.pager.as_ref()?;
        let (min_r, max_r, min_c, max_c) = self.selection.bounding_box()?;
        let max_r = max_r.min(self.row_count().checked_sub(1)?);
        let max_c = max_c.min(pager.columns.len().checked_sub(1)?);
        if min_r > max_r || min_c > max_c {
            return None;
        }

        let mut rows = Vec::new();
        for r in min_r..=max_r {
            let cells: Vec<String> = (min_c..=max_c)
                .map(|c| resolve_cell(pager, &self.changeset, r, c).unwrap_or_default())
                .collect();
            rows.push(cells.join("\t"));
        }
        Some(rows.join("\n"))
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
            // 1. Column header width: name + DBeaver data type badge + key icon + dropdown arrow + padding.
            // Names are measured at a heavier weight than the estimator assumes, hence the 10% margin.
            let name_w = Self::estimate_text_width(&col.name, 12.0) * 1.1;
            let badge_str = dbeaver_type_badge(col.data_type);
            let badge_w = Self::estimate_text_width(badge_str, 10.0);
            let is_pk = col.is_primary_key || col.name.eq_ignore_ascii_case("id");
            let key_w = if is_pk { 17.0 } else { 0.0 };
            let header_w = name_w + badge_w + key_w + 50.0;

            // 2. Data sample width across up to 100 rows
            let mut max_cell_w = 0.0f32;
            for r in 0..sample_rows {
                let cell_str = resolve_cell(pager, &self.changeset, r, col_idx).unwrap_or_else(|| "NULL".to_string());

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
        let total_rows = self.row_count();
        let max_scroll_y = (total_rows as f32 * self.theme.row_height - self.viewport_height).max(0.0);

        self.scroll_top = (self.scroll_top + delta_y).clamp(0.0, max_scroll_y);
        self.scroll_left = (self.scroll_left + delta_x).max(0.0);
    }

    pub fn render_grid(&mut self) -> impl IntoElement {
        self.render_grid_interactive(None, None, None, None, |_col_idx, _width| {
            div()
                .w(px(8.0))
                .h_full()
                .cursor_col_resize()
                .into_any_element()
        })
    }

    pub fn render_grid_interactive<F>(
        &mut self,
        resizing_col: Option<usize>,
        on_cell_mouse_down: Option<CellMouseDownHandler>,
        on_header_menu: Option<HeaderMenuHandler>,
        cell_editor: Option<(u64, usize, CellEditorRenderer)>,
        mut make_resizer: F,
    ) -> impl IntoElement
    where
        F: FnMut(usize, f32) -> AnyElement,
    {
        let theme = &self.theme;
        let row_height = theme.row_height;
        let header_height = theme.header_height;
        let gutter_width = GUTTER_WIDTH;

        let total_rows = self.row_count();
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
                let sort_glyph = self
                    .ordering
                    .as_ref()
                    .filter(|o| o.column == col.name)
                    .map(|o| if o.descending { "↓" } else { "↑" });
                let mut arrow = div()
                    .id(("header-menu-arrow", col_idx))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(14.0))
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
                        .border_color(if is_resizing { theme.border_focused } else { theme.border })
                        .child(
                            // Badge, key and arrow keep their size; only the name shrinks,
                            // ending in an ellipsis once the column is too narrow for it.
                            div()
                                .flex()
                                .flex_1()
                                .min_w_0()
                                .items_center()
                                .gap_1_5()
                                .h_full()
                                .px_2()
                                .overflow_hidden()
                                .child(
                                    div()
                                        .flex_none()
                                        .font_weight(FontWeight::BOLD)
                                        .text_size(px(10.0))
                                        .text_color(rgb(0x58a6ff))
                                        .child(badge_str)
                                )
                                .children(is_pk.then(|| img("icons/key.svg").flex_none().w(px(11.0)).h(px(11.0))))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_size(px(12.0))
                                        .text_color(theme.text_primary)
                                        .truncate()
                                        .child(col.name.clone())
                                )
                                .children(sort_glyph.map(|g| {
                                    div()
                                        .flex_none()
                                        .font_weight(FontWeight::BOLD)
                                        .text_size(px(12.0))
                                        .text_color(rgb(0x58a6ff))
                                        .child(g)
                                }))
                                .child(arrow)
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
        let theme_text_primary = theme.text_primary;
        let theme_bg_cell_selected = theme.bg_grid_cell_selected;
        let theme_bg_cell_modified = theme.bg_grid_cell_modified;
        let theme_border_focused = theme.border_focused;
        let theme_bg_editor = theme.bg_editor;
        let theme_text_success = theme.text_success;
        let theme_text_error = theme.text_error;

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
                                );

                            for c in 0..num_cols {
                                let width = column_widths_clone.get(c).copied().unwrap_or(MIN_COL_WIDTH);
                                let is_selected = selection_clone.is_selected(r_u64, c);
                                let is_modified = matches!(row_ref, RowRef::Fetched(f) if changeset_clone.is_modified(f, c));

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
                                    Some(s) if s.len() > 3000 => {
                                        let truncate_idx = s.char_indices().nth(2000).map(|(i, _)| i).unwrap_or(s.len());
                                        (format!("{}…", &s[..truncate_idx]), false)
                                    }
                                    Some(s) => (s, false),
                                    None => ("NULL".to_string(), true),
                                };

                                let mut text_el = div().truncate().child(display_text);
                                if is_deleted {
                                    text_el = text_el.line_through();
                                }

                                if is_null || is_deleted {
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

        // The header scrolls horizontally in one element and the rows vertically in another,
        // so each would take its own axis from a diagonal swipe. Intercept the wheel for the
        // whole grid instead and move along the dominant axis only.
        let h_handle = self.horizontal_scroll_handle.clone();
        let v_handle = self.vertical_scroll_handle.clone();
        let wheel_router = canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || !bounds.contains(&event.position) {
                        return;
                    }
                    let delta = event.delta.pixel_delta(window.line_height());
                    let (dx, dy) = (f32::from(delta.x), f32::from(delta.y));
                    if dx.abs() > dy.abs() {
                        let max_x = f32::from(h_handle.max_offset().width);
                        let cur = h_handle.offset();
                        h_handle.set_offset(point(px((f32::from(cur.x) + dx).clamp(-max_x, 0.0)), cur.y));
                    } else if dy != 0.0 {
                        let state = v_handle.0.borrow();
                        let max_y = f32::from(state.base_handle.max_offset().height);
                        let cur = state.base_handle.offset();
                        state.base_handle.set_offset(point(cur.x, px((f32::from(cur.y) + dy).clamp(-max_y, 0.0))));
                    }
                    cx.stop_propagation();
                    window.refresh();
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full();

        let mut main_area = div()
            .flex()
            .flex_row()
            .flex_1()
            .size_full()
            .relative()
            .overflow_hidden()
            .child(scroller)
            .child(wheel_router);

        // Vertical scrollbar indicator when content exceeds viewport
        if has_vertical_overflow {
            let scroll_y = (-f32::from(self.vertical_scroll_handle.0.borrow().base_handle.offset().y)).max(0.0);
            let max_scroll_y = (total_content_height - self.viewport_height).max(1.0);
            let laid_out_max_y = f32::from(self.vertical_scroll_handle.0.borrow().base_handle.max_offset().height);
            let ratio_max_y = if laid_out_max_y > 0.0 { laid_out_max_y } else { max_scroll_y };
            let scroll_ratio = (scroll_y / ratio_max_y).clamp(0.0, 1.0);

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
            let scroll_x = (-f32::from(self.horizontal_scroll_handle.offset().x)).max(0.0);
            let max_scroll_x = (content_total_width - self.viewport_width).max(1.0);
            let laid_out_max_x = f32::from(self.horizontal_scroll_handle.max_offset().width);
            let ratio_max_x = if laid_out_max_x > 0.0 { laid_out_max_x } else { max_scroll_x };
            let scroll_ratio = (scroll_x / ratio_max_x).clamp(0.0, 1.0);

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
    fn test_selection_text_copies_cell_or_range() {
        let cols = vec![ColumnMeta::new("id", DataType::Int8), ColumnMeta::new("name", DataType::Text)];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager.append_row(vec![Some(&b"1"[..]), Some(&b"Alice"[..])]).unwrap();
        pager.append_row(vec![Some(&b"2"[..]), None]).unwrap();

        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&pager));
        assert_eq!(grid.selection_text(), None);

        grid.selection = GridSelection::single(0, 1);
        assert_eq!(grid.selection_text().as_deref(), Some("Alice"));

        grid.changeset.set_cell(0, 1, Some(b"Alice".to_vec()), Some("Alicia".to_string()));
        grid.selection.set_range(crate::GridCoord { row: 0, col: 0 }, crate::GridCoord { row: 1, col: 1 });
        assert_eq!(grid.selection_text().as_deref(), Some("1\tAlicia\n2\t"));
    }

    #[test]
    fn test_refill_keeps_column_widths_of_the_same_columns() {
        let cols = || vec![ColumnMeta::new("id", DataType::Int8), ColumnMeta::new("name", DataType::Text)];
        let first = Arc::new(QueryResultPager::new(cols()).unwrap());
        first.append_row(vec![Some(&b"1"[..]), Some(&b"Alice"[..])]).unwrap();

        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&first));
        grid.set_column_width(1, 300.0);
        grid.selection = GridSelection::single(0, 1);
        let widths = grid.column_widths.clone();

        let reordered = Arc::new(QueryResultPager::new(cols()).unwrap());
        reordered.append_row(vec![Some(&b"2"[..]), Some(&b"a much longer name than before, by far"[..])]).unwrap();
        grid.refill_pager(Arc::clone(&reordered));
        grid.on_rows_appended();
        assert_eq!(grid.column_widths, widths);
        assert!(grid.manual_column_widths.contains(&1));
        assert_eq!(grid.selection_text(), None);
        assert_eq!(grid.cell_value(0, 0).as_deref(), Some("2"));

        // Other columns are laid out from scratch
        let other = Arc::new(QueryResultPager::new(vec![ColumnMeta::new("email", DataType::Text)]).unwrap());
        grid.refill_pager(other);
        assert!(grid.manual_column_widths.is_empty());
        assert_eq!(grid.column_widths.len(), 1);
    }

    #[test]
    fn test_staged_rows_and_edits_are_shown() {
        let cols = vec![ColumnMeta::new("id", DataType::Int8), ColumnMeta::new("name", DataType::Text)];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager.append_row(vec![Some(&b"1"[..]), Some(&b"Alice"[..])]).unwrap();

        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&pager));
        grid.changeset.set_cell(0, 1, Some(b"Alice".to_vec()), None);
        grid.changeset.insert_row_at(1, vec![None, Some("Bob".to_string())]);

        assert_eq!(grid.row_count(), 2);
        assert_eq!(grid.cell_value(0, 0).as_deref(), Some("1"));
        assert_eq!(grid.cell_value(0, 1), None, "a cell set to NULL shows as NULL");
        assert_eq!(grid.cell_value(1, 0), None, "an unset cell of a new row is NULL");
        assert_eq!(grid.cell_value(1, 1).as_deref(), Some("Bob"));
        assert_eq!(grid.cell_value(2, 1), None);
    }

    #[test]
    fn test_staged_edits_undo_and_redo() {
        let cols = vec![ColumnMeta::new("id", DataType::Int8), ColumnMeta::new("name", DataType::Text)];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager.append_row(vec![Some(&b"1"[..]), Some(&b"Alice"[..])]).unwrap();

        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&pager));
        grid.stage(|cs| cs.set_cell(0, 1, Some(b"Alice".to_vec()), Some("Alicia".to_string())));
        grid.stage(|cs| cs.insert_row_at(1, vec![None, None]));
        // A change that changes nothing is not an undo step
        grid.stage(|cs| cs.set_cell(0, 1, Some(b"Alice".to_vec()), Some("Alicia".to_string())));

        assert!(grid.step_staged_edits(false));
        assert_eq!(grid.row_count(), 1);
        assert_eq!(grid.cell_value(0, 1).as_deref(), Some("Alicia"));
        assert!(grid.step_staged_edits(false));
        assert_eq!(grid.cell_value(0, 1).as_deref(), Some("Alice"));
        assert!(!grid.step_staged_edits(false));

        assert!(grid.step_staged_edits(true));
        assert_eq!(grid.cell_value(0, 1).as_deref(), Some("Alicia"));
        grid.set_pager(pager);
        assert!(!grid.step_staged_edits(false), "a new result starts with no history");
    }

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
