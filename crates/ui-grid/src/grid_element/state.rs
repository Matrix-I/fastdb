use super::*;
pub struct VirtualDataGrid {
    pub pager: Option<Arc<QueryResultPager>>,
    pub theme: Theme,
    pub table_name: String,
    pub scroll_top: f32,
    pub scroll_left: f32,
    pub viewport_height: f32,
    pub viewport_width: f32,
    pub content_widths: Vec<f32>,
    pub column_widths: Vec<f32>,
    display_columns: Vec<usize>,
    pub manual_column_widths: HashSet<usize>,
    /// Rows already measured for column widths; grows while results stream in
    pub width_sampled_rows: u64,
    pub selection: GridSelection,
    pub changeset: Changeset,
    changeset_revision: u64,
    /// Undo history of the staged edits; each change made through `stage` is one step
    pub changeset_history: History<Changeset>,
    pub editing_cell: Option<CellEdit>,
    /// Ordering applied to the query; kept when a new result replaces the pager
    pub ordering: Vec<ColumnOrder>,
    /// Header widths measured with the window's fonts; the character estimate stands in until then
    pub header_widths: Vec<f32>,
    /// Pager and ordering `header_widths` were measured for
    pub(super) header_widths_key: Option<(usize, Vec<ColumnOrder>)>,
    /// Widest data cell per column, measured with the window's fonts over the first
    /// `cell_widths_rows` rows; the character estimate covers the rows after them
    pub cell_widths: Vec<f32>,
    pub(super) cell_widths_rows: u64,
    /// The sampled rows changed since `cell_widths` were measured
    pub(super) cell_widths_stale: bool,
    pub horizontal_scroll_handle: ScrollHandle,
    pub vertical_scroll_handle: UniformListScrollHandle,
    /// Programmatic search navigation must not request more database rows.
    /// Manual vertical scrolling clears this hold, including after Find closes.
    pub auto_fetch_paused: Rc<std::cell::Cell<bool>>,
}

impl VirtualDataGrid {
    pub fn new(pager: Option<Arc<QueryResultPager>>, theme: Theme) -> Self {
        let display_columns = pager_column_indexes(&pager);
        let mut grid = Self {
            pager,
            theme,
            table_name: "query_result".to_string(),
            scroll_top: 0.0,
            scroll_left: 0.0,
            viewport_height: 600.0,
            viewport_width: 1000.0,
            content_widths: Vec::new(),
            column_widths: Vec::new(),
            display_columns,
            manual_column_widths: HashSet::new(),
            width_sampled_rows: 0,
            selection: GridSelection::new(),
            changeset: Changeset::new(),
            changeset_revision: 0,
            changeset_history: History::new(),
            editing_cell: None,
            ordering: Vec::new(),
            header_widths: Vec::new(),
            header_widths_key: None,
            cell_widths: Vec::new(),
            cell_widths_rows: 0,
            cell_widths_stale: false,
            horizontal_scroll_handle: ScrollHandle::new(),
            vertical_scroll_handle: UniformListScrollHandle::new(),
            auto_fetch_paused: Rc::new(std::cell::Cell::new(false)),
        };
        grid.recalculate_all_widths();
        grid
    }

    pub fn set_pager(&mut self, pager: Arc<QueryResultPager>) {
        self.auto_fetch_paused.set(false);
        self.display_columns = (0..pager.columns.len()).collect();
        self.pager = Some(pager);
        self.manual_column_widths.clear();
        self.header_widths.clear();
        self.header_widths_key = None;
        self.cell_widths.clear();
        self.cell_widths_rows = 0;
        self.recalculate_all_widths();
        self.selection.clear();
        self.changeset.clear();
        self.changeset_revision = self.changeset_revision.wrapping_add(1);
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
        self.auto_fetch_paused.set(false);
        let same_columns = self.pager.as_ref().is_some_and(|old| {
            old.columns.len() == pager.columns.len()
                && old
                    .columns
                    .iter()
                    .zip(&pager.columns)
                    .all(|(a, b)| a.name == b.name && a.data_type == b.data_type)
        });
        if !same_columns {
            self.set_pager(pager);
            return;
        }
        self.pager = Some(pager);
        // The widths already fit the columns; re-measuring the new rows would make them jump
        self.width_sampled_rows = WIDTH_SAMPLE_ROWS;
        self.cell_widths.clear();
        self.cell_widths_rows = 0;
        self.cell_widths_stale = false;
        self.selection.clear();
        self.changeset.clear();
        self.changeset_revision = self.changeset_revision.wrapping_add(1);
        self.changeset_history.clear();
        self.editing_cell = None;
        self.scroll_top = 0.0;
        self.vertical_scroll_handle = UniformListScrollHandle::new();
    }

    pub fn clear_saved_edits(&mut self) {
        self.changeset.clear();
        self.changeset_history.clear();
        self.selection.clear();
        self.changeset_revision = self.changeset_revision.wrapping_add(1);
    }

    pub fn changeset_revision(&self) -> u64 {
        self.changeset_revision
    }

    /// Applies `change` to the staged edits as one undo step; a change that leaves them
    /// as they were records nothing
    pub fn stage<R>(&mut self, change: impl FnOnce(&mut Changeset) -> R) -> R {
        let before = self.changeset.clone();
        let result = change(&mut self.changeset);
        if self.changeset != before {
            self.changeset_history.record(before, EditKind::Other);
            self.changeset_revision = self.changeset_revision.wrapping_add(1);
        }
        result
    }

