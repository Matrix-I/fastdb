use super::script_tools::tool;
use super::*;
use gpui::prelude::FluentBuilder;
use std::sync::atomic::{AtomicBool, Ordering};
use ui_grid::search::{FindDirection, FindOutcome, find_displayed_cell};

const MAX_SEARCH_BYTES: usize = 4096;

struct WorkerGuard(Arc<AtomicBool>);
impl Drop for WorkerGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub(super) struct Search {
    tab: u64,
    pager: Arc<db_wire::QueryResultPager>,
    query: String,
    caret: usize,
    anchor: Option<usize>,
    history: LineHistory,
    message: String,
    cancel: Arc<AtomicBool>,
    running: bool,
}

impl Drop for Search {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn reveal_match(grid: &mut VirtualDataGrid, cell: ui_grid::GridCoord) {
    grid.auto_fetch_paused.set(true);
    grid.selection = ui_grid::GridSelection::single(cell.row, cell.col);
    if let Ok(row) = usize::try_from(cell.row) {
        grid.vertical_scroll_handle
            .scroll_to_item(row, ScrollStrategy::Center);
    }
    let x: f32 = grid
        .display_columns()
        .iter()
        .copied()
        .take_while(|&col| col != cell.col)
        .map(|col| grid.displayed_column_width(col))
        .sum();
    let handle = &grid.horizontal_scroll_handle;
    handle.set_offset(point(
        px(-x.min(f32::from(handle.max_offset().width))),
        px(0.0),
    ));
}

fn same_result(
    grid: &VirtualDataGrid,
    pager: &Arc<db_wire::QueryResultPager>,
    rows: u64,
    changes: &ui_grid::Changeset,
) -> bool {
    grid.pager
        .as_ref()
        .is_some_and(|current| Arc::ptr_eq(current, pager))
        && grid.row_count() == rows
        && grid.changeset == *changes
        && grid.editing_cell.is_none()
}

impl Workspace {
    pub(super) fn close_result_search(&mut self) {
        if let Some(search) = &self.result_search
            && let Some(tab) = self.tabs.iter().find(|tab| tab.id == search.tab)
            && tab
                .grid
                .pager
                .as_ref()
                .is_some_and(|pager| Arc::ptr_eq(pager, &search.pager))
        {
            tab.grid.auto_fetch_paused.set(true);
        }
        self.result_search = None;
    }

    pub(super) fn open_result_search(&mut self, tab_id: u64, cx: &mut Context<Self>) {
        if self.execution_busy(cx) {
            return;
        }
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
            return;
        };
        if tab.grid.editing_cell.is_some() {
            tab.last_error =
                Some("Finish or cancel the cell editor before searching results".into());
            cx.notify();
            return;
        }
        let Some(pager) = tab.grid.pager.clone() else {
            return;
        };
        tab.grid.auto_fetch_paused.set(true);
        tab.view_mode = DataViewMode::Grid;
        tab.close_record_view();
        self.close_result_search();
        self.result_search = Some(Search {
            tab: tab_id,
            pager,
            query: String::new(),
            caret: 0,
            anchor: None,
            history: LineHistory::new(),
            message: "Searches loaded values and staged edits; SQL NULL is not text.".into(),
            cancel: Arc::new(AtomicBool::new(false)),
            running: false,
        });
        cx.notify();
    }

    fn cancel_result_search(&mut self, cx: &mut Context<Self>) {
        if let Some(search) = &mut self.result_search {
            search.cancel.store(true, Ordering::Relaxed);
            search.running = false;
            search.message = "Cancellation requested; pending matches are discarded.".into();
        }
        cx.notify();
    }

