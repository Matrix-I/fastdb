# Unify Script Result State Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: executing-plans-strict. Steps use `- [ ]`.

**Goal** — Store the active and retained result payloads in one Rust type so switching, closing, and starting results preserve the whole payload by construction.

**Architecture** — `TabState` owns one `current_result: ResultState`, the existing ordered `Vec<ResultState>` of inactive results, and the existing selected display index. Table tabs use the same current payload. The app moves whole payloads, including their grids and pager handles.

**Tech stack** — Rust 2024, Cargo, GPUI, `VirtualDataGrid`, `QueryResultPager`.

**Branch** — `chore/script-result-state` for implementation. Baseline `045d4a3`.

## Confirmed Requirements

- Scope the refactor to active and retained result ownership. Preserve the behavior documented in [SQL editor](../guide/sql-editor.md), including 20 retained results total per script, per-result grid and plan state, close-to-neighbor selection, edit guards, and the empty state after the last close.
- Preserve table data browsing, streaming and fetch append behavior, tab-local execution feedback, and stale async completion guards. Preserve exact SQL text, persisted formats, and user-facing controls.

## Global Constraints

Project rules in `AGENTS.md` and `CLAUDE.md` apply, including these verbatim constraints.

- “Drivers stream rows into `mem-engine` as batches arrive; never collect a full result set in memory.”
- “Keep UI work off the render path: GPUI re-renders on every key press or hover, so avoid heavy work in `render`.”
- “Commit or push only when asked. Never force-push `main`.”
- “Do not use Computer Use or request Computer Use permissions. The user verifies the GUI manually. Complete automated checks and report manual GUI acceptance as pending.”
- “Clean up temporary debug/build artifacts after verification to avoid SSD growth. Keep the current runnable binary and reusable build dependencies.”
- “Before calling work done: `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and run the app for UI changes.”
- “Add or update tests in the crate that owns the logic. Tests that need a live database must skip cleanly when the `FASTDB_TEST_*` variables are unset (CI sets `FASTDB_REQUIRE_DB_TESTS=1` to make them mandatory).”

## Technical Defaults

- Keep the existing display-index mapping and relative order. A single ownership change is independently reviewable without a second migration to a different collection layout.
- Keep the active slot populated even when no script result is visible. The first script result and table data currently use this slot, including a grid constructed with the correct theme.
- Use explicit `tab.current_result` access and retain the existing field names. Rust can borrow sibling fields independently, and searches can trace the payload without `Deref` or compatibility setters.
- Keep record view, file-dialog busy/plan request state, execution log, session, workspace operation routing, and async request tokens at their current ownership levels.

## Risks & Rollback

| Risk | Mitigation and rollback |
| --- | --- |
| Result order or pager identity drifts on switch or eviction. | Check selection, retained order, pointer identity, and close/reset against the existing behavior. Revert the single source-and-test change if parity fails. |
| First execution clears a result-independent grid or changes its theme. | Preserve the existing first-result branch and constructors. Keep the existing table and script stream tests. |
| Field migration changes an async capture or borrow. | Preserve captured tab IDs, pager pointers, and operation checks. Run compilation and stale-result tests. Borrow explicit payload fields wherever the original code used disjoint borrows. |
| Desktop/platform gaps | Launch on the available desktop and record manual acceptance separately for Linux/macOS. This change needs no persisted-data migration, API compatibility layer, provider changes, or format rollback. |

## Task 1 · Store and move one result payload

**Files** — `crates/app/src/workspace/tab_state.rs` lines 13–100, 181–191, 214–290, 297–508, and 518–1499. The candidate caller inventory below covers app source files with matching member names. Update only receivers of type `TabState` and their tests.

**Consumes** — `VirtualDataGrid::new(Option<Arc<QueryResultPager>>, Theme) -> VirtualDataGrid` and `FetchState::default() -> FetchState`.

**Produces** — `ResultState::empty(grid: VirtualDataGrid, fetch_size: usize) -> Self` and `TabState::current_result: ResultState` with crate visibility. Existing `result_count(&self) -> usize`, `select_result(&mut self, index: usize)`, `close_result(&mut self, index: usize)`, and `new_script_result(&mut self, sql: String)` signatures stay in place.

**Test levels** — Unit tests for ownership, order and reset. Existing in-memory SQLite streaming and saved-plan filesystem tests exercise the real boundaries used by the migrated callers.

- [x] Add the following test in `script_result_tests` before changing production code. For this baseline run, write every `tab.current_result.FIELD` in the test as `tab.FIELD`. Run `cargo test -p fastdb-app retained_results_move_the_whole_payload_and_keep_the_latest_twenty` and expect one passing test. This establishes the lifecycle assertions against the current behavior.

```rust
#[test]
fn retained_results_move_the_whole_payload_and_keep_the_latest_twenty() {
    let mut tab = TabState::blank(1);
    for index in 0..20 {
        tab.new_script_result(format!("statement {index}"));
    }

    tab.select_result(1);
    let pager = Arc::new(
        QueryResultPager::new(vec![ColumnMeta::new("value", DataType::Text)]).unwrap(),
    );
    tab.current_result.grid.refill_pager(pager.clone());
    tab.current_result.fetch.size = 31;
    tab.current_result.fetch.size_text = "31".into();

    tab.new_script_result("statement 20".into());
    assert_eq!(tab.result_count(), 20);
    assert_eq!(tab.active_result, 19);
    assert_eq!(tab.current_result.script_sql.as_deref(), Some("statement 20"));
    assert_eq!(tab.current_result.fetch.size, 31);
    assert!(tab.current_result.grid.pager.is_none());

    tab.select_result(0);
    assert_eq!(tab.current_result.script_sql.as_deref(), Some("statement 1"));
    assert_eq!(tab.current_result.fetch.size, 31);
    assert!(Arc::ptr_eq(
        tab.current_result.grid.pager.as_ref().unwrap(),
        &pager,
    ));
    tab.close_result(0);
    assert_eq!(tab.current_result.script_sql.as_deref(), Some("statement 2"));

    for index in (1..tab.result_count()).rev() {
        tab.close_result(index);
    }
    tab.current_result.fetch.size = 43;
    tab.current_result.fetch.size_text = "uncommitted text".into();
    tab.close_result(0);
    assert_eq!(tab.result_count(), 0);
    assert_eq!(tab.active_result, 0);
    assert_eq!(tab.current_result.fetch.size, 43);
    assert_eq!(tab.current_result.fetch.size_text, "43");
    assert!(tab.current_result.grid.pager.is_none());
    assert!(tab.current_result.script_sql.is_none());
}
```

- [x] Restore the `tab.current_result.FIELD` accesses in the test as shown. Run `cargo test -p fastdb-app retained_results_move_the_whole_payload_and_keep_the_latest_twenty` again. Expect the test build to fail with a missing `current_result` field. The baseline run has already checked the behavioral assertions.
- [x] Replace `ScriptResult` with the following shared payload. Remove its eleven matching fields from `TabState`, add `pub(crate) current_result: ResultState`, and change `results` to `Vec<ResultState>`.

```rust
const SCRIPT_RESULT_LIMIT: usize = 20;

