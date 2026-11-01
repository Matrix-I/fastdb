use std::time::{Duration, Instant};

use db_wire::QueryResultPager;

use super::*;
use std::path::PathBuf;

use super::sql_script::paged_select;

/// Rows fetched per round trip until the user picks another fetch size
pub const DEFAULT_FETCH_SIZE: usize = 200;

pub struct TabState {
    /// Stable identity, so async query results land in the right tab after tabs move or close
    pub id: u64,
    pub title: String,
    pub schema_name: Option<String>,
    pub table_name: Option<String>,
    pub filter_sql: String,
    pub filter_history: LineHistory,
    pub active_subtab: TableSubTab,
    pub active_property_section: PropertySection,
    pub properties_header_collapsed: bool,
    pub properties_scroll: ScrollHandle,
    pub view_mode: DataViewMode,
    pub show_sql_dialog: bool,
    pub editor: SqlEditorView,
    pub grid: VirtualDataGrid,
    pub execution_time_ms: Option<u64>,
    pub fetch: FetchState,
    /// Statement of a script tab's current result, read again by "fetch next"
    pub script_sql: Option<String>,
    pub query_plan: Option<db_wire::query_plan::QueryPlan>,
    pub plan_requested: bool,
    pub show_plan: bool,
    /// Message of the last failed run, cleared when the next run starts
    pub last_error: Option<String>,
    /// In a script tab, whether keys go to the editor rather than the result grid
    pub editor_focused: bool,
    /// File a script tab is saved to
    pub script_path: Option<PathBuf>,
    /// Saved connection a script tab runs on
    pub connection_id: Option<String>,
    /// Text last written to the script file, to tell whether the editor has unsaved changes
    pub saved_text: String,
    pub saved_revision: u64,
    pub(super) results: Vec<ScriptResult>,
    pub(super) active_result: usize,
    pub(super) pending_statements: std::collections::VecDeque<String>,
    pub(super) show_history: bool,
    pub(super) query_history: Arc<Vec<String>>,
    pub(super) file_busy: bool,
}

/// Inactive results own their grid, pager and fetch state; switching swaps the complete
/// view state instead of copying rows or borrowing another result's SQL.
pub(super) struct ScriptResult {
    grid: VirtualDataGrid,
    fetch: FetchState,
    sql: Option<String>,
    error: Option<String>,
    elapsed: Option<u64>,
    query_plan: Option<db_wire::query_plan::QueryPlan>,
    show_plan: bool,
}

/// DBeaver-style incremental fetching: the first run reads `size` rows, and reaching the
/// end of the grid reads the next `size` rows with an OFFSET and appends them in place.
pub struct FetchState {
    /// Rows requested per round trip
    pub size: usize,
    /// Text of the fetch size box in the result footer
    pub size_text: String,
    pub size_history: LineHistory,
    /// The current result was read with our own LIMIT, so more chunks can follow
    pub paged: bool,
    /// The last chunk came back full, so the table may hold more rows
    pub has_more: bool,
    /// Set while a "fetch next" run copies its rows onto the end of the current result
    append: Option<AppendSource>,
    started_at: Option<Instant>,
    schema_ready_at: Option<Instant>,
    pub last: Option<FetchSummary>,
}

impl Default for FetchState {
    fn default() -> Self {
        Self {
            size: DEFAULT_FETCH_SIZE,
            size_text: DEFAULT_FETCH_SIZE.to_string(),
            size_history: LineHistory::new(),
            paged: false,
            has_more: false,
            append: None,
            started_at: None,
            schema_ready_at: None,
            last: None,
        }
    }
}

impl FetchState {
    /// Applies the fetch size box, falling back to the current size when it is empty or zero
    pub fn commit_size_text(&mut self) {
        match self.size_text.trim().parse::<usize>() {
            Ok(n) if n > 0 => self.size = n,
            _ => {}
        }
        self.size_text = self.size.to_string();
    }

    pub fn is_appending(&self) -> bool {
        self.append.is_some()
    }
}

/// Rows of a "fetch next" run, read into their own pager and copied onto the grid's pager
#[derive(Default)]
struct AppendSource {
    pager: Option<Arc<QueryResultPager>>,
    copied: u64,
}

/// Status line of the last completed fetch, as DBeaver shows it under the result
#[derive(Debug, Clone)]
pub struct FetchSummary {
    pub total_rows: u64,
    pub elapsed: Duration,
    pub fetch: Duration,
    pub finished_at: chrono::DateTime<chrono::Local>,
}

