use super::*;

impl Workspace {
    pub(super) fn editable_query_result(&self, index: usize) -> bool {
        let Some(tab) = self.tabs.get(index) else {
            return false;
        };
        tab.is_script()
            && !tab.schema_stale
            && tab
                .grid
                .pager
                .as_ref()
                .is_some_and(|pager| pager.query_edit_origin.is_some())
            && self
                .tab_session(index)
                .driver
                .as_ref()
                .is_some_and(|driver| {
                    tab.query_edit_driver
                        .as_ref()
                        .and_then(std::sync::Weak::upgrade)
                        .is_some_and(|source| Arc::ptr_eq(driver, &source))
                })
    }

    pub(super) fn load_editable_result(&mut self, tab_id: u64, cx: &mut Context<Self>) {
        if self.execution_busy(cx) {
            return;
        }
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id && tab.is_script() && !tab.file_busy)
        else {
            return;
        };
        if self.tabs[index].grid.changeset.is_dirty()
            || self.tabs[index].grid.editing_cell.is_some()
        {
            self.tabs[index].last_error =
                Some("Save or Cancel result edits before loading another result".into());
            cx.notify();
            return;
        }
        let Some(driver) = self.tab_session(index).driver.clone() else {
            return;
        };
        if driver.is_read_only() {
            self.tabs[index].last_error = Some("This connection is read-only".into());
            cx.notify();
            return;
        }
        let Some(connection_id) = self.tabs[index].connection_id.clone() else {
            return;
        };
        let sql = self.tabs[index].editor.active_statement();
        let Some(run) =
            self.operations
                .begin(tab_id, connection_id, driver.clone(), OperationKind::Save)
        else {
            return;
        };
        self.tabs[index].script_status =
            Some("Loading editable result on an isolated connection…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let query = sql.clone();
            let source = driver.clone();
            let result = db_wire::run_on_shared_runtime(async move { source.load_editable_query(&query).await }).await;
            let _ = this.update(cx, |this, cx| {
                if !this.operations.finish(run) { return; }
                let Some(index) = this.tabs.iter().position(|tab| tab.id == tab_id) else { return; };
                if !this.tab_session(index).driver.as_ref().is_some_and(|current| Arc::ptr_eq(current, &driver)) { return; }
                let tab = &mut this.tabs[index];
                match result {
                    Ok(pager) => {
                        tab.new_script_result(sql);
                        let rows = pager.total_rows();
                        tab.begin_fetch(false, false);
                        tab.apply_stream_event(StreamEvent::SchemaReady(pager));
                        tab.apply_stream_event(StreamEvent::Completed { total_rows: rows, elapsed_ms: 0 });
                        tab.query_edit_driver = Some(Arc::downgrade(&driver));
                        tab.script_status = Some("Editable result · scalar non-key fields · Save commits through an isolated connection".into());
                        tab.editor_focused = false;
                    }
                    Err(error) => {
                        tab.last_error = Some(format!("{error:#}"));
                        tab.script_status = Some("Editable result could not be loaded".into());
                    }
                }
                this.notify_result(cx);
            });
        }).detach();
    }

    pub(super) fn save_query_result_edits(&mut self, cx: &mut Context<Self>) {
        let index = self.active_tab;
        if self.operations.is_running() {
            return;
        }
        if !self.editable_query_result(index) {
            if let Some(tab) = self.tabs.get_mut(index) {
                tab.last_error = Some("Query result source is unavailable. Cancel staged edits and reload the editable result.".into());
                cx.notify();
            }
            return;
        }
        let tab = &self.tabs[index];
        let changes = &tab.grid.changeset;
        if changes.changes.is_empty() {
            return;
        }
        if !changes.inserted_rows.is_empty() || !changes.deleted_rows.is_empty() {
            self.tabs[index].last_error =
                Some("Editable query results support cell updates only".into());
            cx.notify();
            return;
        }
        let pager = tab.grid.pager.clone().unwrap();
        let edits = changes
            .changes
            .iter()
            .map(|(key, value)| db_wire::QueryCellUpdate {
                row: key.row_idx,
                column: key.col_idx,
                value: value.new_value.clone(),
            })
            .collect();
        let saved = changes.clone();
        let query = tab.script_sql.clone().unwrap_or_default();
        let driver = self.tab_session(index).driver.clone().unwrap();
        let tab_id = tab.id;
        let Some(connection_id) = tab.connection_id.clone() else {
            return;
        };
        let Some(run) =
            self.operations
                .begin(tab_id, connection_id, driver.clone(), OperationKind::Save)
        else {
            return;
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let source = driver.clone();
            let result = db_wire::run_on_shared_runtime(async move {
                source.save_query_edits(pager, edits).await?;
                Ok(source.load_editable_query(&query).await.map_err(|error| format!("{error:#}")))
            }).await;
            let _ = this.update(cx, |this, cx| {
                if !this.operations.finish(run) { return; }
                let Some(index) = this.tabs.iter().position(|tab| tab.id == tab_id) else { return; };
                if !this.tab_session(index).driver.as_ref().is_some_and(|current| Arc::ptr_eq(current, &driver)) { return; }
                let tab = &mut this.tabs[index];
                match result {
                    Ok(refreshed) => {
                        tab.finish_row_save(&saved, Ok(saved.changes.len() as u64));
                        tab.query_edit_driver = None;
                        match refreshed {
                            Ok(pager) => {
                                let rows = pager.total_rows();
                                tab.begin_fetch(false, false);
                                tab.apply_stream_event(StreamEvent::SchemaReady(pager));
                                tab.apply_stream_event(StreamEvent::Completed { total_rows: rows, elapsed_ms: 0 });
                                tab.query_edit_driver = Some(Arc::downgrade(&driver));
                                tab.script_status = Some("Result edits committed and stored values reloaded".into());
                            }
                            Err(error) => {
                                tab.last_error = Some(format!("Edits committed, but reloading failed: {error}. Reload editable result before editing again."));
                            }
                        }
                    }
                    Err(error) => {
                        tab.query_edit_driver = None;
                        tab.last_error = Some(format!("{error:#}. Cancel staged edits and reload before retrying."));
                    }
                }
                this.notify_result(cx);
            });
        }).detach();
    }
}
