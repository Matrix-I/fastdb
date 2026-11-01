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
    /// Header widths measured with the window's fonts; the character estimate stands in until then
    pub header_widths: Vec<f32>,
    /// Pager and ordering `header_widths` were measured for
    pub(super) header_widths_key: Option<(usize, Option<ColumnOrder>)>,
    /// Widest data cell per column, measured with the window's fonts over the first
    /// `cell_widths_rows` rows; the character estimate covers the rows after them
    pub cell_widths: Vec<f32>,
    pub(super) cell_widths_rows: u64,
    /// The sampled rows changed since `cell_widths` were measured
    pub(super) cell_widths_stale: bool,
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
            column_widths: Vec::new(),
            manual_column_widths: HashSet::new(),
            width_sampled_rows: 0,
            selection: GridSelection::new(),
            changeset: Changeset::new(),
            changeset_history: History::new(),
            editing_cell: None,
            ordering: None,
            header_widths: Vec::new(),
            header_widths_key: None,
            cell_widths: Vec::new(),
            cell_widths_rows: 0,
            cell_widths_stale: false,
            horizontal_scroll_handle: ScrollHandle::new(),
            vertical_scroll_handle: UniformListScrollHandle::new(),
        };
        grid.recalculate_all_widths();
        grid
    }

    pub fn set_pager(&mut self, pager: Arc<QueryResultPager>) {
        self.pager = Some(pager);
        self.manual_column_widths.clear();
        self.header_widths.clear();
        self.header_widths_key = None;
        self.cell_widths.clear();
        self.cell_widths_rows = 0;
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
}
