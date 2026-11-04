use super::*;
use db_wire::script::{ScriptControl, ScriptEvent, ScriptOutcome};

enum ExecutionKind {
    Script,
    Plan,
    Settings,
    Schema(String),
}

impl Workspace {
    pub(super) fn start_pinned_script(
        &mut self,
        index: usize,
        driver: Arc<dyn DatabaseDriver>,
        statements: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(statements) = self.plan_user_transactions(index, statements, cx) else {
            return;
        };
        let timeout = self.tabs[index].statement_timeout();
        self.start_pinned_execution(
            index,
            driver,
            statements,
            ExecutionKind::Script,
            timeout,
            cx,
        );
    }

    pub(super) fn start_pinned_session_settings(
        &mut self,
        index: usize,
        driver: Arc<dyn DatabaseDriver>,
        statements: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        const SETTINGS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
        self.start_pinned_execution(
            index,
            driver,
            statements,
            ExecutionKind::Settings,
            Some(SETTINGS_TIMEOUT),
            cx,
        );
    }

    pub(super) fn start_pinned_schema_change(
        &mut self,
        index: usize,
        driver: Arc<dyn DatabaseDriver>,
        statements: Vec<String>,
        schema: String,
        cx: &mut Context<Self>,
    ) {
        self.start_pinned_execution(
            index,
            driver,
            statements,
            ExecutionKind::Schema(schema),
            Some(std::time::Duration::from_secs(15)),
            cx,
        );
    }

    pub(super) fn start_pinned_plan(
        &mut self,
        index: usize,
        driver: Arc<dyn DatabaseDriver>,
        sql: String,
        cx: &mut Context<Self>,
    ) {
        let Some(statements) = self.plan_user_transactions(index, vec![sql], cx) else {
            return;
        };
        let timeout = self.tabs[index].statement_timeout();
        self.start_pinned_execution(index, driver, statements, ExecutionKind::Plan, timeout, cx);
    }

    fn plan_user_transactions(
        &mut self,
        index: usize,
        statements: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Option<Vec<String>> {
        match super::transaction_mode::manual_statements(
            self.tab_provider(index),
            self.tabs[index].manual_transactions,
            &self.tabs[index].transaction_state,
            statements,
        ) {
            Ok(statements) => Some(statements),
            Err(error) => {
                self.tabs[index].last_error = Some(error);
                cx.notify();
                None
            }
        }
    }

    fn start_pinned_execution(
        &mut self,
        index: usize,
        driver: Arc<dyn DatabaseDriver>,
        statements: Vec<String>,
        kind: ExecutionKind,
        timeout: Option<std::time::Duration>,
        cx: &mut Context<Self>,
    ) {
        let control = Arc::new(super::transaction_mode::script_control(
            self.tab_provider(index),
            !matches!(kind, ExecutionKind::Settings | ExecutionKind::Schema(_))
                && self.tabs[index].manual_transactions,
            timeout,
        ));
        let tab = &mut self.tabs[index];
        let tab_id = tab.id;
        let Some(connection_id) = tab.connection_id.clone() else {
            return;
        };
        let Some(run) =
            self.operations
                .begin_script(tab_id, connection_id, driver.clone(), control.clone())
        else {
            return;
        };
        if tab.transaction_state == "Session closed" {
            tab.transaction_state = "Idle".into();
        }
        tab.pending_statements.clear();
        tab.last_error = None;
        tab.export.message = None;
        tab.script_status = Some("Opening tab session…".into());
        let (tx, rx) = flume::unbounded();
        db_wire::get_runtime().spawn(db_wire::script::run_tab_script(
            driver,
            statements,
            control.clone(),
            tx,
            Arc::downgrade(&tab.tab_session),
        ));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut finished = false;
            while let Ok(event) = rx.recv_async().await {
                finished = matches!(event, ScriptEvent::Finished(_));
                let applied = this.update(cx, |this, cx| {
                    this.apply_pinned_script_event(run, tab_id, event, &control, &kind);
                    this.notify_result(cx);
                });
                if applied.is_err() {
                    control.request_cancel();
                    return;
                }
                if finished {
                    return;
                }
            }
            if !finished {
                let _ = this.update(cx, |this, cx| {
                    this.apply_pinned_script_event(
                        run,
                        tab_id,
                        ScriptEvent::Finished(ScriptOutcome {
                            error: Some("Script worker stopped before completion".into()),
                            ..Default::default()
                        }),
                        &control,
                        &kind,
                    );
                    this.notify_result(cx);
                });
            }
        })
        .detach();
    }

    pub(super) fn transaction_command(&mut self, sql: &str, cx: &mut Context<Self>) {
        if self.execution_busy(cx) {
            return;
        }
        let index = self.active_tab;
        if let Some(driver) = self.tab_session(index).driver.clone()
            && driver.supports_script_session()
        {
            self.start_pinned_script(index, driver, vec![sql.into()], cx);
        }
    }

    fn apply_pinned_script_event(
        &mut self,
        run: u64,
        tab_id: u64,
        event: ScriptEvent,
        control: &ScriptControl,
        kind: &ExecutionKind,
    ) {
        if !self.operations.matches(run, tab_id) {
            control.request_cancel();
            return;
        }
        let finished = matches!(event, ScriptEvent::Finished(_));
        let index = self.tabs.iter().position(|tab| {
            tab.id == tab_id
                && self.operations.active().is_some_and(|op| {
                    tab.connection_id.as_deref() == Some(op.connection_id.as_str())
                })
        });
        let Some(index) = index else {
            control.request_cancel();
            if finished {
                self.operations.finish(run);
            }
            return;
        };
        let provider = self.tab_provider(index);
        let tab = &mut self.tabs[index];
        match event {
            ScriptEvent::TransactionState { active } => {
                tab.transaction_state = if active { "In transaction" } else { "Idle" }.into();
            }
            ScriptEvent::TransactionFailed => {
                tab.transaction_state = "Transaction failed; Rollback required".into();
            }
            ScriptEvent::StatementStarted { sql } => {
                if let Some(connection) = &tab.connection_id {
                    self.query_history.record(connection.clone(), sql.clone());
                }
                let is_plan = super::transaction_mode::statement_is_plan(
                    matches!(kind, ExecutionKind::Plan),
                    &sql,
                );
                tab.begin_session_statement(sql, is_plan);
                tab.script_status = Some("Running script on a dedicated session…".into());
            }
            ScriptEvent::Stream(event) => {
                if matches!(event, StreamEvent::Completed { .. })
                    && let Some(sql) = &tab.script_sql
                    && let Some(state) = sql_intel::session_transaction_transition(sql)
                {
                    tab.transaction_state = state.into();
                }
                tab.apply_stream_event(event);
            }
            ScriptEvent::CleaningUp => {
                tab.transaction_state = "Session closed".into();
                tab.script_status =
                    Some("Closing script session and rolling back any open transaction…".into());
            }
            ScriptEvent::Finished(mut outcome) => {
                if let ExecutionKind::Schema(expected) = kind
                    && outcome.error.is_none()
                    && !outcome.cancelled
                    && !schema_readback_matches(tab.grid.pager.as_deref(), expected, provider)
                {
                    outcome.error = Some(format!(
                        "Server did not confirm {expected} as the current schema"
                    ));
                }
                if outcome.cancelled || outcome.cleanup_error.is_some() {
                    tab.transaction_state = "Session closed".into();
                } else if outcome.error.is_some()
                    && tab.transaction_state == "In transaction"
                    && let Some(state) = transaction_error_state(provider)
                {
                    tab.transaction_state = state.into();
                }
                let message = if !outcome.cancelled
                    && outcome.error.is_none()
                    && outcome.cleanup_error.is_none()
                {
                    format!("Tab session open · {}", tab.transaction_state)
                } else {
                    format!("{} · {}", outcome_message(&outcome), tab.transaction_state)
                };
                if outcome.cancelled || outcome.error.is_some() || outcome.cleanup_error.is_some() {
                    if !tab.execution_log.is_active() {
                        tab.execution_log
                            .start("Execution stopped / session feedback");
                    }
                    tab.apply_stream_event(StreamEvent::Error(message.clone()));
                }
                tab.script_status = Some(message);
                self.operations.finish(run);
            }
        }
    }
}

