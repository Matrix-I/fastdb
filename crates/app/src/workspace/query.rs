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
    pub view_mode: DataViewMode,
    pub show_sql_dialog: bool,
    pub editor: SqlEditorView,
    pub grid: VirtualDataGrid,
    pub execution_time_ms: Option<u64>,
    pub fetch: FetchState,
    /// Statement of a script tab's current result, read again by "fetch next"
    pub script_sql: Option<String>,
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

/// Column name quoted for the driver's SQL dialect
pub(super) fn quote_ident(kind: DbDriverKind, name: &str) -> String {
    match kind {
        DbDriverKind::MySql | DbDriverKind::MariaDb => format!("`{}`", name.replace('`', "``")),
        DbDriverKind::Postgres
        | DbDriverKind::Sqlite
        | DbDriverKind::SqlServer
        | DbDriverKind::Oracle
        | DbDriverKind::Db2 => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// Vendor-specific pagination for table browsing. Free-form ODBC SQL runs unchanged.
fn table_page_suffix(kind: DbDriverKind, size: usize, offset: u64, ordered: bool) -> String {
    match kind {
        DbDriverKind::SqlServer => format!(
            "{} OFFSET {offset} ROWS FETCH NEXT {size} ROWS ONLY",
            if ordered {
                ""
            } else {
                " ORDER BY (SELECT NULL)"
            }
        ),
        DbDriverKind::Oracle | DbDriverKind::Db2 => {
            format!(" OFFSET {offset} ROWS FETCH NEXT {size} ROWS ONLY")
        }
        _ if offset > 0 => format!(" LIMIT {size} OFFSET {offset}"),
        _ => format!(" LIMIT {size}"),
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
    pub fn is_dirty(&self) -> bool {
        self.script_path.is_some() && self.editor.buffer.text() != self.saved_text
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
            view_mode: DataViewMode::Grid,
            show_sql_dialog: false,
            editor: SqlEditorView::new("", Theme::dark(), None),
            grid: VirtualDataGrid::new(None, Theme::dark()),
            execution_time_ms: None,
            fetch: FetchState::default(),
            script_sql: None,
            last_error: None,
            editor_focused: false,
            script_path: None,
            connection_id: None,
            saved_text: String::new(),
        }
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
                QueryStatus::Done { rows, elapsed_ms }
            }
            StreamEvent::Error(e) => self.fail_fetch(e),
        }
    }

    fn fail_fetch(&mut self, error: String) -> QueryStatus {
        // Rows copied before the failure stay; the next scroll to the end retries from there
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
        if source.num_cols() != target.num_cols() {
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

impl Workspace {
    /// SQL of the first fetch for a tab, as shown in the "SQL" dialog
    pub fn build_query_for_tab(&self, tab_idx: usize) -> String {
        self.build_fetch_query(tab_idx, 0).0
    }

    /// SQL reading one chunk of `fetch.size` rows starting at `offset`, and whether it is
    /// limited by the fetch size (false for raw SQL or a filter with its own LIMIT)
    fn build_fetch_query(&self, tab_idx: usize, offset: u64) -> (String, bool) {
        let tab = match self.tabs.get(tab_idx) {
            Some(t) => t,
            None => return ("SELECT 1;".to_string(), false),
        };

        let schema = tab.schema_name.as_deref().unwrap_or("public");
        let table = match tab.table_name.as_deref() {
            Some(t) if !t.is_empty() => t,
            _ => {
                return if self.tab_provider(tab_idx).uses_odbc() {
                    // Execute vendor SQL as written: CTEs, TOP and PL/SQL are not LIMIT dialects.
                    (tab.script_sql.clone().unwrap_or_default(), false)
                } else {
                    script_fetch_query(tab, offset)
                };
            }
        };

        let target = match self.tab_provider(tab_idx) {
            DbDriverKind::Postgres
            | DbDriverKind::SqlServer
            | DbDriverKind::Oracle
            | DbDriverKind::Db2 => format!(
                "{}.{}",
                quote_ident(self.tab_provider(tab_idx), schema),
                quote_ident(self.tab_provider(tab_idx), table)
            ),
            DbDriverKind::MySql | DbDriverKind::MariaDb => format!("`{}`.`{}`", schema, table),
            DbDriverKind::Sqlite => format!("\"{}\"", table),
        };

        let filter = tab.filter_sql.trim();
        let f_upper = filter.to_uppercase();
        // A filter that orders or limits its rows itself already ends the statement,
        // so the header ordering cannot be appended after it
        let order_suffix = match &tab.grid.ordering {
            Some(o)
                if !f_upper.contains("ORDER BY")
                    && !["LIMIT", "OFFSET", "FETCH"]
                        .iter()
                        .any(|w| f_upper.split_whitespace().any(|p| p == *w)) =>
            {
                format!(
                    " ORDER BY {} {}",
                    quote_ident(self.tab_provider(tab_idx), &o.column),
                    if o.descending { "DESC" } else { "ASC" }
                )
            }
            _ => String::new(),
        };

        let limit_suffix = table_page_suffix(
            self.tab_provider(tab_idx),
            tab.fetch.size,
            offset,
            !order_suffix.is_empty() || f_upper.contains("ORDER BY"),
        );

        if filter.is_empty() {
            (
                format!("SELECT * FROM {}{}{};", target, order_suffix, limit_suffix),
                true,
            )
        } else {
            // A filter with its own LIMIT decides the row count, so it is not fetched in chunks
            let paged = !["LIMIT", "OFFSET", "FETCH"]
                .iter()
                .any(|word| f_upper.split_whitespace().any(|part| part == *word));
            let limit_suffix = if paged { limit_suffix.as_str() } else { "" };

            let sql = if f_upper.starts_with("WHERE")
                || f_upper.starts_with("ORDER BY")
                || f_upper.starts_with("GROUP BY")
                || f_upper.starts_with("HAVING")
                || f_upper.starts_with("LIMIT")
                || f_upper.starts_with("OFFSET")
                || f_upper.starts_with("FETCH")
            {
                format!(
                    "SELECT * FROM {} {}{}{};",
                    target, filter, order_suffix, limit_suffix
                )
            } else {
                format!(
                    "SELECT * FROM {} WHERE {}{}{};",
                    target, filter, order_suffix, limit_suffix
                )
            };
            (sql, paged)
        }
    }

    pub fn query_table(&mut self, schema: &str, table: &str, cx: &mut Context<Self>) {
        self.connections.navigator_mut().selected_table_name = Some(table.to_string());

        let existing_tab_idx = self.tabs.iter().position(|t| {
            t.connection_id == self.connections.active_connection_id
                && t.table_name.as_deref() == Some(table)
                && t.schema_name.as_deref() == Some(schema)
        });

        if let Some(idx) = existing_tab_idx {
            self.active_tab = idx;
        } else {
            let id = self.allocate_tab_id();
            self.tabs.push(TabState::table(
                id,
                schema,
                table,
                self.theme.clone(),
                self.tab_session(self.active_tab).schema_cache.clone(),
            ));
            self.tabs.last_mut().unwrap().connection_id =
                self.connections.active_connection_id.clone();
            self.active_tab = self.tabs.len() - 1;
        }

        self.is_filter_focused = false;
        self.run_active_query(cx);
    }

    fn allocate_tab_id(&mut self) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        id
    }

    pub fn run_active_query(&mut self, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        self.save_script_tab(self.active_tab);
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.fetch.commit_size_text();
            if tab.is_script() {
                let statement = tab.editor.active_statement();
                tab.script_sql = (!statement.is_empty()).then_some(statement);
            }
        }
        self.is_fetch_size_focused = false;
        let (sql, paged) = self.build_fetch_query(self.active_tab, 0);
        self.start_query(sql, paged, false, cx);
    }

    /// Reads the next chunk of the active tab's result and appends it to the grid.
    /// Does nothing unless the last chunk came back full and no query is running.
    pub fn fetch_next_rows(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };
        if self.is_running_query
            || self.tab_session(self.active_tab).driver.is_none()
            || !tab.fetch.has_more
            || tab.grid.pager.is_none()
        {
            return;
        }
        let offset = tab.loaded_rows();
        let (sql, paged) = self.build_fetch_query(self.active_tab, offset);
        self.start_query(sql, paged, true, cx);
    }

    fn start_query(&mut self, sql: String, paged: bool, append: bool, cx: &mut Context<Self>) {
        let Some(driver) = self.tab_session(self.active_tab).driver.clone() else {
            self.form.show_connection_modal = true;
            cx.notify();
            return;
        };

        // The connection runs one statement at a time, and a cancel request targets whatever is
        // running when it arrives, so a new run waits for the current one instead of racing it
        if self.is_running_query {
            cx.notify();
            return;
        }

        if sql.trim().trim_matches(';').trim().is_empty() {
            cx.notify();
            return;
        }

        self.query_generation += 1;
        let generation = self.query_generation;
        let tab = &mut self.tabs[self.active_tab];
        tab.begin_fetch(paged, append);
        let tab_id = tab.id;
        self.is_running_query = true;
        self.cancel_requested = false;
        cx.notify();

        let (event_tx, event_rx) = flume::unbounded();
        let driver = Arc::clone(&driver);
        db_wire::get_runtime().spawn(async move {
            // Failures are reported through StreamEvent::Error
            let _ = driver.execute_streaming(&sql, event_tx).await;
        });

        cx.spawn(async move |this, cx| {
            while let Ok(event) = event_rx.recv_async().await {
                let keep_listening = this
                    .update(cx, |this, cx| {
                        let keep = this.apply_query_event(generation, tab_id, event);
                        // Nobody will see the rest of an orphaned run, so stop it on the server
                        if keep && !this.tabs.iter().any(|t| t.id == tab_id) {
                            this.cancel_running_query(cx);
                        }
                        cx.notify();
                        keep
                    })
                    .unwrap_or(false);
                if !keep_listening {
                    return;
                }
            }

            // The driver hung up without a final event
            let _ = this.update(cx, |this, cx| {
                if this.query_generation == generation && this.is_running_query {
                    this.is_running_query = false;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Routes a streaming event to its tab and updates the status bar.
    /// Returns false once the run is finished or no longer relevant.
    fn apply_query_event(&mut self, generation: u64, tab_id: u64, event: StreamEvent) -> bool {
        if self.query_generation != generation {
            return false;
        }

        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            // The tab was closed while its query was running: drop events until the run ends
            let finished = matches!(event, StreamEvent::Completed { .. } | StreamEvent::Error(_));
            if finished {
                self.is_running_query = false;
            }
            return !finished;
        };

        match tab.apply_stream_event(event) {
            QueryStatus::Running { .. } => true,
            QueryStatus::Done { .. } | QueryStatus::Failed(_) => {
                self.is_running_query = false;
                false
            }
        }
    }

    pub fn cancel_running_query(&mut self, cx: &mut Context<Self>) {
        let Some(driver) = self.tab_session(self.active_tab).driver.clone() else {
            return;
        };
        if !self.is_running_query || self.cancel_requested {
            return;
        }

        self.cancel_requested = true;
        cx.notify();

        let driver = Arc::clone(&driver);
        let generation = self.query_generation;
        cx.spawn(async move |this, cx| {
            let result =
                db_wire::run_on_shared_runtime(async move { driver.cancel_active_query().await })
                    .await;
            if result.is_err() {
                let _ = this.update(cx, |this, cx| {
                    if this.query_generation == generation && this.is_running_query {
                        this.cancel_requested = false;
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    /// "+ SQL", Cmd+] and the navigator's "SQL Editor": opens the latest script of
    /// `conn_id` (the active connection when `None`), creating one when it has none, or
    /// offers the script chooser when the latest one is already open
    pub fn open_sql_script(&mut self, conn_id: Option<&str>, cx: &mut Context<Self>) {
        let Some(conn_id) = conn_id
            .map(str::to_string)
            .or_else(|| self.connections.active_connection_id.clone())
        else {
            self.form.show_connection_modal = true;
            return;
        };
        let script = match self.script_store.most_recent(&conn_id) {
            Some(recent)
                if self
                    .tabs
                    .iter()
                    .any(|t| t.script_path.as_deref() == Some(recent.path.as_path())) =>
            {
                self.open_script_chooser(&conn_id);
                return;
            }
            Some(recent) => recent,
            None => match self.script_store.create(&conn_id) {
                Ok(script) => script,
                Err(e) => {
                    tracing::warn!("{e:#}");
                    return;
                }
            },
        };
        self.open_script_file(&script, cx);
    }

    /// Activates the tab showing `script`, or reads it into a new tab, on its connection
    pub fn open_script_file(&mut self, script: &ScriptFile, cx: &mut Context<Self>) {
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.script_path.as_deref() == Some(script.path.as_path()))
        {
            self.active_tab = idx;
        } else {
            let text = match self.script_store.read(script) {
                Ok(text) => text,
                Err(e) => {
                    tracing::warn!("{e:#}");
                    return;
                }
            };
            let id = self.allocate_tab_id();
            self.tabs.push(TabState::script(
                id,
                script,
                &text,
                self.theme.clone(),
                self.tab_session(self.active_tab).schema_cache.clone(),
            ));
            self.active_tab = self.tabs.len() - 1;
        }
        self.is_filter_focused = false;
        self.blur_fetch_size();
        self.activate_script_connection(cx);
    }

    /// Makes the active script tab's connection the active connection, so it runs there
    pub fn activate_script_connection(&mut self, cx: &mut Context<Self>) {
        let Some(conn_id) = self
            .tabs
            .get(self.active_tab)
            .and_then(|t| t.connection_id.clone())
        else {
            return;
        };
        if self.connections.active_connection_id.as_deref() != Some(conn_id.as_str())
            || !self.connections.active().is_connected
        {
            self.connect_saved_by_id(&conn_id, cx);
        }
    }

    /// Writes a script tab's text to its file when it changed; a failure shows in its banner
    pub fn save_script_tab(&mut self, idx: usize) {
        let Some(tab) = self.tabs.get_mut(idx).filter(|t| t.is_dirty()) else {
            return;
        };
        let Some(path) = tab.script_path.clone() else {
            return;
        };
        let text = tab.editor.buffer.text();
        match self.script_store.write(&path, &text) {
            Ok(()) => tab.saved_text = text,
            Err(e) => tab.last_error = Some(format!("{e:#}")),
        }
    }

    /// Saves every script tab with unsaved changes
    pub fn save_dirty_scripts(&mut self) {
        for idx in 0..self.tabs.len() {
            self.save_script_tab(idx);
        }
    }

    pub fn close_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.close_tabs_where(index, |i| i != index);
        }
    }
}

#[cfg(test)]
mod provider_tests {
    use super::{quote_ident, table_page_suffix};
    use db_wire::DbDriverKind;

    #[test]
    fn enterprise_table_paging_uses_vendor_syntax() {
        assert_eq!(
            table_page_suffix(DbDriverKind::SqlServer, 50, 100, false),
            " ORDER BY (SELECT NULL) OFFSET 100 ROWS FETCH NEXT 50 ROWS ONLY"
        );
        assert_eq!(
            table_page_suffix(DbDriverKind::SqlServer, 50, 0, true),
            " OFFSET 0 ROWS FETCH NEXT 50 ROWS ONLY"
        );
        for kind in [DbDriverKind::Oracle, DbDriverKind::Db2] {
            assert_eq!(
                table_page_suffix(kind, 50, 100, false),
                " OFFSET 100 ROWS FETCH NEXT 50 ROWS ONLY"
            );
            assert_eq!(quote_ident(kind, "a\"b"), "\"a\"\"b\"");
        }
    }

    #[test]
    fn mariadb_uses_mysql_identifier_quoting() {
        assert_eq!(
            quote_ident(DbDriverKind::MariaDb, "user`name"),
            "`user``name`"
        );
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