impl FetchSummary {
    pub fn message(&self) -> String {
        format!(
            "{} row(s) fetched - {:.3}s ({:.3}s fetch), on {} at {}",
            self.total_rows,
            self.elapsed.as_secs_f64(),
            self.fetch.as_secs_f64(),
            self.finished_at.format("%Y-%m-%d"),
            self.finished_at.format("%H:%M:%S"),
        )
    }
}

/// SQL reading one chunk of a script tab's statement from `offset`, and whether it is paged.
/// Statements that are not plain SELECTs run as written, in one go.
pub fn script_fetch_query(tab: &TabState, offset: u64) -> (String, bool) {
    let Some(ref statement) = tab.script_sql else {
        return (String::new(), false);
    };
    match paged_select(statement, tab.fetch.size, offset) {
        Some(sql) => (sql, true),
        None => (statement.clone(), false),
    }
}

/// Formats a row count with thousands separators: 1400 -> "1,400"
pub fn group_thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Outcome of applying one streaming event to a tab
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryStatus {
    Running { rows: u64 },
    Done { rows: u64, elapsed_ms: u64 },
    Failed(String),
}

impl TabState {
    /// Tab showing the rows of `schema.table`, opened on its Data sub-tab
    pub fn table(
        id: u64,
        schema: &str,
        table: &str,
        theme: Theme,
        cache: Arc<SchemaCache>,
    ) -> Self {
        let mut grid = VirtualDataGrid::new(None, theme.clone());
        grid.set_table_name(table);
        Self {
            title: table.to_string(),
            schema_name: Some(schema.to_string()),
            table_name: Some(table.to_string()),
            editor: SqlEditorView::new("", theme, Some(cache)),
            grid,
            ..Self::blank(id)
        }
    }

    /// Tab editing the saved script `script`, whose file holds `text`; the editor has the keyboard
    pub fn script(
        id: u64,
        script: &ScriptFile,
        text: &str,
        theme: Theme,
        cache: Arc<SchemaCache>,
    ) -> Self {
        Self {
            title: script.name.clone(),
            editor: SqlEditorView::new(text, theme.clone(), Some(cache)),
            grid: VirtualDataGrid::new(None, theme),
            editor_focused: true,
            script_path: Some(script.path.clone()),
            connection_id: Some(script.connection_id.clone()),
            saved_text: text.to_string(),
            ..Self::blank(id)
        }
    }

    /// Whether a script tab's editor holds changes not yet written to its file
    pub fn mark_saved(&mut self, revision: u64, text: String) {
        if revision >= self.saved_revision {
            self.saved_revision = revision;
            self.saved_text = text;
        }
    }
    pub fn is_dirty(&self) -> bool {
        self.script_path.is_some() && self.editor.buffer.revision() != self.saved_revision
    }

    /// Whether this tab runs SQL typed in its editor rather than reading a table
    pub fn is_script(&self) -> bool {
        self.table_name.is_none()
    }

    fn blank(id: u64) -> Self {
        Self {
            id,
            title: String::new(),
            schema_name: None,
            table_name: None,
            filter_sql: String::new(),
            filter_history: Default::default(),
            active_subtab: TableSubTab::Data,
            active_property_section: PropertySection::Columns,
            properties_header_collapsed: false,
            properties_scroll: ScrollHandle::new(),
            view_mode: DataViewMode::Grid,
            show_sql_dialog: false,
            editor: SqlEditorView::new("", Theme::dark(), None),
            grid: VirtualDataGrid::new(None, Theme::dark()),
            execution_time_ms: None,
            fetch: FetchState::default(),
            script_sql: None,
            query_plan: None,
            plan_requested: false,
            show_plan: false,
            last_error: None,
            editor_focused: false,
            script_path: None,
            connection_id: None,
            saved_text: String::new(),
            saved_revision: 0,
            results: Vec::new(),
            active_result: 0,
            pending_statements: Default::default(),
            show_history: false,
            query_history: Arc::new(Vec::new()),
            file_busy: false,
        }
    }

    pub(super) fn result_count(&self) -> usize {
        self.results.len() + usize::from(self.script_sql.is_some())
    }

