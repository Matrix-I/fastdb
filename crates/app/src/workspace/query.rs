use super::tab_state::QueryStatus;
use super::*;

fn table_tab_index(
    tabs: &[TabState],
    connection_id: &str,
    schema: &str,
    table: &str,
) -> Option<usize> {
    tabs.iter().position(|tab| {
        tab.connection_id.as_deref() == Some(connection_id)
            && tab.schema_name.as_deref() == Some(schema)
            && tab.table_name.as_deref() == Some(table)
    })
}

impl Workspace {
    pub fn query_table(
        &mut self,
        connection_id: &str,
        schema: &str,
        table: &str,
        cx: &mut Context<Self>,
    ) {
        self.connect_saved_by_id(connection_id, cx);
        if self.connections.active_connection_id.as_deref() == Some(connection_id) {
            self.connections.navigator_mut().selected_table_name = Some(table.to_string());
        }

        if let Some(idx) = table_tab_index(&self.tabs, connection_id, schema, table) {
            self.active_tab = idx;
            self.tab_scroll.scroll_to_item(idx);
            self.input.is_filter_focused = false;
            cx.notify();
            return;
        }

        let id = self.allocate_tab_id();
        let mut tab = TabState::table(
            id,
            schema,
            table,
            self.theme.clone(),
            self.connections.session(connection_id).schema_cache.clone(),
        );
        tab.connection_id = Some(connection_id.to_string());
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;

        self.tab_scroll.scroll_to_item(self.active_tab);
        self.input.is_filter_focused = false;
        self.run_active_query(cx);
    }

    pub(super) fn allocate_tab_id(&mut self) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        id
    }

    pub fn run_active_query(&mut self, cx: &mut Context<Self>) {
        if self.operations.is_running() {
            return;
        }
        if let Some(tab) = self.tabs.get(self.active_tab).filter(|t| !t.file_busy) {
            self.save_then(vec![tab.id], AfterSave::Run(tab.id), cx);
        }
    }
    pub(super) fn run_saved_query(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.operations.is_running() {
            return;
        }
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        if self.tabs[index].file_busy {
            return;
        }
        if self.tabs.is_empty() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.fetch.commit_size_text();
            if tab.is_script() {
                let statement = tab.editor.active_statement();
                tab.pending_statements.clear();
                tab.script_status = None;
                if statement.trim().is_empty() {
                    return;
                }
                tab.new_script_result(statement);
            }
        }
        self.input.is_fetch_size_focused = false;
        let (sql, paged) = self.build_fetch_query(index, 0);
        self.start_query(index, sql, paged, false, cx);
    }

    pub(super) fn run_script(&mut self, cx: &mut Context<Self>) {
        if self.operations.is_running() {
            return;
        }
        if let Some(tab) = self
            .tabs
            .get(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy)
        {
            self.save_then(vec![tab.id], AfterSave::RunScript(tab.id), cx);
        }
    }

    pub(super) fn run_saved_script(&mut self, id: u64, cx: &mut Context<Self>) {
        if self.operations.is_running() {
            return;
        }
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let Some(driver) = self.tab_session(index).driver.clone() else {
            let owner = self.tabs[index].connection_id.clone();
            self.edit_saved_connection(owner.as_deref());
            cx.notify();
            return;
        };
        if driver.supports_script_session() {
            let parsed =
                sql_intel::split_script_with_transactions(&self.tabs[index].editor.buffer.text());
            match parsed {
                Ok(statements) if !statements.is_empty() => {
                    self.start_pinned_script(index, driver, statements, cx)
                }
                Ok(_) => {}
                Err(error) => {
                    self.tabs[index].last_error = Some(error);
                    cx.notify();
                }
            }
            return;
        }
        let tab = &mut self.tabs[index];
        tab.script_status = None;
        match sql_intel::split_script(&tab.editor.buffer.text()) {
            Ok(statements) => {
                tab.pending_statements = statements.into();
                self.start_next_statement(index, cx);
            }
            Err(error) => {
                tab.last_error = Some(error);
                cx.notify();
            }
        }
    }

    fn start_next_statement(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(sql) = self.tabs[index].pending_statements.pop_front() {
            self.tabs[index].new_script_result(sql.clone());
            // Execute each statement once, exactly as written. Stream all rows into the
            // paged store rather than OFFSET-replaying statements within a script.
            self.start_query(index, sql, false, false, cx);
        }
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
        if tab.is_script()
            && !append
            && let Some(connection) = tab.connection_id.as_ref()
        {
            self.query_history.record(
                connection.clone(),
                tab.script_sql.clone().unwrap_or(sql.clone()),
            );
        }
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
                        if !keep
                            && !this.operations.is_running()
                            && let Some(index) = this.tabs.iter().position(|t| t.id == tab_id)
                        {
                            this.start_next_statement(index, cx);
                        }
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
                        tab.pending_statements.clear();
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
        let cancelled = self
            .operations
            .active()
            .is_some_and(|op| op.cancel_requested);
        match tab.apply_stream_event(event) {
            QueryStatus::Running { .. } => true,
            QueryStatus::Failed(_) => {
                tab.pending_statements.clear();
                self.operations.finish(generation);
                false
            }
            QueryStatus::Done { .. } => {
                if cancelled {
                    tab.pending_statements.clear();
                }

                self.operations.finish(generation);
                false
            }
        }
    }

    pub fn cancel_running_query(&mut self, cx: &mut Context<Self>) {
        if let Some(op) = self.operations.active()
            && let Some(tab) = self.tabs.iter_mut().find(|t| t.id == op.tab_id)
        {
            tab.pending_statements.clear();
        }
        let Some((generation, driver)) = self.operations.request_cancel() else {
            return;
        };
        if let Some(control) = self
            .operations
            .active()
            .and_then(|op| op.script_control.clone())
        {
            control.request_cancel();
            cx.notify();
            return;
        }
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

#[cfg(test)]
mod table_tab_tests {
    use super::table_tab_index;
    use crate::workspace::TabState;
    use sql_intel::SchemaCache;
    use std::sync::Arc;
    use ui_core::theme::Theme;

    #[test]
    fn reopening_uses_the_tables_owner_even_with_identical_titles() {
        let cache = Arc::new(SchemaCache::new().unwrap());
        let tabs: Vec<_> = [("a", "public"), ("b", "public"), ("a", "archive")]
            .into_iter()
            .enumerate()
            .map(|(i, (connection, schema))| {
                let mut tab =
                    TabState::table(i as u64, schema, "users", Theme::dark(), cache.clone());
                tab.connection_id = Some(connection.into());
                tab
            })
            .collect();

        // Repeated opens target the same tab, regardless of which other tab is active.
        for _ in 0..3 {
            assert_eq!(table_tab_index(&tabs, "a", "public", "users"), Some(0));
            assert_eq!(table_tab_index(&tabs, "b", "public", "users"), Some(1));
            assert_eq!(table_tab_index(&tabs, "a", "archive", "users"), Some(2));
        }
        assert_eq!(table_tab_index(&tabs, "c", "public", "users"), None);
        assert_eq!(table_tab_index(&tabs, "a", "public", "orders"), None);
    }
}
