use super::*;

pub struct TabState {
    /// Stable identity, so async query results land in the right tab after tabs move or close
    pub id: u64,
    pub title: String,
    pub schema_name: Option<String>,
    pub table_name: Option<String>,
    pub filter_sql: String,
    pub active_subtab: TableSubTab,
    pub active_property_section: PropertySection,
    pub properties_header_collapsed: bool,
    pub view_mode: DataViewMode,
    pub show_sql_dialog: bool,
    pub limit: usize,
    pub editor: SqlEditorView,
    pub grid: VirtualDataGrid,
    pub execution_time_ms: Option<u64>,
}

/// Outcome of applying one streaming event to a tab
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryStatus {
    Running { rows: u64 },
    Done { rows: u64, elapsed_ms: u64 },
    Failed(String),
}

impl TabState {
    /// Applies a streaming query event to this tab's grid
    pub fn apply_stream_event(&mut self, event: StreamEvent) -> QueryStatus {
        match event {
            StreamEvent::SchemaReady(pager) => {
                self.grid.set_pager(pager);
                if let Some(ref t_name) = self.table_name {
                    self.grid.set_table_name(t_name.clone());
                }
                self.execution_time_ms = None;
                QueryStatus::Running { rows: 0 }
            }
            StreamEvent::PageAppended { total_rows } => {
                self.grid.on_rows_appended();
                QueryStatus::Running { rows: total_rows }
            }
            StreamEvent::Completed { total_rows, elapsed_ms } => {
                self.grid.on_rows_appended();
                self.execution_time_ms = Some(elapsed_ms);
                QueryStatus::Done { rows: total_rows, elapsed_ms }
            }
            StreamEvent::Error(e) => QueryStatus::Failed(e),
        }
    }
}

impl Workspace {
    pub fn build_query_for_tab(&self, tab_idx: usize) -> String {
        let tab = match self.tabs.get(tab_idx) {
            Some(t) => t,
            None => return "SELECT 1;".to_string(),
        };

        let schema = tab.schema_name.as_deref().unwrap_or("public");
        let table = match tab.table_name.as_deref() {
            Some(t) if !t.is_empty() => t,
            _ => {
                if !tab.title.is_empty() && !tab.title.starts_with("Query ") {
                    &tab.title
                } else {
                    let full_text = tab.editor.buffer.text();
                    let non_comments: Vec<&str> = full_text
                        .lines()
                        .filter(|l| !l.trim().starts_with("--") && !l.trim().trim_matches(';').trim().is_empty())
                        .collect();
                    if !non_comments.is_empty() {
                        return non_comments.join("\n");
                    }
                    return "SELECT 1;".to_string();
                }
            }
        };

        let target = match self.selected_driver_kind {
            DbDriverKind::Postgres => format!("\"{}\".\"{}\"", schema, table),
            DbDriverKind::MySql => format!("`{}`.`{}`", schema, table),
            DbDriverKind::Sqlite => format!("\"{}\"", table),
        };

        let filter = tab.filter_sql.trim();
        let limit = tab.limit;

        if filter.is_empty() {
            format!("SELECT * FROM {} LIMIT {};", target, limit)
        } else {
            let f_upper = filter.to_uppercase();
            let has_limit = f_upper.contains("LIMIT");
            let limit_suffix = if has_limit {
                "".to_string()
            } else {
                format!(" LIMIT {}", limit)
            };

            if f_upper.starts_with("WHERE")
                || f_upper.starts_with("ORDER BY")
                || f_upper.starts_with("GROUP BY")
                || f_upper.starts_with("HAVING")
                || f_upper.starts_with("LIMIT")
            {
                format!("SELECT * FROM {} {}{};", target, filter, limit_suffix)
            } else {
                format!("SELECT * FROM {} WHERE {}{};", target, filter, limit_suffix)
            }
        }
    }