    pub(super) fn select_result(&mut self, index: usize) {
        if index >= self.result_count() || index == self.active_result {
            return;
        }
        let slot = if index < self.active_result {
            index
        } else {
            index - 1
        };
        let result = &mut self.results[slot];
        std::mem::swap(&mut self.grid, &mut result.grid);
        std::mem::swap(&mut self.fetch, &mut result.fetch);
        std::mem::swap(&mut self.script_sql, &mut result.sql);
        std::mem::swap(&mut self.last_error, &mut result.error);
        std::mem::swap(&mut self.execution_time_ms, &mut result.elapsed);
        std::mem::swap(&mut self.query_plan, &mut result.query_plan);
        std::mem::swap(&mut self.show_plan, &mut result.show_plan);
        // Keep inactive results in display order after removing the selected slot.
        let result = self.results.remove(slot);
        let old_slot = if index < self.active_result {
            self.active_result - 1
        } else {
            self.active_result
        };
        self.results.insert(old_slot, result);
        self.active_result = index;
    }

    pub(super) fn new_script_result(&mut self, sql: String) {
        let fetch_size = self.fetch.size;
        if self.script_sql.is_some() {
            let result = ScriptResult {
                grid: std::mem::replace(
                    &mut self.grid,
                    VirtualDataGrid::new(None, self.editor.theme.clone()),
                ),
                fetch: std::mem::take(&mut self.fetch),
                sql: self.script_sql.take(),
                error: self.last_error.take(),
                elapsed: self.execution_time_ms.take(),
                query_plan: self.query_plan.take(),
                show_plan: std::mem::take(&mut self.show_plan),
            };
            self.results
                .insert(self.active_result.min(self.results.len()), result);
            // Bound retained grid metadata and spill files as well as row memory.
            if self.results.len() >= 20 {
                self.results.remove(0);
            }
        }
        self.fetch.size = fetch_size;
        self.fetch.size_text = fetch_size.to_string();
        self.active_result = self.results.len();
        self.script_sql = Some(sql);
    }

    /// Rows currently shown in the grid
    pub fn loaded_rows(&self) -> u64 {
        self.grid
            .pager
            .as_ref()
            .map(|p| p.total_rows())
            .unwrap_or(0)
    }

    /// Marks the start of a run; `append` keeps the current rows and adds the new ones after them
    pub fn begin_fetch(&mut self, paged: bool, append: bool) {
        self.query_plan = None;
        self.plan_requested = false;
        self.show_plan = false;
        self.fetch.paged = paged;
        self.fetch.append = append.then(AppendSource::default);
        self.fetch.started_at = Some(Instant::now());
        self.fetch.schema_ready_at = None;
        self.last_error = None;
    }

    /// Applies a streaming query event to this tab's grid
    pub fn apply_stream_event(&mut self, event: StreamEvent) -> QueryStatus {
        match event {
            StreamEvent::SchemaReady(pager) => {
                self.fetch.schema_ready_at = Some(Instant::now());
                if let Some(ref mut append) = self.fetch.append {
                    append.pager = Some(pager);
                } else {
                    self.grid.refill_pager(pager);
                    if let Some(ref t_name) = self.table_name {
                        self.grid.set_table_name(t_name.clone());
                    }
                    self.execution_time_ms = None;
                }
                QueryStatus::Running {
                    rows: self.loaded_rows(),
                }
            }
            StreamEvent::PageAppended { .. } => {
                if let Err(e) = self.copy_appended_rows() {
                    return self.fail_fetch(e);
                }
                self.grid.on_rows_appended();
                QueryStatus::Running {
                    rows: self.loaded_rows(),
                }
            }
            StreamEvent::Completed {
                total_rows,
                elapsed_ms,
            } => {
                if let Err(e) = self.copy_appended_rows() {
                    return self.fail_fetch(e);
                }
                self.grid.on_rows_appended();
                self.fetch.append = None;
                self.fetch.has_more = self.fetch.paged && total_rows >= self.fetch.size as u64;
                self.execution_time_ms = Some(elapsed_ms);

                let now = Instant::now();
                let started = self.fetch.started_at.take().unwrap_or(now);
                let schema_ready = self.fetch.schema_ready_at.take().unwrap_or(started);
                let rows = self.loaded_rows();
                self.fetch.last = Some(FetchSummary {
                    total_rows: rows,
                    elapsed: now - started,
                    fetch: now - schema_ready,
                    finished_at: chrono::Local::now(),
                });
                if self.plan_requested {
                    self.plan_requested = false;
                    let parsed = self
                        .grid
                        .pager
                        .as_ref()
                        .and_then(|p| p.get_cell(0, 0))
                        .ok_or_else(|| anyhow::anyhow!("EXPLAIN returned no plan"))
                        .and_then(|bytes| db_wire::query_plan::QueryPlan::parse(&bytes));
                    match parsed {
                        Ok(plan) => {
                            self.query_plan = Some(plan);
                            self.show_plan = true;
                        }
                        Err(error) => return self.fail_fetch(error.to_string()),
                    }
                }
                QueryStatus::Done { rows, elapsed_ms }
            }
            StreamEvent::Error(e) => self.fail_fetch(e),
        }
    }