pub(crate) struct ResultState {
    pub(crate) grid: VirtualDataGrid,
    pub(crate) fetch: FetchState,
    /// Statement used by fetch next for a script result.
    pub(crate) script_sql: Option<String>,
    pub(crate) last_error: Option<String>,
    pub(crate) execution_time_ms: Option<u64>,
    pub(crate) affected_rows: Option<Option<u64>>,
    pub(crate) query_plan: Option<db_wire::query_plan::QueryPlan>,
    pub(crate) plan_comparison: Option<SharedString>,
    pub(crate) plan_scroll: ScrollHandle,
    pub(crate) show_plan: bool,
    pub(crate) query_edit_driver: Option<std::sync::Weak<dyn DatabaseDriver>>,
}

impl ResultState {
    fn empty(grid: VirtualDataGrid, fetch_size: usize) -> Self {
        Self {
            grid,
            fetch: FetchState {
                size: fetch_size,
                size_text: fetch_size.to_string(),
                ..FetchState::default()
            },
            script_sql: None,
            last_error: None,
            execution_time_ms: None,
            affected_rows: None,
            query_plan: None,
            plan_comparison: None,
            plan_scroll: ScrollHandle::new(),
            show_plan: false,
            query_edit_driver: None,
        }
    }
}
```

- [x] In `TabState::blank` replace the eleven removed initializers with the first initializer below. In `table` replace its `grid,` initializer with the second, retaining `set_table_name`. In both `script` and `database_script` replace their `grid:` initializer with the third.

```rust
// blank
current_result: ResultState::empty(VirtualDataGrid::new(None, Theme::dark()), DEFAULT_FETCH_SIZE),
// table
current_result: ResultState::empty(grid, DEFAULT_FETCH_SIZE),
// script and database_script
current_result: ResultState::empty(VirtualDataGrid::new(None, theme), DEFAULT_FETCH_SIZE),
```

- [x] Replace the four transition methods with the following code within the existing `impl TabState`. In `invalidate_dropped_object_results`, migrate the active field accesses to `self.current_result` and keep the retained loop.

```rust
pub(super) fn result_count(&self) -> usize {
    self.results.len() + usize::from(self.current_result.script_sql.is_some())
}