    pub fn query_table(&mut self, schema: &str, table: &str, cx: &mut Context<Self>) {
        self.selected_table_name = Some(table.to_string());

        let existing_tab_idx = self.tabs.iter().position(|t| {
            t.table_name.as_deref() == Some(table) && t.schema_name.as_deref() == Some(schema)
        });

        if let Some(idx) = existing_tab_idx {
            self.active_tab = idx;
        } else {
            let mut grid = VirtualDataGrid::new(None, self.theme.clone());
            grid.set_table_name(table);
            let id = self.allocate_tab_id();
            self.tabs.push(TabState {
                id,
                title: table.to_string(),
                schema_name: Some(schema.to_string()),
                table_name: Some(table.to_string()),
                filter_sql: String::new(),
                active_subtab: TableSubTab::Data,
                active_property_section: PropertySection::Columns,
                properties_header_collapsed: false,
                view_mode: DataViewMode::Grid,
                show_sql_dialog: false,
                limit: 200,
                editor: SqlEditorView::new("SELECT 1;", self.theme.clone(), Some(self.schema_cache.clone())),
                grid,
                execution_time_ms: None,
            });
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
        let Some(ref driver) = self.driver else {
            self.show_connection_modal = true;
            self.status_message = "Please connect to a database first.".to_string();
            cx.notify();
            return;
        };

        if self.tabs.is_empty() {
            return;
        }

        // The connection runs one statement at a time, and a cancel request targets whatever is
        // running when it arrives, so a new run waits for the current one instead of racing it
        if self.is_running_query {
            self.status_message = "⏳ A query is still running — cancel it or wait for it to finish.".to_string();
            cx.notify();
            return;
        }

        let sql = self.build_query_for_tab(self.active_tab);

        if sql.trim().trim_matches(';').trim().is_empty() {
            self.status_message = "No SQL statement to execute.".to_string();
            cx.notify();
            return;
        }

        self.query_generation += 1;
        let generation = self.query_generation;
        let tab_id = self.tabs[self.active_tab].id;
        self.is_running_query = true;
        self.cancel_requested = false;
        self.status_message = format!("Executing query: {}", sql.trim().replace('\n', " "));
        cx.notify();

        let (event_tx, event_rx) = flume::unbounded();
        let driver = Arc::clone(driver);
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
                    this.status_message = "❌ Query ended without a result.".to_string();
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
                self.status_message = "⏹ Query cancelled (tab closed).".to_string();
            }
            return !finished;
        };

        match tab.apply_stream_event(event) {
            QueryStatus::Running { rows } => {
                self.status_message = format!("⏳ Fetching rows… {} so far", rows);
                true
            }
            QueryStatus::Done { rows, elapsed_ms } => {
                self.is_running_query = false;
                self.status_message = format!("✅ Fetched {} rows in {}ms", rows, elapsed_ms);
                false
            }
            QueryStatus::Failed(e) => {
                self.is_running_query = false;
                self.status_message = if self.cancel_requested {
                    "⏹ Query cancelled.".to_string()
                } else {
                    format!("❌ SQL Error: {}", e)
                };
                false
            }
        }
    }

    pub fn cancel_running_query(&mut self, cx: &mut Context<Self>) {
        let Some(ref driver) = self.driver else {
            return;
        };
        if !self.is_running_query || self.cancel_requested {
            return;
        }

        self.cancel_requested = true;
        self.status_message = "⏹ Cancelling query…".to_string();
        cx.notify();

        let driver = Arc::clone(driver);
        let generation = self.query_generation;
        cx.spawn(async move |this, cx| {
            let result = db_wire::run_on_shared_runtime(async move { driver.cancel_active_query().await }).await;
            if let Err(e) = result {
                let _ = this.update(cx, |this, cx| {
                    if this.query_generation == generation && this.is_running_query {
                        this.cancel_requested = false;
                        this.status_message = format!("❌ Cancel failed: {}", e);
                        cx.notify();
                    }
                });
            }
        })
        .detach();
    }

    #[allow(dead_code)]
    pub fn new_tab(&mut self) {
        let count = self.tabs.len() + 1;
        let title = format!("Query {}", count);
        let editor = SqlEditorView::new(
            "SELECT 1;",
            self.theme.clone(),
            Some(self.schema_cache.clone()),
        );
        let grid = VirtualDataGrid::new(None, self.theme.clone());

        let id = self.allocate_tab_id();
        self.tabs.push(TabState {
            id,
            title,
            schema_name: None,
            table_name: None,
            filter_sql: String::new(),
            active_subtab: TableSubTab::Data,
            active_property_section: PropertySection::Columns,
            properties_header_collapsed: false,
            view_mode: DataViewMode::Grid,
            show_sql_dialog: false,
            limit: 200,
            editor,
            grid,
            execution_time_ms: None,
        });
        self.active_tab = self.tabs.len() - 1;
        self.is_filter_focused = false;
    }

    pub fn close_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.tabs.remove(index);
            if self.tabs.is_empty() {
                self.active_tab = 0;
                self.is_filter_focused = false;
            } else if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
                self.is_filter_focused = false;
            }
        }
    }
}