    fn fail_fetch(&mut self, error: String) -> QueryStatus {
        // Rows copied before the failure stay; the next scroll to the end retries from there
        self.pending_statements.clear();
        self.plan_requested = false;
        self.fetch.append = None;
        self.fetch.started_at = None;
        self.fetch.schema_ready_at = None;
        self.last_error = Some(error.clone());
        QueryStatus::Failed(error)
    }

    /// Copies rows that arrived in a "fetch next" pager onto the end of the grid's pager
    fn copy_appended_rows(&mut self) -> Result<(), String> {
        let Some(ref mut append) = self.fetch.append else {
            return Ok(());
        };
        let (Some(source), Some(target)) = (append.pager.as_ref(), self.grid.pager.as_ref()) else {
            return Ok(());
        };
        if source.num_cols() != target.num_cols()
            || source
                .columns
                .iter()
                .zip(&target.columns)
                .any(|(a, b)| a.name != b.name || a.data_type != b.data_type)
        {
            return Err(
                "The table's columns changed since the first fetch; run the query again."
                    .to_string(),
            );
        }
        let num_cols = source.num_cols() as usize;
        let available = source.total_rows();
        while append.copied < available {
            let row = (0..num_cols).map(|c| source.get_cell(append.copied, c));
            target.append_row(row).map_err(|e| format!("{e:#}"))?;
            append.copied += 1;
        }
        Ok(())
    }
}

impl TabState {
    pub(super) fn finish_row_save(
        &mut self,
        saved: &ui_grid::Changeset,
        result: Result<u64, String>,
    ) -> bool {
        if let Err(error) = result {
            self.last_error = Some(format!("Could not save row changes: {error}"));
            return false;
        }
        if self.grid.changeset != *saved {
            return false;
        }
        self.last_error = None;
        self.grid.changeset.clear();
        self.grid.changeset_history.clear();
        self.grid.selection.clear();
        true
    }
}
#[cfg(test)]
mod relocated_tests {
    #[test]
    fn test_property_sections_and_subtabs() {
        use crate::workspace::{PropertySection, TabState, TableSubTab};
        use ui_core::theme::Theme;

        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        let mut tab = TabState::table(1, "public", "additionalInfo", Theme::dark(), cache);
        tab.active_subtab = TableSubTab::Properties;

        assert_eq!(tab.active_subtab, TableSubTab::Properties);
        assert_eq!(tab.active_property_section, PropertySection::Columns);
        assert!(!tab.properties_header_collapsed);
    }

    fn sample_table_tab() -> crate::workspace::TabState {
        use ui_core::theme::Theme;
        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        crate::workspace::TabState::table(7, "public", "users", Theme::dark(), cache)
    }

    fn sample_script_tab(text: &str) -> crate::workspace::TabState {
        use crate::workspace::{ScriptFile, TabState};
        use ui_core::theme::Theme;
        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        let script = ScriptFile {
            path: std::path::PathBuf::from("/tmp/Script-4.sql"),
            connection_id: "pg".to_string(),
            name: "Script-4".to_string(),
            modified: std::time::SystemTime::now(),
            preview: String::new(),
        };
        TabState::script(3, &script, text, Theme::dark(), cache)
    }

    #[test]
    fn test_script_tab_title_text_and_dirty_flag_come_from_its_file() {
        let mut tab = sample_script_tab("SELECT 1;");
        assert_eq!(tab.title, "Script-4");
        assert_eq!(tab.connection_id.as_deref(), Some("pg"));
        assert_eq!(tab.editor.buffer.text(), "SELECT 1;");
        assert!(!tab.is_dirty());
        tab.editor.insert_text("\nSELECT 2;");
        assert!(tab.is_dirty());
        assert!(!sample_table_tab().is_dirty());
    }

