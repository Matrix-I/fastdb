//! Table tab session actions preserve the table grid instead of creating SQL result tabs.
use super::session_controls::SessionSetting;
use super::table_session::{self, Boundary};
use super::*;

impl Workspace {
    pub(super) fn apply_table_transaction(&mut self, boundary: Boundary, cx: &mut Context<Self>) {
        if self.execution_busy(cx) {
            return;
        }
        self.commit_cell_edit();
        let index = self.active_tab;
        let Some(tab) = self.tabs.get(index).filter(|tab| tab.table_name.is_some()) else {
            return;
        };
        let Some(driver) = self
            .tab_session(index)
            .driver
            .clone()
            .filter(|driver| driver.supports_script_session() && !driver.is_read_only())
        else {
            return;
        };
        if tab.schema_stale && boundary != Boundary::Rollback {
            self.tabs[index].last_error =
                Some("Schema changed; cancel staged edits and reload before saving".into());
            cx.notify();
            return;
        }
        let statements = if boundary == Boundary::Rollback {
            Vec::new()
        } else {
            match self.table_edit_statements(index) {
                Ok(statements) => statements,
                Err(error) => {
                    self.tabs[index].last_error = Some(error);
                    cx.notify();
                    return;
                }
            }
        };
        let tab = &self.tabs[index];
        let Some(connection_id) = tab.connection_id.clone() else {
            return;
        };
        let id = tab.id;
        let state = tab.transaction_state.clone();
        if boundary == Boundary::Commit && statements.is_empty() && state != "In transaction" {
            return;
        }
        if boundary == Boundary::Save && statements.is_empty() {
            return;
        }
        if boundary == Boundary::Rollback
            && matches!(state.as_str(), "Idle" | "Session closed")
            && !tab.grid.changeset.is_dirty()
        {
            return;
        }
        let owner = Arc::downgrade(&tab.tab_session);
        let provider = self.tab_provider(index);
        let saved = tab.grid.changeset.clone();
        let Some(run) =
            self.operations
                .begin(id, connection_id, driver.clone(), OperationKind::Save)
        else {
            return;
        };
        self.tabs[index].last_error = None;
        self.tabs[index].script_status = Some("Applying table transaction…".into());
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = db_wire::run_on_shared_runtime(async move {
                Ok::<_, anyhow::Error>(
                    table_session::apply(driver, owner, provider, state, statements, boundary)
                        .await,
                )
            })
            .await
            .unwrap_or_else(|error| {
                table_session::Outcome::failed("Session closed", format!("{error:#}"))
            });
            let _ = this.update(cx, |this, cx| {
                if !this.operations.finish(run) {
                    return;
                }
                let Some(index) = this.tabs.iter().position(|tab| tab.id == id) else {
                    return;
                };
                let tab = &mut this.tabs[index];
                tab.transaction_state = outcome.state;
                if let Some(error) = outcome.error {
                    tab.last_error = Some(error);
                    cx.notify();
                    return;
                }
                let cleared = if boundary == Boundary::Rollback {
                    tab.grid.clear_saved_edits();
                    tab.close_record_view();
                    true
                } else {
                    tab.finish_row_save(&saved, Ok(0))
                };
                tab.script_status = Some(format!("Table session · {}", tab.transaction_state));
                if cleared {
                    this.run_saved_query(id, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn apply_table_setting(
        &mut self,
        index: usize,
        driver: Arc<dyn DatabaseDriver>,
        setting: SessionSetting,
        statements: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let tab = &self.tabs[index];
        let Some(connection_id) = tab.connection_id.clone() else {
            return;
        };
        let id = tab.id;
        let owner = Arc::downgrade(&tab.tab_session);
        let Some(run) =
            self.operations
                .begin(id, connection_id, driver.clone(), OperationKind::Save)
        else {
            return;
        };
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = db_wire::run_on_shared_runtime(async move {
                let mut state = None;
                for sql in statements {
                    let result = table_session::statement(driver.clone(), owner.clone(), sql)
                        .await
                        .map_err(anyhow::Error::msg)?;
                    if result.active.is_some() {
                        state = result.active;
                    }
                }
                Ok::<_, anyhow::Error>(state)
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                if !this.operations.finish(run) {
                    return;
                }
                let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) else {
                    return;
                };
                match result {
                    Ok(active) => {
                        match setting {
                            SessionSetting::AutoCommit(enabled) => {
                                tab.manual_transactions = !enabled
                            }
                            SessionSetting::Isolation(level) => tab.session_isolation = Some(level),
                        }
                        if let Some(active) = active {
                            tab.transaction_state =
                                if active { "In transaction" } else { "Idle" }.into();
                        }
                        tab.last_error = None;
                    }
                    Err(error) => tab.last_error = Some(format!("{error:#}")),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Workspace {
    pub(super) fn start_table_query(
        &mut self,
        index: usize,
        driver: Arc<dyn DatabaseDriver>,
        sql: String,
        paged: bool,
        append: bool,
        cx: &mut Context<Self>,
    ) {
        use db_wire::script::{ScriptControl, ScriptEvent, ScriptOutcome};
        let tab = &mut self.tabs[index];
        if !append {
            tab.filter_timeline
                .record(super::filter_timeline::FilterSnapshot {
                    sql: tab.filter_sql.clone(),
                    order: tab.grid.ordering.clone(),
                    fetch_size: tab.fetch.size,
                    criteria: tab.filter_spec.clone(),
                });
        }
        tab.execution_log.start(&sql);
        tab.begin_fetch(paged, append);
        let id = tab.id;
        let Some(connection) = tab.connection_id.clone() else {
            return;
        };
        let owner = Arc::downgrade(&tab.tab_session);
        let timeout = tab.statement_timeout();
        let query_timeout = timeout
            .map(|t| t + std::time::Duration::from_secs(30))
            .unwrap_or(std::time::Duration::from_secs(60));
        let control = Arc::new(ScriptControl::with_timeout(timeout));
        let Some(run) =
            self.operations
                .begin_script(id, connection, driver.clone(), control.clone())
        else {
            return;
        };
        let (tx, rx) = flume::bounded(64);
        let run_control = control.clone();
        let feedback = tx.clone();
        db_wire::get_runtime().spawn(async move {
            let result = tokio::time::timeout(
                query_timeout,
                db_wire::script::run_tab_script(driver, vec![sql], run_control, tx, owner.clone()),
            )
            .await;
            if result.is_err() {
                if let Some(owner) = owner.upgrade() {
                    owner.close();
                }
                let _ = feedback.send_async(ScriptEvent::CleaningUp).await;
                let _ = feedback
                    .send_async(ScriptEvent::Finished(ScriptOutcome {
                        error: Some("Table query timed out; session closed".into()),
                        ..Default::default()
                    }))
                    .await;
            }
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut terminal = None;
            let mut stream_error = None;
            let mut finished = false;
            while let Ok(event) = rx.recv_async().await {
                let final_outcome = match event {
                    ScriptEvent::Stream(StreamEvent::Completed {
                        total_rows,
                        elapsed_ms,
                    }) => {
                        terminal = Some(StreamEvent::Completed {
                            total_rows,
                            elapsed_ms,
                        });
                        continue;
                    }
                    ScriptEvent::Stream(StreamEvent::Error(error)) => {
                        stream_error = Some(error);
                        continue;
                    }
                    ScriptEvent::Finished(outcome) => Some(outcome),
                    event => {
                        let result = this.update(cx, |this, cx| {
                            if !this.operations.matches(run, id) {
                                return;
                            }
                            let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) else {
                                control.request_cancel();
                                return;
                            };
                            match event {
                                ScriptEvent::Stream(event) => {
                                    if let super::tab_state::QueryStatus::Failed(error) =
                                        tab.apply_stream_event(event)
                                    {
                                        stream_error = Some(error);
                                        control.request_cancel();
                                    }
                                }
                                ScriptEvent::TransactionState { active } => {
                                    tab.transaction_state =
                                        if active { "In transaction" } else { "Idle" }.into()
                                }
                                ScriptEvent::TransactionFailed => {
                                    tab.transaction_state =
                                        super::transaction_mode::FAILED_TRANSACTION_STATE.into()
                                }
                                ScriptEvent::CleaningUp => {
                                    tab.transaction_state = "Session closed".into()
                                }
                                _ => {}
                            }
                            this.notify_result(cx);
                        });
                        if result.is_err() {
                            control.request_cancel();
                            return;
                        }
                        None
                    }
                };
                if let Some(outcome) = final_outcome {
                    let error = outcome
                        .error
                        .or(outcome.cleanup_error)
                        .or(stream_error.take());
                    let event = if outcome.cancelled {
                        StreamEvent::Error(
                            "Table query cancelled; transaction rolled back and session closed"
                                .into(),
                        )
                    } else if let Some(error) = error {
                        StreamEvent::Error(error)
                    } else {
                        terminal.take().unwrap_or_else(|| {
                            StreamEvent::Error("Table query ended without completion".into())
                        })
                    };
                    let _ = this.update(cx, |this, cx| {
                        if !this.operations.finish(run) {
                            return;
                        }
                        if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) {
                            if outcome.cancelled {
                                tab.transaction_state = "Session closed".into();
                            }
                            tab.apply_stream_event(event);
                        }
                        this.notify_result(cx);
                    });
                    finished = true;
                    break;
                }
            }
            if !finished {
                let _ = this.update(cx, |this, cx| {
                    if this.operations.finish(run) {
                        if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) {
                            tab.tab_session.close();
                            tab.transaction_state = "Session closed".into();
                            tab.last_error = Some("Table worker stopped; session closed".into());
                        }
                        this.notify_result(cx);
                    }
                });
            }
        })
        .detach();
    }
}
