//! File dialogs and async snapshot operations bound to stable tab/result identity.
use super::*;

fn same_result(
    expected: &Option<Arc<QueryResultPager>>,
    current: &Option<Arc<QueryResultPager>>,
) -> bool {
    match (expected, current) {
        (None, None) => true,
        (Some(expected), Some(current)) => Arc::ptr_eq(expected, current),
        _ => false,
    }
}

struct LoadedPlan {
    pager: Arc<QueryResultPager>,
    plan: QueryPlan,
    comparison: Option<String>,
}

fn load(
    path: &std::path::Path,
    current: Option<(Arc<QueryResultPager>, DbDriverKind)>,
) -> Result<LoadedPlan> {
    let saved = files::read(path)?;
    let (pager, plan) = saved.restore()?;
    let comparison = current
        .map(|(current, provider)| {
            files::compare(&parse_plan_for(Some(provider), &current)?, &plan)
        })
        .transpose()?;
    Ok(LoadedPlan {
        pager,
        plan,
        comparison,
    })
}

impl TabState {
    fn accept_file_plan(
        &mut self,
        original: &Option<Arc<QueryResultPager>>,
        loaded: LoadedPlan,
        running: bool,
    ) -> Result<()> {
        ensure!(
            !running
                && same_result(original, &self.grid.pager)
                && !self.grid.changeset.is_dirty()
                && self.grid.editing_cell.is_none(),
            "Result changed while choosing a plan; reopen the file"
        );
        if let Some(report) = loaded.comparison {
            self.plan_comparison = Some(report.into());
            self.show_plan = true;
        } else {
            self.new_script_result("-- Opened saved query plan (read-only)".into());
            self.grid.set_pager(loaded.pager);
            self.query_plan = Some(loaded.plan);
            self.show_plan = true;
            self.plan_scroll = ScrollHandle::new();
            self.script_status = Some("Opened native plan; no SQL was executed".into());
        }
        Ok(())
    }
}