fn schema_readback_matches(
    pager: Option<&db_wire::QueryResultPager>,
    expected: &str,
    provider: DbDriverKind,
) -> bool {
    pager
        .and_then(|pager| pager.get_cell(0, 0))
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .is_some_and(|schema| {
            if provider == DbDriverKind::Db2 {
                schema.trim_end() == expected
            } else {
                schema == expected
            }
        })
}

fn transaction_error_state(provider: DbDriverKind) -> Option<&'static str> {
    match provider {
        DbDriverKind::Postgres => Some(super::transaction_mode::FAILED_TRANSACTION_STATE),
        _ => None,
    }
}

fn outcome_message(outcome: &ScriptOutcome) -> String {
    let mut message = match (&outcome.error, outcome.cancelled) {
        (Some(error), _) => format!("SQL stopped: {error}. Earlier committed/autocommit statements may remain applied"),
        (None, true) => "Script cancelled; session closed. Earlier committed/autocommit statements may remain applied".into(),
        (None, false) if outcome.cleanup_error.is_some() => "Script statements finished; session cleanup failed".into(),
        (None, false) => "Script complete; dedicated session closed".into(),
    };
    if let Some(error) = &outcome.cleanup_error {
        message.push_str(&format!(". {error}"));
    }
    message
}

#[cfg(test)]
mod tests {
    use super::outcome_message;
    use db_wire::script::ScriptOutcome;

    #[test]
    fn transaction_errors_do_not_assume_postgres_failure_semantics() {
        assert!(
            super::transaction_error_state(db_wire::DbDriverKind::Postgres)
                .unwrap()
                .contains("Transaction failed")
        );
        assert_eq!(
            super::transaction_error_state(db_wire::DbDriverKind::Sqlite),
            None
        );
        assert_eq!(
            super::transaction_error_state(db_wire::DbDriverKind::MySql),
            None
        );
    }

    #[test]
    fn errors_and_cleanup_failure_do_not_claim_all_writes_were_rolled_back() {
        let message = outcome_message(&ScriptOutcome {
            error: Some("COMMIT acknowledgement lost".into()),
            cancelled: false,
            cleanup_error: Some("connection closed".into()),
        });
        assert!(message.contains("COMMIT acknowledgement lost"));
        assert!(message.contains("Earlier committed/autocommit statements may remain applied"));
        assert!(message.contains("connection closed"));
        assert!(!message.contains("Script complete"));
        assert!(
            !outcome_message(&ScriptOutcome {
                cleanup_error: Some("rollback failed".into()),
                ..Default::default()
            })
            .contains("Script complete")
        );
        assert!(
            outcome_message(&ScriptOutcome {
                cancelled: true,
                ..Default::default()
            })
            .contains("Script cancelled")
        );
    }
}