    pub(super) fn result_search_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        if key.key == "escape" {
            let cleared = self.result_search.as_mut().is_some_and(|search| {
                ui_editor::line_input::clear_selection(
                    &mut search.caret,
                    &mut search.anchor,
                    search.query.chars().count(),
                )
            });
            if !cleared {
                self.close_result_search();
            }
        } else if (key.modifiers.platform || key.modifiers.control) && key.key == "w" {
            self.close_result_search();
        } else if matches!(key.key.as_str(), "enter" | "return") {
            self.find_in_result(
                if key.modifiers.shift {
                    FindDirection::Previous
                } else {
                    FindDirection::Next
                },
                cx,
            );
        } else if let Some(search) = &mut self.result_search {
            let previous = search.query.clone();
            handle_line_key(
                LineInput {
                    text: &mut search.query,
                    caret: &mut search.caret,
                    anchor: &mut search.anchor,
                    digits_only: false,
                    history: &mut search.history,
                },
                key,
                cx,
            );
            if previous != search.query {
                search.cancel.store(true, Ordering::Relaxed);
                search.running = false;
                if search.query.len() > MAX_SEARCH_BYTES {
                    search.query = previous;
                    search.caret = search.query.chars().count();
                    search.anchor = None;
                    search.history = LineHistory::new();
                    search.message =
                        format!("Search text is limited to {MAX_SEARCH_BYTES} UTF-8 bytes.");
                } else {
                    search.message.clear();
                }
            }
        }
        cx.notify();
    }

    fn find_in_result(&mut self, direction: FindDirection, cx: &mut Context<Self>) {
        if self.execution_busy(cx) {
            return;
        }
        let Some(search) = self.result_search.as_mut() else {
            return;
        };
        if search.running {
            return;
        }
        search.cancel.store(true, Ordering::Relaxed);
        search.running = false;
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == search.tab) else {
            return;
        };
        if !tab
            .grid
            .pager
            .as_ref()
            .is_some_and(|pager| Arc::ptr_eq(pager, &search.pager))
            || tab.grid.editing_cell.is_some()
        {
            search.message =
                "The original result changed or is being edited; close and reopen Find.".into();
            cx.notify();
            return;
        }
        if search.query.is_empty() || search.query.len() > MAX_SEARCH_BYTES {
            search.message = format!("Enter 1–{MAX_SEARCH_BYTES} UTF-8 bytes to find.");
            cx.notify();
            return;
        }
        if self
            .result_search_worker
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            search.message = "Previous search is stopping; retry shortly.".into();
            cx.notify();
            return;
        }
        let worker_guard = WorkerGuard(self.result_search_worker.clone());
        let pager = search.pager.clone();
        let changes = Arc::new(tab.grid.changeset.clone());
        let rows = tab.grid.row_count();
        let from = tab.grid.selection.focus;
        let columns = tab.grid.display_columns().to_vec();
        let worker_columns = columns.clone();
        let needle = search.query.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        search.cancel = cancel.clone();
        search.running = true;
        search.message = "Searching loaded result…".into();
        let worker_pager = pager.clone();
        let worker_changes = changes.clone();
        let worker_cancel = cancel.clone();
        let job = cx.background_executor().spawn(async move {
            let _guard = worker_guard;
            find_displayed_cell(
                &worker_pager,
                &worker_changes,
                &worker_columns,
                &needle,
                from,
                direction,
                || worker_cancel.load(Ordering::Relaxed),
            )
        });
        cx.spawn(async move |this, cx| {
            let outcome = job.await;
            let _ = this.update(cx, |this, cx| {
                this.apply_result_search(&pager, &changes, (rows, &columns), &cancel, outcome);
                this.notify_result(cx);
            });
        })
        .detach();
        cx.notify();
    }

    fn apply_result_search(
        &mut self,
        pager: &Arc<db_wire::QueryResultPager>,
        changes: &ui_grid::Changeset,
        (rows, columns): (u64, &[usize]),
        cancel: &Arc<AtomicBool>,
        outcome: FindOutcome,
    ) {
        let Some(search) = self.result_search.as_mut().filter(|search| {
            Arc::ptr_eq(&search.cancel, cancel) && !cancel.load(Ordering::Relaxed)
        }) else {
            return;
        };
        search.running = false;
        let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == search.tab) else {
            search.message = "The original result was closed.".into();
            return;
        };
        if !same_result(&tab.grid, pager, rows, changes) || tab.grid.display_columns() != columns {
            search.message = "Result or staged edits changed during search; search again.".into();
            return;
        }
        search.message = match outcome {
            FindOutcome::Found(cell) => {
                reveal_match(&mut tab.grid, cell);
                self.value_panel.format_override = None;
                self.value_panel.show_format_menu = false;
                format!(
                    "Match selected: row {}, column {}.",
                    cell.row + 1,
                    cell.col + 1
                )
            }
            FindOutcome::NotFound => "No matching loaded value.".into(),
            FindOutcome::Cancelled => "Search cancelled.".into(),
            FindOutcome::Failed(error) => format!("Search failed: {error}"),
        };
    }

    pub(super) fn render_result_search(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let search = self.result_search.as_ref().unwrap();
        let theme = &self.theme;
        let weak = cx.entity().downgrade();
        let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(search) = &mut this.result_search {
                    apply_pick(&mut search.caret, &mut search.anchor, index, kind, true);
                    cx.notify();
                }
            });
        });
        let panel =
            div()
                .w(px(520.0))
                .p_4()
                .flex()
                .flex_col()
                .gap_3()
                .rounded_md()
                .bg(theme.bg_surface)
                .text_color(theme.text_primary)
                .child("Find in loaded result — literal, case-sensitive")
                .child(
                    ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                        .px_2()
                        .py_1()
                        .border_1()
                        .border_color(theme.border_focused)
                        .child(render_field_content(
                            &search.query,
                            "Text to find",
                            true,
                            search.anchor,
                            search.caret,
                            true,
                            theme,
                            &pick,
                        )),
                )
                .child(search.message.clone())
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            tool("Previous")
                                .opacity(if search.running { 0.4 } else { 1.0 })
                                .when(!search.running, |button| {
                                    button.on_click(cx.listener(|this, _, _, cx| {
                                        this.find_in_result(FindDirection::Previous, cx)
                                    }))
                                }),
                        )
                        .child(
                            tool("Next")
                                .opacity(if search.running { 0.4 } else { 1.0 })
                                .when(!search.running, |button| {
                                    button.on_click(cx.listener(|this, _, _, cx| {
                                        this.find_in_result(FindDirection::Next, cx)
                                    }))
                                }),
                        )
                        .when(search.running, |bar| {
                            bar.child(tool("Cancel search").on_click(
                                cx.listener(|this, _, _, cx| this.cancel_result_search(cx)),
                            ))
                        })
                        .child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                            this.close_result_search();
                            cx.notify();
                        }))),
                );
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(panel)
    }
}