    /// Steps the staged edits back (`redo == false`) or forward through their history.
    /// Returns `false` when there is no step to take.
    pub fn step_staged_edits(&mut self, redo: bool) -> bool {
        let current = self.changeset.clone();
        let restored = if redo {
            self.changeset_history.redo(current)
        } else {
            self.changeset_history.undo(current)
        };
        let Some(restored) = restored else {
            return false;
        };
        let rows_moved = restored.inserted_rows.len() != self.changeset.inserted_rows.len();
        self.changeset = restored;
        self.changeset_revision = self.changeset_revision.wrapping_add(1);
        self.editing_cell = None;
        // Added or dropped rows shift the rows below them, so the selection no longer fits
        if rows_moved {
            self.selection.clear();
        }
        true
    }

    /// Rows shown in the grid: the fetched rows and the rows added between them
    pub fn row_count(&self) -> u64 {
        self.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0)
            + self.changeset.inserted_rows.len() as u64
    }

    /// Text of a cell as shown, staged edits included; `None` for NULL or out of range
    pub fn cell_value(&self, row: u64, col: usize) -> Option<String> {
        resolve_cell(self.pager.as_ref()?, &self.changeset, row, col)
    }

    pub fn display_columns(&self) -> &[usize] {
        &self.display_columns
    }

    pub fn adjacent_column(&self, column: usize, forward: bool) -> Option<usize> {
        let index = self
            .display_columns
            .iter()
            .position(|&candidate| candidate == column)?;
        let adjacent = if forward {
            index.checked_add(1)?
        } else {
            index.checked_sub(1)?
        };
        self.display_columns.get(adjacent).copied()
    }

    pub fn hide_column(&mut self, column: usize) {
        if self.editing_cell.is_none()
            && self.display_columns.len() > 1
            && let Some(index) = self
                .display_columns
                .iter()
                .position(|&candidate| candidate == column)
        {
            self.display_columns.remove(index);
            self.reset_column_interaction();
        }
    }

    pub fn move_column(&mut self, column: usize, forward: bool) {
        if self.editing_cell.is_none()
            && let Some(neighbor) = self.adjacent_column(column, forward)
        {
            let index = self
                .display_columns
                .iter()
                .position(|&candidate| candidate == column)
                .unwrap();
            let other = if forward { index + 1 } else { index - 1 };
            debug_assert_eq!(self.display_columns[other], neighbor);
            self.display_columns.swap(index, other);
            self.reset_column_interaction();
        }
    }

    pub fn reset_column_layout(&mut self) {
        if self.editing_cell.is_none() {
            self.display_columns = pager_column_indexes(&self.pager);
            self.reset_column_interaction();
        }
    }

    fn reset_column_interaction(&mut self) {
        self.selection.clear();
        self.scroll_left = 0.0;
        self.horizontal_scroll_handle
            .set_offset(point(px(0.0), px(0.0)));
    }

    pub fn selection_display_range(&self) -> Option<(u64, u64, std::ops::RangeInclusive<usize>)> {
        let (anchor, focus) = (self.selection.anchor?, self.selection.focus?);
        let anchor_col = self
            .display_columns
            .iter()
            .position(|&col| col == anchor.col)?;
        let focus_col = self
            .display_columns
            .iter()
            .position(|&col| col == focus.col)?;
        let first_row = anchor.row.min(focus.row);
        let last_row = anchor
            .row
            .max(focus.row)
            .min(self.row_count().checked_sub(1)?);
        (first_row <= last_row).then_some((
            first_row,
            last_row,
            anchor_col.min(focus_col)..=anchor_col.max(focus_col),
        ))
    }

    pub fn displayed_column_width(&self, column: usize) -> f32 {
        self.column_widths
            .get(column)
            .copied()
            .unwrap_or(MIN_COL_WIDTH)
    }

    pub(super) fn displayed_width(&self) -> f32 {
        self.display_columns
            .iter()
            .map(|&col| self.displayed_column_width(col))
            .sum()
    }

    /// Text of the selected cells for the clipboard: one cell as its bare value,
    /// a range as tab-separated columns and newline-separated rows. Edited cells
    /// copy their new value, NULL copies as empty. `None` without a selection.
    pub fn selection_text(&self) -> Option<String> {
        let pager = self.pager.as_ref()?;
        let (min_r, max_r, columns) = self.selection_display_range()?;
        let columns = &self.display_columns[columns];

        let mut rows = Vec::new();
        for r in min_r..=max_r {
            let cells: Vec<String> = columns
                .iter()
                .map(|&c| resolve_cell(pager, &self.changeset, r, c).unwrap_or_default())
                .collect();
            rows.push(cells.join("\t"));
        }
        Some(rows.join("\n"))
    }

    pub fn set_table_name(&mut self, name: impl Into<String>) {
        self.table_name = name.into();
    }
}

fn pager_column_indexes(pager: &Option<Arc<QueryResultPager>>) -> Vec<usize> {
    (0..pager.as_ref().map_or(0, |pager| pager.columns.len())).collect()
}
