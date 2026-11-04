use super::*;
use db_wire::script::{ScriptControl, ScriptEvent};
use db_wire::session_context::{ContextKind, choices_sql, schema_statements};

const CONTEXT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);
const MAX_CONTEXT_CHOICES: usize = 64;

fn schema_change_allowed(state: &str) -> bool {
    matches!(state, "Idle" | "Session closed")
}

async fn load_choices(
    driver: Arc<dyn DatabaseDriver>,
    provider: DbDriverKind,
    kind: ContextKind,
) -> Result<Vec<String>, String> {
    let sql = choices_sql(provider, kind)?;
    let (tx, rx) = flume::unbounded();
    db_wire::script::run_script(
        driver,
        vec![sql.into()],
        Arc::new(ScriptControl::with_timeout(Some(CONTEXT_TIMEOUT))),
        tx,
    )
    .await;
    let mut choices = Vec::new();
    let mut finished = false;
    for event in rx.drain() {
        match event {
            ScriptEvent::Stream(StreamEvent::SchemaReady(pager)) => {
                if pager.total_rows() > MAX_CONTEXT_CHOICES as u64 {
                    return Err("More than 64 contexts are available; use explicit session SQL or a separate saved connection".into());
                }
                for row in 0..pager.total_rows() {
                    let bytes = pager
                        .get_cell(row, 0)
                        .ok_or("Context catalog returned NULL")?;
                    let name = String::from_utf8(bytes).map_err(|error| error.to_string())?;
                    db_wire::session_context::validate_name(&name)?;
                    choices.push(name);
                }
            }
            ScriptEvent::Finished(outcome) => {
                finished = true;
                if let Some(error) = outcome.error.or(outcome.cleanup_error) {
                    return Err(error);
                }
                if outcome.cancelled {
                    return Err("Context discovery cancelled or timed out".into());
                }
            }
            _ => {}
        }
    }
    if !finished {
        return Err("Context discovery stopped before completion".into());
    }
    if choices.is_empty() {
        return Err("No accessible contexts were reported by the provider".into());
    }
    Ok(choices)
}

impl Workspace {
    fn context_tab_index(&self, id: u64, driver: &Arc<dyn DatabaseDriver>) -> Option<usize> {
        self.tabs.iter().enumerate().find_map(|(index, tab)| {
            (tab.id == id
                && tab.is_script()
                && !tab.file_busy
                && self
                    .tab_session(index)
                    .driver
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, driver)))
            .then_some(index)
        })
    }

    pub(super) fn choose_sql_context(
        &mut self,
        tab_id: u64,
        kind: ContextKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.execution_busy(cx) {
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| {
            tab.id == tab_id && tab.is_script() && !tab.file_busy && !tab.script_file_dialog_busy
        }) else {
            return;
        };
        let provider = self.tab_provider(index);
        let Some(driver) = self.tab_session(index).driver.clone() else {
            return;
        };
        let error = if kind == ContextKind::Schema
            && !schema_change_allowed(&self.tabs[index].transaction_state)
        {
            Some("Commit or Rollback before changing schema".into())
        } else {
            choices_sql(provider, kind).err()
        };
        if let Some(error) = error {
            self.tabs[index].last_error = Some(error);
            cx.notify();
            return;
        }
        self.tabs[index].script_file_dialog_busy = true;
        self.tabs[index].script_status = Some("Loading accessible contexts…".into());
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let source = driver.clone();
            let choices = cx.background_executor().spawn(async move {
                db_wire::get_runtime().block_on(load_choices(source, provider, kind))
            }).await;
            let choices = match choices {
                Ok(choices) => choices,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                            tab.script_file_dialog_busy = false;
                            tab.last_error = Some(error);
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            let answer = cx.update(|window, cx| this.update(cx, |this, cx| {
                let index = this.context_tab_index(tab_id, &driver)?;
                let choices: Vec<_> = std::iter::once("Cancel").chain(choices.iter().map(String::as_str)).collect();
                let message = if kind == ContextKind::Database {
                    "Opens a new empty SQL tab. Its independent session opens on first execution. The original tab and its transaction are preserved. This is a temporary tab. Use Save SQL copy to keep its text."
                } else {
                    "Changes only this SQL tab's idle session and reads back the native schema. Commit or Rollback is required first."
                };
                this.tabs[index].script_status = Some("Choose a context…".into());
                Some(window.prompt(PromptLevel::Info, "SQL tab context", Some(message), &choices, cx))
            })).ok().and_then(Result::ok).flatten();
            let choice = match answer { Some(answer) => answer.await.ok(), None => None };
            let selected = choice.and_then(|choice| choice.checked_sub(1)).and_then(|index| choices.get(index)).cloned();
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                    tab.script_file_dialog_busy = false;
                }
                let Some(index) = this.context_tab_index(tab_id, &driver) else { return; };
                this.tabs[index].script_status = None;
                let Some(name) = selected else { cx.notify(); return; };
                if this.execution_busy(cx) { return; }
                match kind {
                    ContextKind::Schema => {
                        if !schema_change_allowed(&this.tabs[index].transaction_state) {
                            this.tabs[index].last_error = Some("Commit or Rollback before changing schema".into());
                        } else {
                            match schema_statements(provider, &name) {
                                Ok(sql) => this.start_pinned_schema_change(index, driver, sql.into(), name, cx),
                                Err(error) => this.tabs[index].last_error = Some(error),
                            }
                        }
                    }
                    ContextKind::Database => this.open_database_context(index, driver, name),
                }
                cx.notify();
            });
        }).detach();
    }

    fn open_database_context(
        &mut self,
        index: usize,
        source: Arc<dyn DatabaseDriver>,
        database: String,
    ) {
        let driver = match source.with_database(&database) {
            Ok(driver) => driver,
            Err(error) => {
                self.tabs[index].last_error = Some(error.to_string());
                return;
            }
        };
        let Some(connection_id) = self.tabs[index].connection_id.clone() else {
            return;
        };
        let provider = self.tab_provider(index);
        let id = self.allocate_tab_id();
        let tab = TabState::database_script(
            id,
            connection_id,
            database,
            provider,
            driver,
            self.theme.clone(),
        );
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.tab_scroll.scroll_to_item(self.active_tab);
        self.input.is_filter_focused = false;
        self.blur_fetch_size();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn schema_selection_requires_known_idle_state() {
        for state in [
            "In transaction",
            "Unknown",
            "",
            super::super::transaction_mode::FAILED_TRANSACTION_STATE,
        ] {
            assert!(!super::schema_change_allowed(state));
        }
        for state in ["Idle", "Session closed"] {
            assert!(super::schema_change_allowed(state));
        }
    }
}