#[cfg(test)]
mod tests {
    use super::{Search, WorkerGuard, reveal_match, same_result};
    use db_wire::{ColumnMeta, QueryResultPager};
    use mem_engine::DataType;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use ui_core::theme::Theme;
    use ui_grid::{Changeset, VirtualDataGrid};

    #[test]
    fn revealing_a_match_does_not_trigger_bottom_auto_fetch() {
        let pager =
            Arc::new(QueryResultPager::new(vec![ColumnMeta::new("v", DataType::Text)]).unwrap());
        pager.append_row(vec![Some(b"match")]).unwrap();
        let mut grid = VirtualDataGrid::new(Some(pager.clone()), Theme::dark());
        let auto_fetch = |grid: &VirtualDataGrid| {
            crate::workspace::data_panel::auto_fetch_allowed(
                true,
                false,
                true,
                grid.auto_fetch_paused.get(),
                false,
            )
        };
        assert!(auto_fetch(&grid));
        assert!(
            !crate::workspace::data_panel::auto_fetch_allowed(true, false, true, false, true),
            "Find must block auto-fetch even if a wheel capture changes the grid hold"
        );
        reveal_match(&mut grid, ui_grid::GridCoord { row: 0, col: 0 });
        assert!(
            !auto_fetch(&grid),
            "programmatic Find reveal must not issue another query"
        );
        assert_eq!(
            grid.selection.focus,
            Some(ui_grid::GridCoord { row: 0, col: 0 })
        );
        grid.refill_pager(pager);
        assert!(auto_fetch(&grid));
    }

    #[test]
    fn stale_results_and_changed_edits_cannot_receive_a_search_selection() {
        let make_pager =
            || Arc::new(QueryResultPager::new(vec![ColumnMeta::new("v", DataType::Text)]).unwrap());
        let pager = make_pager();
        pager.append_row(vec![Some(b"value")]).unwrap();
        let mut grid = VirtualDataGrid::new(Some(pager.clone()), Theme::dark());
        let snapshot = Changeset::new();
        assert!(same_result(&grid, &pager, 1, &snapshot));
        assert!(!same_result(&grid, &make_pager(), 1, &snapshot));
        grid.changeset
            .set_cell(0, 0, Some(b"value".to_vec()), Some("edited".into()));
        assert!(!same_result(&grid, &pager, 1, &snapshot));
        grid.changeset.clear();
        grid.editing_cell = Some(ui_grid::CellEdit {
            row: 0,
            col: 0,
            text: "typing".into(),
            caret: 0,
            anchor: None,
        });
        assert!(!same_result(&grid, &pager, 1, &snapshot));
        grid.editing_cell = None;
        pager.append_row(vec![Some(b"appended")]).unwrap();
        assert!(!same_result(&grid, &pager, 1, &snapshot));
    }

    #[test]
    fn closing_search_cancels_worker_and_worker_drop_releases_capacity() {
        let cancel = Arc::new(AtomicBool::new(false));
        let search = Search {
            tab: 1,
            pager: Arc::new(QueryResultPager::new(Vec::new()).unwrap()),
            query: String::new(),
            caret: 0,
            anchor: None,
            history: crate::workspace::LineHistory::new(),
            message: String::new(),
            cancel: cancel.clone(),
            running: true,
        };
        drop(search);
        assert!(cancel.load(Ordering::Relaxed));
        let active = Arc::new(AtomicBool::new(true));
        drop(WorkerGuard(active.clone()));
        assert!(!active.load(Ordering::Acquire));
    }
}