pub(super) fn select_result(&mut self, index: usize) {
    if index >= self.result_count() || index == self.active_result {
        return;
    }
    let grid = &self.current_result.grid;
    if grid.changeset.is_dirty() || grid.editing_cell.is_some() {
        self.current_result.last_error =
            Some("Save or Cancel result edits before switching results".into());
        return;
    }
    self.close_record_view();
    let from_left = index < self.active_result;
    let slot = if from_left { index } else { index - 1 };
    let old_slot = if from_left {
        self.active_result - 1
    } else {
        self.active_result
    };
    let next = self.results.remove(slot);
    let previous = std::mem::replace(&mut self.current_result, next);
    self.results.insert(old_slot, previous);
    self.active_result = index;
}

pub(super) fn close_result(&mut self, index: usize) {
    if index >= self.result_count() {
        return;
    }
    if index != self.active_result {
        let slot = if index < self.active_result {
            index
        } else {
            index - 1
        };
        self.results.remove(slot);
        if index < self.active_result {
            self.active_result -= 1;
        }
        return;
    }
    let grid = &self.current_result.grid;
    if grid.changeset.is_dirty() || grid.editing_cell.is_some() {
        self.current_result.last_error =
            Some("Save or Cancel result edits before closing results".into());
        return;
    }
    if self.result_count() > 1 {
        let neighbor = if index + 1 < self.result_count() {
            index + 1
        } else {
            index - 1
        };
        self.select_result(neighbor);
        self.close_result(index);
        return;
    }
    self.close_record_view();
    self.current_result = ResultState::empty(
        VirtualDataGrid::new(None, self.editor.theme.clone()),
        self.current_result.fetch.size,
    );
    self.active_result = 0;
}