    #[test]
    fn test_failed_run_keeps_its_message_until_the_next_run() {
        use db_wire::StreamEvent;

        let mut tab = sample_table_tab();
        tab.begin_fetch(true, false);
        tab.apply_stream_event(StreamEvent::Error(
            "relation \"users\" does not exist".to_string(),
        ));
        assert_eq!(
            tab.last_error.as_deref(),
            Some("relation \"users\" does not exist")
        );

        tab.begin_fetch(true, false);
        assert_eq!(tab.last_error, None);
    }

    #[test]
    fn test_script_fetch_query_pages_selects_and_runs_other_statements_as_written() {
        use crate::workspace::script_fetch_query;
        let mut tab = sample_script_tab("");
        tab.fetch.size = 100;

        tab.script_sql = Some("SELECT * FROM users;".to_string());
        assert_eq!(
            script_fetch_query(&tab, 0),
            (
                "SELECT * FROM (SELECT * FROM users\n) fastdb_page LIMIT 100".to_string(),
                true
            )
        );
        assert_eq!(
            script_fetch_query(&tab, 100).0,
            "SELECT * FROM (SELECT * FROM users\n) fastdb_page LIMIT 100 OFFSET 100"
        );

        tab.script_sql = Some("DELETE FROM users WHERE id = 1;".to_string());
        assert_eq!(
            script_fetch_query(&tab, 0),
            ("DELETE FROM users WHERE id = 1;".to_string(), false)
        );

        tab.script_sql = None;
        assert_eq!(script_fetch_query(&tab, 0), (String::new(), false));
    }

    #[test]
    fn test_script_tabs_start_empty_with_the_editor_focused() {
        let tab = sample_script_tab("");
        assert!(tab.is_script());
        assert!(tab.editor_focused);
        assert_eq!(tab.editor.buffer.text(), "");
        assert!(!sample_table_tab().is_script());
    }

    #[test]
    fn test_stream_events_fill_the_tab_grid_incrementally() {
        use crate::workspace::QueryStatus;
        use db_wire::{ColumnMeta, DataType, QueryResultPager, StreamEvent};
        use std::sync::Arc;

        let mut tab = sample_table_tab();
        let pager = Arc::new(
            QueryResultPager::new(vec![
                ColumnMeta::new("id", DataType::Int4),
                ColumnMeta::new("name", DataType::Text),
            ])
            .unwrap(),
        );

        // The grid shows the result as soon as the schema arrives
        assert_eq!(
            tab.apply_stream_event(StreamEvent::SchemaReady(Arc::clone(&pager))),
            QueryStatus::Running { rows: 0 }
        );
        assert!(Arc::ptr_eq(tab.grid.pager.as_ref().unwrap(), &pager));
        assert_eq!(tab.grid.table_name, "users");

        pager
            .append_row(vec![
                Some(&b"1"[..]),
                Some(&b"a fairly long user name streamed in"[..]),
            ])
            .unwrap();
        pager.append_row(vec![Some(&b"2"[..]), None]).unwrap();
        assert_eq!(
            tab.apply_stream_event(StreamEvent::PageAppended { total_rows: 2 }),
            QueryStatus::Running { rows: 2 }
        );
        assert_eq!(tab.grid.width_sampled_rows, 2);

        assert_eq!(
            tab.apply_stream_event(StreamEvent::Completed {
                total_rows: 2,
                elapsed_ms: 15
            }),
            QueryStatus::Done {
                rows: 2,
                elapsed_ms: 15
            }
        );
        assert_eq!(tab.execution_time_ms, Some(15));
    }

    #[test]
    fn test_stream_error_before_schema_keeps_previous_result() {
        use crate::workspace::QueryStatus;
        use db_wire::{ColumnMeta, DataType, QueryResultPager, StreamEvent};
        use std::sync::Arc;

        let mut tab = sample_table_tab();
        let previous =
            Arc::new(QueryResultPager::new(vec![ColumnMeta::new("id", DataType::Int4)]).unwrap());
        tab.grid.set_pager(Arc::clone(&previous));

        assert_eq!(
            tab.apply_stream_event(StreamEvent::Error("relation does not exist".to_string())),
            QueryStatus::Failed("relation does not exist".to_string())
        );
        assert!(Arc::ptr_eq(tab.grid.pager.as_ref().unwrap(), &previous));
    }

