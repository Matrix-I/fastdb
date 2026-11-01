use super::tab_state::QueryStatus;
use super::*;
impl Workspace {
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
                self.connections.active().schema_cache.clone(),
            ));
            self.tabs.last_mut().unwrap().connection_id =
                self.connections.active_connection_id.clone();
            self.active_tab = self.tabs.len() - 1;
        }

        self.input.is_filter_focused = false;
        self.run_active_query(cx);
    }

    pub(super) fn allocate_tab_id(&mut self) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        id
    }

    pub fn run_active_query(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active_tab) {
            self.save_then(vec![tab.id], AfterSave::Run(tab.id), cx);
        }
    }
    pub(super) fn run_saved_query(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        if self.tabs.is_empty() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.fetch.commit_size_text();
            if tab.is_script() {
                let statement = tab.editor.active_statement();
                tab.script_sql = (!statement.is_empty()).then_some(statement);
            }
        }
        self.input.is_fetch_size_focused = false;
        let (sql, paged) = self.build_fetch_query(index, 0);
        self.start_query(index, sql, paged, false, cx);
    }

    /// Reads the next chunk of the active tab's result and appends it to the grid.
    /// Does nothing unless the last chunk came back full and no query is running.
    pub fn fetch_next_rows(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };
        if self.operations.is_running()
            || self.tab_session(self.active_tab).driver.is_none()
            || !tab.fetch.has_more
            || tab.grid.pager.is_none()
        {
            return;
        }
        let offset = tab.loaded_rows();
        let (sql, paged) = self.build_fetch_query(self.active_tab, offset);
        self.start_query(self.active_tab, sql, paged, true, cx);
    }

    pub(super) fn start_query(
        &mut self,
        index: usize,
        sql: String,
        paged: bool,
        append: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(driver) = self.tab_session(index).driver.clone() else {
            let owner = self.tabs.get(index).and_then(|t| t.connection_id.clone());
            self.edit_saved_connection(owner.as_deref());
            cx.notify();
            return;
        };

        // The connection runs one statement at a time, and a cancel request targets whatever is
        // running when it arrives, so a new run waits for the current one instead of racing it
        if self.operations.is_running() {
            cx.notify();
            return;
        }

        if sql.trim().trim_matches(';').trim().is_empty() {
            cx.notify();
            return;
        }

        let tab = &mut self.tabs[index];
        tab.begin_fetch(paged, append);
        let tab_id = tab.id;
        let connection_id = tab
            .connection_id
            .clone()
            .expect("query tab owns a connection");
        let Some(generation) =
            self.operations
                .begin(tab_id, connection_id, driver.clone(), OperationKind::Query)
        else {
            return;
        };
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
                        this.notify_result(cx);
                        keep
                    })
                    .unwrap_or(false);
                if !keep_listening {
                    return;
                }
            }

            // The driver hung up without a final event
            let _ = this.update(cx, |this, cx| {
                if this.operations.finish(generation) {
                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == tab_id) {
                        tab.apply_stream_event(StreamEvent::Error(
                            "Query stream closed before completion".into(),
                        ));
                    }
                    this.notify_result(cx);
                }
            });
        })
        .detach();
    }

    /// Routes a streaming event to its tab and updates the status bar.
    /// Returns false once the run is finished or no longer relevant.
    fn apply_query_event(&mut self, generation: u64, tab_id: u64, event: StreamEvent) -> bool {
        if !self.operations.matches(generation, tab_id) {
            return false;
        }

        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) else {
            // The tab was closed while its query was running: drop events until the run ends
            let finished = matches!(event, StreamEvent::Completed { .. } | StreamEvent::Error(_));
            if finished {
                self.operations.finish(generation);
            }
            return !finished;
        };

        if self
            .operations
            .active()
            .is_none_or(|op| tab.connection_id.as_deref() != Some(op.connection_id.as_str()))
        {
            return false;
        }
        match tab.apply_stream_event(event) {
            QueryStatus::Running { .. } => true,
            QueryStatus::Done { .. } | QueryStatus::Failed(_) => {
                self.operations.finish(generation);
                false
            }
        }
    }

    pub fn cancel_running_query(&mut self, cx: &mut Context<Self>) {
        let Some((generation, driver)) = self.operations.request_cancel() else {
            return;
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result =
                db_wire::run_on_shared_runtime(async move { driver.cancel_active_query().await })
                    .await;
            if result.is_err() {
                let _ = this.update(cx, |this, cx| {
                    this.operations.cancel_failed(generation);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}