pub(super) fn new_script_result(&mut self, sql: String) {
    self.close_record_view();
    let fetch_size = self.current_result.fetch.size;
    if self.current_result.script_sql.is_some() {
        let next = ResultState::empty(
            VirtualDataGrid::new(None, self.editor.theme.clone()),
            fetch_size,
        );
        let previous = std::mem::replace(&mut self.current_result, next);
        self.results.insert(self.active_result.min(self.results.len()), previous);
        // Bound retained grid metadata and spill files as well as row memory.
        if self.results.len() >= SCRIPT_RESULT_LIMIT {
            self.results.remove(0);
        }
    } else {
        self.current_result.fetch.size_text = fetch_size.to_string();
        self.current_result.query_edit_driver = None;
    }
    self.active_result = self.results.len();
    self.current_result.script_sql = Some(sql);
}
```

- [x] Inventory the concrete call sites with the following command. The output includes unrelated structs with the same field names. Apply the explicit receiver mapping only where Rust resolves the receiver to `TabState`. For example, `tab.grid` becomes `tab.current_result.grid`, `self.tabs[index].fetch` becomes `self.tabs[index].current_result.fetch`, and `self.plan_comparison` inside `impl TabState` becomes `self.current_result.plan_comparison`. Preserve `script_fetch_query` branching and paging inputs, table/session operations, external driver calls, SQL text, and event handlers.

```bash
rg -n '\.(grid|fetch|script_sql|last_error|execution_time_ms|affected_rows|query_plan|plan_comparison|plan_scroll|show_plan|query_edit_driver)\b' crates/app/src
cargo check -p fastdb-app --all-targets
```

The caller checklist under `crates/app/src/` includes `benchmark.rs` and these files under `workspace/`.

| Area | Candidate files |
| --- | --- |
| Workspace and tab setup | `mod.rs`, `connections.rs`, `column_editor.rs`, `preferences.rs`, `tab_state.rs` |
| Execution and result UI | `query.rs`, `fetch_plan.rs`, `params_modal.rs`, `pinned_script.rs`, `script_view.rs`, `script_tools.rs`, `script_files.rs`, `script_saves.rs`, `session_context.rs`, `session_controls.rs`, `table_transaction.rs` |
| Plans and editability | `query_plan.rs`, `query_plan/file_actions.rs`, `query_result_edit.rs`, `drop_object_modal.rs`, `data_edit.rs` |
| Data consumers | `data_panel.rs`, `table_view.rs`, `record_view.rs`, `export.rs`, `result_search.rs`, `value_editor.rs`, `value_panel.rs`, `grid_clipboard.rs`, `grid_paste.rs`, `grid_references.rs`, `grid_selection.rs`, `column_menu.rs`, `filter_builder.rs`, `filter_suggest.rs`, `keyboard.rs`, `text_input.rs` |

- [x] Run the commands below and expect all selected tests to pass. The SQLite test uses the actual driver and checks streaming plus a mid-script error. The plan-file test uses a real snapshot file and checks preserved results and rejection of a stale completion.

```bash
cargo test -p fastdb-app retained_results_move_the_whole_payload_and_keep_the_latest_twenty
cargo test -p fastdb-app workspace::tab_state
cargo test -p fastdb-app sqlite_script_streams_individual_results_and_stops_after_an_error
cargo test -p fastdb-app opening_and_comparing_files_preserves_prior_results_and_rejects_stale_completion
```

- [ ] Ready to commit the migrated app files and tests as `refactor(app): unify script result state` after the final gate and explicit authorization.

## Task 2 · Verification & Docs

**Files** — Inspect `docs/guide/sql-editor.md` lines 12–14, 31–37 and the result-panel-sizing entry. Inspect the complete implementation diff and all untracked files. The guides, README, and provider docs need no edits for behavior-preserving implementation.

**Consumes** — The Task 1 app patch. **Produces** — command results, parity evidence, and the final review verdict.

**Test levels** — Workspace unit/integration suite and desktop end-to-end acceptance. This verification task adds no production code.

- [x] Run `cargo fmt --all`, inspect `git status --short`, `git diff --check`, and the full `git diff HEAD`. Read any untracked files. Confirm every changed call site is a result-state migration or a focused test and the patch contains no secrets or debug output.
- [x] Run `cargo fmt --all -- --check`, `cargo check --workspace --all-targets`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace --all-targets`, and `cargo test --workspace`. Record each outcome and any provider tests skipped due to missing configuration.
- [ ] Run `cargo run -p fastdb-app`. For manual acceptance on an existing SQLite connection, open a script with the two existing read-only statements `SELECT 1` and `SELECT 2`, execute the script, switch results, close the selected result, then close the last result. Expect distinct grids, neighbor selection, and the editor filling the space after the final close. If available, open a saved plan and switch back to confirm its plan state and previous rows survive switching. Open a table Data tab to check initial load and fetch-next. Report which desktop and paths were actually exercised. Leave GUI interaction to the user, and report desktop launch failure separately if the environment cannot open the app.
- [x] Review result ordering, eviction, first and final results, dirty and editing guards, and error paths using Task 1's covered cases. Trace stable tab IDs, pager identity, and running-operation guards in async and UI callers. Confirm that moves retain the pager, add no row copies or render work, and preserve all security boundaries and persisted formats. Check names, stable Rust APIs, test assertions, and the single-owner design.
- [x] Apply `code-review-strict` to the final app diff. Address findings or report them with evidence. After any edits repeat the full gate. Clean up only temporary artifacts created by verification and preserve the runnable binary and reusable build dependencies.
- [x] Save the plan in the documentation commit `6402fd1`.

## Execution checkpoint

- Branch `chore/script-result-state` started from `6402fd1`, which contains the plan and has the same application sources as `045d4a3`.
- The workspace baseline passed 823 harness cases. The new characterization passed on the baseline, went red on the missing `current_result` field, and passed after the migration.
- All four focused commands passed, including 29 tab-state tests and the real SQLite streaming and saved-plan filesystem checks. The final workspace run reported 824 passed, zero failed. Provider tests requiring unset live-test variables took their configured skip paths.
- Workspace format, typecheck, Clippy with denied Rust warnings, and build checks passed. Cargo emitted unused-dependency warnings for four unchanged manifests.
- Linux verification used temporary development libraries. Tests needed native access to `/var/tmp/fastdb` plus loopback for the existing SSH test.
- Linux smoke launch with `cargo run -p fastdb-app` reached the app process and stayed running until the 12-second timeout with isolated config and spill storage. Desktop interaction on Linux and macOS remains pending.
- The result transitions preserve the display index and cap. Existing tests cover dirty-edit blocks, final close, append failure, and stale plan completions. The pager remains the same allocation, no row copies were added, and SQL inputs, operation guards, IDs, and persisted formats are unchanged.
- Independent review approved the lifecycle, grid, and workspace packages without findings in one round. The available Codex reviewers substituted for Opus, which the review tool cannot select.