    #[test]
    fn test_fetch_next_appends_rows_to_the_current_result() {
        use crate::workspace::QueryStatus;
        use db_wire::{ColumnMeta, DataType, QueryResultPager, StreamEvent};
        use std::sync::Arc;

        let columns = || vec![ColumnMeta::new("id", DataType::Int4)];
        let mut tab = sample_table_tab();
        tab.fetch.size = 2;

        // First chunk comes back full, so more rows may follow
        let first = Arc::new(QueryResultPager::new(columns()).unwrap());
        tab.begin_fetch(true, false);
        tab.apply_stream_event(StreamEvent::SchemaReady(Arc::clone(&first)));
        first.append_row(vec![Some(&b"1"[..])]).unwrap();
        first.append_row(vec![Some(&b"2"[..])]).unwrap();
        tab.apply_stream_event(StreamEvent::Completed {
            total_rows: 2,
            elapsed_ms: 3,
        });
        assert!(tab.fetch.has_more);
        assert!(
            tab.fetch
                .last
                .as_ref()
                .unwrap()
                .message()
                .starts_with("2 row(s) fetched - ")
        );

        // The next chunk lands after the existing rows in the same pager
        let next = Arc::new(QueryResultPager::new(columns()).unwrap());
        tab.begin_fetch(true, true);
        assert_eq!(
            tab.apply_stream_event(StreamEvent::SchemaReady(Arc::clone(&next))),
            QueryStatus::Running { rows: 2 }
        );
        next.append_row(vec![Some(&b"3"[..])]).unwrap();
        assert_eq!(
            tab.apply_stream_event(StreamEvent::PageAppended { total_rows: 1 }),
            QueryStatus::Running { rows: 3 }
        );
        assert_eq!(
            tab.apply_stream_event(StreamEvent::Completed {
                total_rows: 1,
                elapsed_ms: 2
            }),
            QueryStatus::Done {
                rows: 3,
                elapsed_ms: 2
            }
        );

        assert!(Arc::ptr_eq(tab.grid.pager.as_ref().unwrap(), &first));
        assert_eq!(first.get_cell(2, 0), Some(b"3".to_vec()));
        assert!(
            !tab.fetch.has_more,
            "a short chunk means the table is exhausted"
        );
        assert!(!tab.fetch.is_appending());
        assert!(
            tab.fetch
                .last
                .as_ref()
                .unwrap()
                .message()
                .starts_with("3 row(s) fetched - ")
        );
    }

    #[test]
    fn test_fetch_size_text_falls_back_to_current_size() {
        let mut tab = sample_table_tab();
        tab.fetch.size_text = "500".to_string();
        tab.fetch.commit_size_text();
        assert_eq!(tab.fetch.size, 500);

        tab.fetch.size_text = "0".to_string();
        tab.fetch.commit_size_text();
        assert_eq!((tab.fetch.size, tab.fetch.size_text.as_str()), (500, "500"));
    }

    #[test]
    fn test_row_counts_group_thousands() {
        use crate::workspace::group_thousands;
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1400), "1,400");
        assert_eq!(group_thousands(1234567), "1,234,567");
    }
}

#[cfg(test)]
mod append_tests {
    use super::{QueryStatus, TabState};
    use db_wire::{ColumnMeta, DataType, QueryResultPager, StreamEvent};
    use std::sync::Arc;
    #[test]
    fn changed_schema_does_not_append_rows_to_an_existing_result() {
        let cache = Arc::new(sql_intel::SchemaCache::new().unwrap());
        let mut tab = TabState::table(1, "public", "t", ui_core::theme::Theme::dark(), cache);
        let old =
            Arc::new(QueryResultPager::new(vec![ColumnMeta::new("old", DataType::Text)]).unwrap());
        tab.grid.set_pager(old.clone());
        tab.begin_fetch(true, true);
        let new =
            Arc::new(QueryResultPager::new(vec![ColumnMeta::new("new", DataType::Text)]).unwrap());
        tab.apply_stream_event(StreamEvent::SchemaReady(new.clone()));
        new.append_row([Some(b"value".as_slice())]).unwrap();
        assert!(matches!(
            tab.apply_stream_event(StreamEvent::PageAppended { total_rows: 1 }),
            QueryStatus::Failed(_)
        ));
        assert_eq!(old.total_rows(), 0);
    }
}

