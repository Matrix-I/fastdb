use gpui::*;
use mem_engine::QueryResultPager;
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;
use ui_core::history::{EditKind, History};
use ui_core::theme::Theme;

use crate::changeset::{Changeset, RowRef};
use crate::selection::GridSelection;

pub const MIN_COL_WIDTH: f32 = 75.0;
/// Widest a column auto-fits to; longer content is cut with an ellipsis until the user drags it wider
pub const MAX_AUTO_COL_WIDTH: f32 = 480.0;
pub const GUTTER_WIDTH: f32 = 52.0;
/// Number of leading rows measured when auto-fitting column widths
pub const WIDTH_SAMPLE_ROWS: u64 = 100;

// Header cell geometry, kept in sync with the header row built in `render_grid_interactive`
const HEADER_PADDING_X: f32 = 8.0;
const HEADER_GAP: f32 = 6.0;
const HEADER_ARROW_WIDTH: f32 = 14.0;
const HEADER_KEY_WIDTH: f32 = 11.0;
/// Width of the drag handle the app appends to every header cell
const HEADER_RESIZER_WIDTH: f32 = 8.0;
// Data cell geometry, kept in sync with the cells built in `render_grid_interactive`
const CELL_PADDING_X: f32 = 8.0;
/// Chars of a cell measured for its width; anything longer is past `MAX_AUTO_COL_WIDTH` anyway
const CELL_MEASURE_CHARS: usize = 200;

/// Width a data cell needs to show `text_w` px of text in full: padding, right border and
/// a pixel of slack so sub-pixel rounding never triggers the ellipsis
fn data_cell_width(text_w: f32) -> f32 {
    text_w + 2.0 * CELL_PADDING_X + 2.0
}

/// Leading part of a cell's text that is measured for the column width
fn cell_measure_sample(text: &str) -> &str {
    match text.char_indices().nth(CELL_MEASURE_CHARS) {
        Some((i, _)) => &text[..i],
        None => text,
    }
}

/// Called with (row, col) when a data cell is pressed with the left or right button
pub type CellMouseDownHandler = Rc<dyn Fn(u64, usize, &MouseDownEvent, &mut Window, &mut App)>;

/// Called with (row, col) while the left button moves over a data cell.
pub type CellMouseMoveHandler = Rc<dyn Fn(u64, usize, &MouseMoveEvent, &mut Window, &mut App)>;

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
pub fn resolve_cell(
    pager: &QueryResultPager,
    changeset: &Changeset,
    row: u64,
    col: usize,
) -> Option<String> {
    match changeset.resolve_row(row) {
        RowRef::New(i) => changeset
            .inserted_rows
            .get(i)
            .and_then(|r| r.values.get(col).cloned().flatten()),
        RowRef::Fetched(f) => match changeset.edited_value(f, col) {
            Some(v) => v.map(str::to_string),
            None => pager
                .get_cell(f, col)
                .map(|b| String::from_utf8_lossy(&b).into_owned()),
        },
    }
}

/// Columns of a row that intersect the horizontally scrolled viewport, plus one on each
/// side, and the x offset (after the gutter) where the first of them starts. Rows only
/// build these cells, so a wide table costs no more per frame than a narrow one.
pub fn visible_columns(
    widths: &[f32],
    scroll_x: f32,
    viewport_width: f32,
) -> (std::ops::Range<usize>, f32) {
    let (view_start, view_end) = (
        scroll_x - GUTTER_WIDTH,
        scroll_x + viewport_width - GUTTER_WIDTH,
    );
    let mut x = 0.0;
    let mut first = None;
    let mut end = widths.len();
    for (c, w) in widths.iter().enumerate() {
        if first.is_none() && x + w > view_start {
            first = Some(c);
        }
        if x >= view_end {
            end = c;
            break;
        }
        x += w;
    }
    let first = first.unwrap_or(widths.len()).saturating_sub(1);
    let end = (end + 1).min(widths.len()).max(first);
    (first..end, widths[..first].iter().sum())
}

/// Cell text cut to what a column of `width` px can show; the rest would only be
/// measured and dropped by the ellipsis on every frame. Kept long enough to still overflow.
fn clip_cell_text(mut text: String, width: f32) -> String {
    let max_chars = (width / 2.5) as usize + 1;
    if let Some((i, _)) = text.char_indices().nth(max_chars) {
        text.truncate(i);
    }
    text
}

/// Width a header cell needs to show its name in full, from the widths of its texts.
/// Mirrors the header layout: padding + badge + [key] + name + [sort glyph] + arrow,
/// a gap between each of them, then the resizer and the right border.
fn header_cell_width(badge_w: f32, name_w: f32, is_pk: bool, sort_glyph_w: Option<f32>) -> f32 {
    let mut items_w = badge_w + name_w + HEADER_ARROW_WIDTH;
    let mut gaps = 2.0;
    if is_pk {
        items_w += HEADER_KEY_WIDTH;
        gaps += 1.0;
    }
    if let Some(w) = sort_glyph_w {
        items_w += w;
        gaps += 1.0;
    }
    2.0 * HEADER_PADDING_X + items_w + gaps * HEADER_GAP + HEADER_RESIZER_WIDTH + 1.0
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

impl VirtualDataGrid {
    pub fn render_grid(&mut self) -> impl IntoElement + use<> {
        self.render_grid_interactive(None, None, None, None, None, |_col_idx, _width| {
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
        on_cell_mouse_move: Option<CellMouseMoveHandler>,
        on_header_menu: Option<HeaderMenuHandler>,
        cell_editor: Option<(u64, usize, CellEditorRenderer)>,
        make_resizer: F,
    ) -> impl IntoElement + use<F>
    where
        F: FnMut(usize, f32) -> AnyElement,
    {
        let total_width =
            (GUTTER_WIDTH + self.column_widths.iter().sum::<f32>()).max(self.viewport_width);
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

        scroller = scroller.child(self.render_header(
            total_width,
            resizing_col,
            on_header_menu,
            make_resizer,
        ));

        scroller = scroller.child(self.render_rows(
            total_width,
            on_cell_mouse_down,
            on_cell_mouse_move,
            cell_editor,
        ));

        self.render_scrolled_area(scroller.into_any_element())
    }
}

impl Render for VirtualDataGrid {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.render_grid()
    }
}

mod header;
mod rows;
mod scrollbars;
mod state;
#[cfg(test)]
mod tests;
mod widths;
pub use state::VirtualDataGrid;