impl Workspace {
    pub(super) fn save_plan_file(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.operations.is_running() {
            return;
        }
        let Some(tab) = self
            .tabs
            .get_mut(index)
            .filter(|tab| !tab.plan_file_busy && tab.query_plan.is_some())
        else {
            return;
        };
        let Some(pager) = tab.grid.pager.clone() else {
            return;
        };
        let id = tab.id;
        let provider = tab.query_plan.as_ref().unwrap().provider;
        tab.plan_file_busy = true;
        tab.script_status = Some("Choose where to save the native query plan…".into());
        let dialog = cx.prompt_for_new_path(
            &platform_hal::paths::downloads_dir(),
            Some("query-plan.json"),
        );
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = match dialog.await {
                Ok(Ok(Some(path))) => {
                    cx.background_executor()
                        .spawn(async move {
                            let saved = files::SavedPlan::capture(&pager, provider)?;
                            files::write(&path, &saved)?;
                            Ok::<_, anyhow::Error>(Some(format!(
                                "Saved query plan to {}",
                                path.display()
                            )))
                        })
                        .await
                }
                Ok(Ok(None)) => Ok(None),
                Ok(Err(error)) => Err(anyhow::anyhow!("Cannot open plan save dialog: {error}")),
                Err(_) => Err(anyhow::anyhow!("Plan save dialog closed unexpectedly")),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.plan_file_busy = false;
                    match outcome {
                        Ok(message) => tab.script_status = message,
                        Err(error) => {
                            tab.script_status = None;
                            tab.last_error = Some(format!("Cannot save plan: {error}"));
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::workspace) fn open_plan_file(
        &mut self,
        index: usize,
        compare: bool,
        cx: &mut Context<Self>,
    ) {
        if self.operations.is_running() {
            return;
        }
        let Some(tab) = self
            .tabs
            .get_mut(index)
            .filter(|tab| tab.is_script() && !tab.plan_file_busy && !tab.file_busy)
        else {
            return;
        };
        if tab.grid.changeset.is_dirty() || tab.grid.editing_cell.is_some() {
            tab.last_error = Some("Save or Cancel result edits before opening a plan".into());
            cx.notify();
            return;
        }
        if compare && tab.query_plan.is_none() {
            return;
        }
        let id = tab.id;
        let original = tab.grid.pager.clone();
        let current = if compare {
            original
                .clone()
                .zip(tab.query_plan.as_ref().map(|plan| plan.provider))
        } else {
            None
        };
        tab.plan_file_busy = true;
        tab.script_status = Some("Choose a saved query plan JSON file…".into());
        let dialog = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(
                if compare {
                    "Compare with saved plan"
                } else {
                    "Open saved plan (no SQL execution)"
                }
                .into(),
            ),
        });
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = match dialog.await {
                Ok(Ok(Some(paths))) => match paths.into_iter().next() {
                    Some(path) => {
                        cx.background_executor()
                            .spawn(async move { load(&path, current).map(Some) })
                            .await
                    }
                    None => Ok(None),
                },
                Ok(Ok(None)) => Ok(None),
                Ok(Err(error)) => Err(anyhow::anyhow!("Cannot open plan dialog: {error}")),
                Err(_) => Err(anyhow::anyhow!("Plan dialog closed unexpectedly")),
            };
            let _ = this.update(cx, |this, cx| {
                let running = this.operations.is_running();
                if let Some(tab) = this.tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.plan_file_busy = false;
                    tab.script_status = None;
                    match outcome {
                        Ok(Some(loaded)) => {
                            if let Err(error) = tab.accept_file_plan(&original, loaded, running) {
                                tab.last_error = Some(error.to_string());
                            }
                        }
                        Ok(None) => {}
                        Err(error) => tab.last_error = Some(format!("Cannot open plan: {error}")),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::{TabState, files, load, same_result};
    use db_wire::{ColumnMeta, DataType, DbDriverKind, QueryResultPager, SqliteDriver};
    use std::sync::Arc;

    #[test]
    fn file_results_only_apply_to_the_captured_pager() {
        let original = Arc::new(QueryResultPager::new(Vec::new()).unwrap());
        let replacement = Arc::new(QueryResultPager::new(Vec::new()).unwrap());
        assert!(same_result(
            &Some(original.clone()),
            &Some(original.clone())
        ));
        assert!(!same_result(&Some(original.clone()), &Some(replacement)));
        assert!(!same_result(&Some(original), &None));
        assert!(same_result(&None, &None));
    }

    #[test]
    fn opening_and_comparing_files_preserves_prior_results_and_rejects_stale_completion() {
        let raw = |cost| {
            let pager = Arc::new(
                QueryResultPager::new(vec![ColumnMeta::new("QUERY PLAN", DataType::Text)]).unwrap(),
            );
            pager
                .append_row([Some(
                    serde_json::json!([{"Plan":{"Node Type":"Result","Total Cost":cost}}])
                        .to_string(),
                )])
                .unwrap();
            pager
        };
        let original = raw(5);
        let target = raw(8);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("native-plan.json");
        files::write(
            &path,
            &files::SavedPlan::capture(&target, DbDriverKind::Postgres).unwrap(),
        )
        .unwrap();
        let mut tab = TabState::database_script(
            1,
            "owner".into(),
            "main".into(),
            DbDriverKind::Sqlite,
            Arc::new(SqliteDriver::new_memory()),
            ui_core::theme::Theme::dark(),
        );
        tab.new_script_result("SELECT 42".into());
        tab.grid.set_pager(original.clone());
        let expected = Some(original.clone());
        assert!(
            tab.accept_file_plan(&expected, load(&path, None).unwrap(), true)
                .is_err()
        );
        assert_eq!(tab.result_count(), 1);
        assert!(Arc::ptr_eq(tab.grid.pager.as_ref().unwrap(), &original));
        tab.accept_file_plan(&expected, load(&path, None).unwrap(), false)
            .unwrap();
        assert_eq!(tab.result_count(), 2);
        assert_eq!(
            tab.query_plan.as_ref().unwrap().provider,
            DbDriverKind::Postgres
        );
        assert_eq!(
            tab.grid.pager.as_ref().unwrap().get_cell(0, 0),
            target.get_cell(0, 0)
        );
        tab.select_result(0);
        assert_eq!(tab.script_sql.as_deref(), Some("SELECT 42"));
        let comparison = load(&path, Some((original.clone(), DbDriverKind::Postgres))).unwrap();
        tab.accept_file_plan(&expected, comparison, false).unwrap();
        assert!(Arc::ptr_eq(tab.grid.pager.as_ref().unwrap(), &original));
        assert!(
            tab.plan_comparison
                .as_ref()
                .unwrap()
                .contains("Changed: Result")
        );
        tab.select_result(1);
        assert!(
            tab.accept_file_plan(&expected, load(&path, None).unwrap(), false)
                .is_err()
        );
        assert_eq!(tab.result_count(), 2);
    }
}