#[cfg(test)]
mod save_revision_tests {
    use super::TabState;
    use std::sync::Arc;
    #[test]
    fn an_old_write_completion_does_not_mark_new_edits_saved() {
        let file = super::ScriptFile {
            path: "/tmp/revision-test.sql".into(),
            connection_id: "a".into(),
            name: "revision-test".into(),
            modified: std::time::SystemTime::now(),
            preview: String::new(),
        };
        let mut tab = TabState::script(
            1,
            &file,
            "",
            super::Theme::dark(),
            Arc::new(sql_intel::SchemaCache::new().unwrap()),
        );
        tab.editor.insert_text("old");
        let old_revision = tab.editor.buffer.revision();
        let old_text = tab.editor.buffer.text();
        tab.editor.insert_text("new");
        let latest = tab.editor.buffer.revision();
        let text = tab.editor.buffer.text();
        tab.mark_saved(old_revision, old_text.clone());
        assert!(tab.is_dirty());
        tab.mark_saved(latest, text);
        assert!(!tab.is_dirty());
        tab.mark_saved(old_revision, old_text);
        assert!(!tab.is_dirty());
        tab.editor.buffer.move_left();
        assert_eq!(tab.editor.buffer.revision(), latest);
        tab.editor.undo();
        assert!(tab.is_dirty());
    }
}

#[cfg(test)]
mod row_save_tests {
    #[test]
    fn failure_and_newer_edits_are_retained_until_a_matching_save_succeeds() {
        let mut tab = super::TabState::table(
            1,
            "s",
            "t",
            super::Theme::dark(),
            std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap()),
        );
        tab.grid
            .changeset
            .set_cell(0, 0, None, Some("before".into()));
        let saved = tab.grid.changeset.clone();
        assert!(!tab.finish_row_save(&saved, Err("conflict".into())));
        assert!(tab.grid.changeset.is_dirty());
        assert!(tab.last_error.as_deref().unwrap().contains("conflict"));
        tab.grid
            .changeset
            .set_cell(0, 0, None, Some("newer".into()));
        assert!(!tab.finish_row_save(&saved, Ok(1)));
        assert!(tab.grid.changeset.is_dirty());
        let latest = tab.grid.changeset.clone();
        assert!(tab.finish_row_save(&latest, Ok(1)));
        assert!(!tab.grid.changeset.is_dirty());
        assert!(tab.last_error.is_none());
    }
}

#[cfg(test)]
mod script_result_tests {
    use super::{ColumnMeta, DataType, QueryResultPager, SchemaCache, ScriptFile, TabState, Theme};
    use std::sync::Arc;
    fn tab() -> TabState {
        TabState::script(
            1,
            &ScriptFile {
                path: "/tmp/results.sql".into(),
                connection_id: "a".into(),
                name: "results".into(),
                modified: std::time::SystemTime::now(),
                preview: String::new(),
            },
            "",
            Theme::dark(),
            Arc::new(SchemaCache::new().unwrap()),
        )
    }
    #[test]
    fn sqlite_script_streams_individual_results_and_stops_after_an_error() {
        let driver = db_wire::create_driver(db_wire::ConnectionConfig {
            driver: db_wire::DbDriverKind::Sqlite,
            database: ":memory:".into(),
            ..Default::default()
        })
        .unwrap();
        let mut tab = tab();
        tab.pending_statements = sql_intel::split_script(
            "create table t (v text); insert into t values ('a;b'); select * from t; select missing from t; insert into t values ('must not run');"
        ).unwrap().into();
        while let Some(sql) = tab.pending_statements.pop_front() {
            tab.new_script_result(sql.clone());
            tab.begin_fetch(false, false);
            let (sender, receiver) = flume::unbounded();
            let _ = db_wire::get_runtime().block_on(driver.execute_streaming(&sql, sender));
            for event in receiver.drain() {
                tab.apply_stream_event(event);
            }
        }
        assert_eq!(tab.result_count(), 4);
        assert!(tab.last_error.is_some());
        assert!(!tab.fetch.has_more);
        tab.select_result(2);
        assert_eq!(tab.loaded_rows(), 1);
        assert!(tab.last_error.is_none());
        assert_eq!(
            tab.grid.pager.as_ref().unwrap().get_cell(0, 0).as_deref(),
            Some(b"a;b".as_slice())
        );
        assert_eq!(tab.connection_id.as_deref(), Some("a"));
    }

    #[test]
    fn result_switching_preserves_order_sql_errors_fetch_size_and_pagers() {
        let mut tab = tab();
        for n in 0..4 {
            tab.new_script_result(format!("select {n}"));
            tab.fetch.size = n + 10;
            tab.last_error = Some(n.to_string());
            let pager = Arc::new(
                QueryResultPager::new(vec![ColumnMeta::new(format!("col{n}"), DataType::Text)])
                    .unwrap(),
            );
            tab.grid.refill_pager(pager);
        }
        for index in [0, 3, 1, 2, 0, 2, 3] {
            tab.select_result(index);
            assert_eq!(
                tab.script_sql.as_deref(),
                Some(format!("select {index}").as_str())
            );
            assert_eq!(tab.fetch.size, index + 10);
            assert_eq!(tab.last_error.as_deref(), Some(index.to_string().as_str()));
            assert_eq!(
                tab.grid.pager.as_ref().unwrap().columns[0].name,
                format!("col{index}")
            );
        }
        tab.select_result(99);
        assert_eq!(tab.active_result, 3);
        for n in 0..30 {
            tab.new_script_result(format!("select {n}"));
        }
        assert_eq!(tab.result_count(), 20);
    }
}

#[cfg(test)]
mod query_plan_tests {
    use super::{QueryStatus, TabState};
    use db_wire::{ColumnMeta, DataType, QueryResultPager, StreamEvent};
    use std::sync::Arc;

    fn tab() -> TabState {
        TabState::blank(42)
    }

    fn complete_plan(tab: &mut TabState, json: &[u8]) -> QueryStatus {
        let pager = Arc::new(
            QueryResultPager::new(vec![ColumnMeta::new("QUERY PLAN", DataType::Json)]).unwrap(),
        );
        pager.append_row([Some(json.to_vec())]).unwrap();
        tab.apply_stream_event(StreamEvent::SchemaReady(pager));
        tab.apply_stream_event(StreamEvent::Completed {
            total_rows: 1,
            elapsed_ms: 2,
        })
    }

    #[test]
    fn completion_builds_plan_and_next_execution_clears_it() {
        let mut tab = tab();
        tab.begin_fetch(false, false);
        tab.plan_requested = true;
        let status = complete_plan(
            &mut tab,
            br#"[{"Plan":{"Node Type":"Result","Plan Rows":1}}]"#,
        );
        assert!(matches!(status, QueryStatus::Done { .. }));
        assert!(tab.query_plan.is_some());
        assert!(tab.show_plan);
        assert!(!tab.plan_requested);
        assert!(!tab.fetch.has_more);
        // The raw plan remains available without re-execution.
        assert_eq!(tab.grid.pager.as_ref().unwrap().total_rows(), 1);
        tab.begin_fetch(true, false);
        assert!(tab.query_plan.is_none());
        assert!(!tab.show_plan);
        assert!(!tab.plan_requested);
    }

    #[test]
    fn switching_results_restores_the_matching_plan() {
        let mut tab = tab();
        tab.new_script_result("EXPLAIN (FORMAT JSON) SELECT 1".into());
        tab.begin_fetch(false, false);
        tab.plan_requested = true;
        complete_plan(&mut tab, br#"[{"Plan":{"Node Type":"Result"}}]"#);
        tab.new_script_result("SELECT 2".into());
        tab.begin_fetch(false, false);
        assert!(tab.query_plan.is_none());
        assert!(!tab.show_plan);
        tab.select_result(0);
        assert!(tab.query_plan.is_some());
        assert!(tab.show_plan);
        tab.select_result(1);
        assert!(tab.query_plan.is_none());
        assert!(!tab.show_plan);
    }

    #[test]
    fn failed_and_malformed_plans_do_not_leave_pending_state() {
        let mut tab = tab();
        tab.begin_fetch(false, false);
        tab.plan_requested = true;
        assert!(matches!(
            complete_plan(&mut tab, b"{}"),
            QueryStatus::Failed(_)
        ));
        assert!(tab.query_plan.is_none());
        assert!(!tab.plan_requested);
        assert!(tab.last_error.is_some());
        tab.begin_fetch(false, false);
        tab.plan_requested = true;
        tab.apply_stream_event(StreamEvent::Error("cancelled".into()));
        assert!(!tab.plan_requested);
        assert!(!tab.show_plan);
    }
}
