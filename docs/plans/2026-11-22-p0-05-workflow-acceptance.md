# P0-05 Workflow Acceptance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: executing-plans-strict. Steps use `- [ ]`.

**Goal:** Automate the P0-05 data workflow for all three enabled providers and give the desktop operator a traceable acceptance record.
**Architecture:** Extend the existing db-wire integration fixture and workspace ownership characterization. Keep the existing provider gate and release protocol. Extend the shared workspace save gate to protect table grid edits from departure.
**Tech stack:** Rust, Tokio, SQLite files, live PostgreSQL and MySQL, cargo.
**Branch:** `feature/p0-05-workflow-acceptance` from `origin/main`.

## Confirmed Requirements

- Require the full automated workflow for PostgreSQL, MySQL and SQLite. Keep MariaDB, SQL Server, Oracle and Db2 pending in the matrix, per the user's choice.
- Verify connect, browse, run, edit/save/read-back, export/read-back, cancel and disconnect. Include multi-connection ownership, stale async completion, pending edits and failed transaction recovery as required by P0-05.
- Desktop interaction is performed by the user. Leave the interactive acceptance matrix pending and keep P0-05 open until that evidence is recorded.
- Open a PR. The user authorized this session to plan and orchestrate using GPT-6.

## Decision Log

- Require workflow evidence for the three enabled providers. The four disabled presets stay pending, consistent with the connection gate and the user's approval. Not posted: the GitHub issue search found no P0-05 issue. Include this decision in the PR description.

## Scope Map

An acceptance result must demonstrate the stated workflow on its owning connection and provider, and must distinguish runtime evidence from unexecuted desktop cases.

- `crates/db-wire/tests/workflow_test.rs` SQLite and live PostgreSQL/MySQL fixtures → in (tasks 1–2).
- `crates/app/src/workspace/operations.rs` generation, tab and cancel ownership, called by table query/scripts/save → in (task 2, characterization).
- `script_saves.rs`, `tab_context_menu.rs` and `connections.rs` route close, Quit/restart, and disconnect/delete through the save gate → in (task 3 departure guard; the FK picker calls `close_saved_tabs` directly for its read-only picker only). Both the immediate and post-save/flush paths participate. Same-profile connection replacement also checks that owner's current grid drafts before swapping the driver. Explicit database deletion performs its own staged-edit guard in `drop_object_modal.rs`.
- `query.rs` table result replacement, `pinned_script.rs` operation ownership, `filter_builder.rs` filter/history safeguards, column draft guards, and the real navigator and export UI → in (task 4 desktop protocol and existing automated boundaries).
- Optional MariaDB/ODBC driver tests and the disabled connection presets → out (user approved keeping these pending).
- CI already discovers `workflow_test` and requires PostgreSQL/MySQL environment variables. No CI configuration changes are needed.

## Existing Data & States

- Same-named tables on different connections → edit/read-back must affect only the intended source, checked by the existing fixture on all three providers.
- NULL, empty and quoted Unicode text → retain existing direct/SQL replay assertions and the documented loaded CSV encoding limitation.
- An open uncommitted transaction → validate its write from the same session, close the owner, then verify a separate connection can reuse the key and recover the saved state (tasks 1–2).
- Query cancelled, or event arrives after a replacement operation → cancellation stays with the original connection and the obsolete event cannot release or match the replacement (tasks 1–2).
- Pending grid edits on table departure must stay in the owning tab and be resolved by Save or Cancel. Cover an active cell editor, staged data, targeted and unrelated tabs, and retry after cancelling (task 3). Existing script-result and column-draft guards still apply. Exercise the real focus and window paths in task 4.

## Global Constraints

- “Do not use Computer Use or request Computer Use permissions. The user verifies the GUI manually. Complete automated checks and report manual GUI acceptance as pending.”
- “Tests that need a live database must skip cleanly when the `FASTDB_TEST_*` variables are unset (CI sets `FASTDB_REQUIRE_DB_TESTS=1` to make them mandatory).”
- “Before calling work done: `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and run the app for UI changes.”
- Keep changes off main. The PR request authorizes the branch commits and push. Commit the reviewed plan separately.
- Run fixture SQL only on disposable databases. Keep fixture identifiers trusted and connection information out of test output.

## Technical Defaults

- Use the existing `tempfile` dependency for SQLite files, and use loopback PostgreSQL 16 and MySQL 8.4 fixtures as in CI.
- Use bounded event waits and native cancellation, with no fixed sleeps. An expensive but finite recursive SQLite aggregate gives time to observe interruption without streaming a large fixture.
- Use an independent insert of the same primary key after close to prove rollback and writer release. A read alone cannot distinguish rollback from a still-open uncommitted writer.

## Risks & Rollback

- A test can strand a provider lock or collide with another run → keep lock probes time-bounded, run only on disposable fixtures and preserve existing fixture cleanup. The SQLite files are owned by a temporary directory.
- Validating cancellation by error alone can accept a syntax failure → require the SQLite schema event, an interruption error, and read-back after cancellation. On PostgreSQL/MySQL distinguish native cancellation from an unrelated error. Run the full fixture on both.
- The table departure guard deliberately makes a rejected action retryable after Save or Cancel. It does not persist or auto-save grid drafts. No public API, schema or persisted format changes. Revert the guard and its tests as one commit if necessary.

## Implementation phase

### Task 1: Complete the SQLite workflow on a real file

**Files:** `crates/db-wire/tests/workflow_test.rs`, lines 1–121, using `rows`, `row`, and `run_statements` below that fixture.
**Interfaces:** `async fn sqlite_browse_edit_failure_recovery_and_export_readback()`; use existing `SqliteDriver::new`, `DatabaseDriver`, and `TabSession` interfaces.
**Test level:** integration against a real SQLite file.
**RED:** n/a for existing product behavior. Add characterization and verify that deliberately skipping close causes the independent insert to fail.

- [ ] Replace only the source's in-memory constructor with a real file and preserve the existing edit, type, CSV, SQL replay and connection-isolation assertions. Add the script check after the browse assertions:

  ```rust
  let directory = tempfile::tempdir().unwrap();
  let config = db_wire::ConnectionConfig {
      driver: DbDriverKind::Sqlite,
      database: directory.path().join("source.sqlite").to_str().unwrap().into(),
      ..Default::default()
  };
  let source: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new(config.clone()).unwrap());
  source.ping().await.unwrap();
  // After the existing browse assertions:
  let (started, completed, outcome) = run_statements(
      source.clone(),
      &[
          "SELECT id FROM sample ORDER BY id",
          "SELECT id FROM missing_sample",
          "SELECT 1",
      ],
      None,
  )
  .await;
  assert_eq!(started, ["SELECT id FROM sample ORDER BY id", "SELECT id FROM missing_sample"]);
  assert_eq!(completed, [3]);
  assert!(outcome.error.is_some_and(|error| error.contains("missing_sample")));
  ```

  Keep the existing other connection separate by binding both entries in the setup loop as references to `dyn DatabaseDriver`, or by wrapping the existing in-memory driver in `Arc<dyn DatabaseDriver>`. Keep the SQL replay destination separate from both. Rename the replay row count binding from `rows` to `restored_count` so it does not shadow the `rows` helper.
- [ ] Include staged insert and delete in the same existing SQLite Save as the update. Add `(4,'delete me')` to the source seed, expect four streamed rows and four rows in the first successful script result, then add the following before `plan_sql`:

  ```rust
  draft.inserted_rows.push(vec![Some("5".into()), Some("inserted".into())]);
  draft.deleted_rows.insert(3);
  ```

  Expect three changes from `apply_row_changes`, four saved/replayed/exported rows, ID 5 text `inserted` and absence of ID 4. Append the inserted row to the exact CSV expectation and the expected read-back array below; retain NULL and empty fields and the existing foreign fixture.

- [ ] Add bounded cancellation and close/reconnect checks at the end of that fixture. Use the helper in this task for SQLite and in task 2 for the live fixtures:

  ```rust
  async fn assert_session_rollback(
      source: Arc<dyn DatabaseDriver>,
      fresh: &Arc<dyn DatabaseDriver>,
      begin: &str,
      insert: &str,
      probe: &str,
  ) {
      use std::time::Duration;
      let owner = Arc::new(db_wire::script::TabSession::default());
      let (started, _, outcome) =
          run_statements(source.clone(), &[begin, insert], Some(&owner)).await;
      assert_eq!(started, [begin, insert]);
      assert!(outcome.error.is_none(), "{:?}", outcome.error);
      // Command statements may report zero-row completion on some providers.
      let (_, completed, outcome) = run_statements(source, &[probe], Some(&owner)).await;
      assert!(outcome.error.is_none(), "{:?}", outcome.error);
      assert_eq!(completed, [1], "the pinned session must see its write");
      assert!(rows(fresh, probe).await.is_empty());

      owner.close();
      let affected = tokio::time::timeout(
          Duration::from_secs(10),
          fresh.apply_row_changes(&[insert.to_owned()]),
      )
      .await
      .expect("the disconnected session must release the writer")
      .expect("an independent writer must be able to reuse the rolled-back key");
      assert_eq!(affected, 1);
      assert_eq!(rows(fresh, probe).await.len(), 1);
  }

  // At the end of the SQLite fixture:
  use std::time::Duration;
  let (sender, receiver) = flume::unbounded();
  let runner = source.clone();
  let mut query = tokio::spawn(async move {
      runner
          .execute_streaming(
              "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n \
               WHERE i < 1000000000) SELECT COUNT(*) FROM n",
              sender,
          )
          .await
  });
  assert!(matches!(
      tokio::time::timeout(Duration::from_secs(5), receiver.recv_async())
          .await
          .expect("SQLite query must start promptly")
          .unwrap(),
      StreamEvent::SchemaReady(_)
  ));
  let result = tokio::time::timeout(Duration::from_secs(5), async {
      loop {
          source.cancel_active_query().await.unwrap();
          if let Ok(joined) = tokio::time::timeout(Duration::from_millis(100), &mut query).await {
              break joined.unwrap();
          }
      }
  })
  .await
  .expect("interrupted SQLite query must stop promptly");
  assert!(format!("{:#}", result.unwrap_err()).contains("interrupt"));
  assert!(receiver.drain().any(|event| matches!(event, StreamEvent::Error(_))));
  let saved_sql = "SELECT id, value FROM sample ORDER BY id";
  let expected = [
      row(&[Some("1"), Some("名,'quoted'\nline")]),
      row(&[Some("2"), None]),
      row(&[Some("3"), Some("")]),
      row(&[Some("5"), Some("inserted")]),
  ];
  assert_eq!(rows(&source, saved_sql).await, expected);

  let fresh: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new(config.clone()).unwrap());
  assert_session_rollback(
      source.clone(),
      &fresh,
      "BEGIN",
      "INSERT INTO sample VALUES (9, 'reused after rollback')",
      "SELECT id FROM sample WHERE id = 9",
  )
  .await;
  assert_eq!(
      rows(&fresh, "SELECT value FROM sample WHERE id = 9").await,
      [row(&[Some("reused after rollback")])]
  );
  drop((source, fresh));
  let reopened: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new(config).unwrap());
  reopened.ping().await.unwrap();
  assert_eq!(
      rows(&reopened, "SELECT id, value FROM sample WHERE id < 9 ORDER BY id").await,
      expected
  );
  ```

- [ ] Run `cargo test -p db-wire --test workflow_test --locked sqlite_browse_edit_failure_recovery_and_export_readback -- --nocapture`. Temporarily omit `owner.close()` in the helper and verify the same test fails at the independent write. Restore and rerun. Keep test-only insert SQL constant and in the disposable fixtures.

### Task 2: Strengthen live disconnect and workspace ownership evidence

**Files:** `crates/db-wire/tests/workflow_test.rs`, live fixture close block around lines 453–474; `crates/app/src/workspace/operations.rs`, characterization around lines 98–148.
**Interfaces:** `assert_session_rollback` (task 1), `Operations::begin`, `matches`, `finish`, `request_cancel`.
**Test level:** PostgreSQL/MySQL integration and deterministic workspace unit test.
**RED:** n/a for existing product behavior. Validate the new assertions against controlled regressions.

- [ ] Replace the live disconnect read-only probe with the task 1 helper. Do not change the preceding save/export/cancel assertions or the fixture cleanup:

  ```rust
  assert_session_rollback(
      source.clone(),
      &fresh,
      p.begin,
      "INSERT INTO wf_items VALUES (9, 'reused after rollback', 1, NULL)",
      "SELECT id FROM wf_items WHERE id = 9",
  )
  .await;
  assert_eq!(
      rows(&fresh, "SELECT label FROM wf_items WHERE id = 9").await,
      [row(&[Some("reused after rollback")])]
  );
  ```

- [ ] In the live cancellation block require the provider's native cancellation error, as already asserted in `postgres_test.rs` and `mysql_test.rs`. Retain the fresh read:

  ```rust
  let error = format!("{:#}", result.unwrap_err());
  let marker = match p.kind {
      DbDriverKind::Postgres => "canceling statement",
      DbDriverKind::MySql => "interrupted",
      kind => panic!("unexpected workflow provider: {kind:?}"),
  };
  assert!(error.contains(marker), "expected cancellation: {error}");
  ```

- [ ] Extend `stale_completion_and_cancellation_keep_the_original_owner` to exercise distinct actual drivers and a late event targeting the current tab with an obsolete generation. Bind `other_driver` immediately after `driver`, use its clone for connection B's refused operation, and replace the old second-operation block:

  ```rust
  let other_driver = create_driver(ConnectionConfig {
      driver: db_wire::DbDriverKind::Sqlite,
      database: ":memory:".into(),
      ..ConnectionConfig::default()
  })
  .unwrap();
  assert!(!Arc::ptr_eq(&driver, &other_driver));
  // After assert!(runs.finish(first)):
  let second = runs
      .begin(8, "connection-b".into(), other_driver.clone(), OperationKind::Save)
      .unwrap();
  assert!(!runs.finish(first));
  assert!(!runs.matches(first, 8));
  assert!(!runs.matches(second, 7));
  assert!(runs.matches(second, 8));
  assert!(Arc::ptr_eq(&runs.active().unwrap().driver, &other_driver));
  assert_eq!(runs.active().unwrap().connection_id, "connection-b");
  assert!(runs.request_cancel().is_none());
  assert!(runs.finish(second));
  ```

- [ ] Run `cargo test -p fastdb-app --locked stale_completion_and_cancellation_keep_the_original_owner`. Temporarily make `matches` ignore generation, then verify the test fails at the obsolete generation assertion. Restore and rerun. With both disposable servers configured, run `FASTDB_REQUIRE_DB_TESTS=1 cargo test -p db-wire --test workflow_test --locked -- --nocapture`, and verify both named server tests exercised the live providers.

### Task 3: Prevent departure with unresolved table grid edits

**Files:** `crates/app/src/workspace/script_saves.rs`, `save_then` and `after_save` around lines 180–315 and the test module around lines 359–425; `crates/app/src/workspace/connections.rs`, successful replacement branch around lines 245–248.
**Interfaces:** `fn pending_grid_edit(tabs: &[TabState], ids: &[u64], action: &AfterSave) -> Option<(usize, &'static str)>`; private `Workspace::block_pending_grid_edit` calls it, selects the owning tab and publishes its existing grid error.
**Test level:** deterministic unit regression against the actual save-gate decision. Interactive close, Quit and disconnect are covered by the manual checklist.
**Root cause:** `f18b5ff` introduced a guard for editable script results. Table grids never entered that guard; `close_saved_tabs` removes them and after Quit their drafts are absent from session serialization. The source path was confirmed at `515d7b4` by tracing a staged table row through `close_tabs_where` → `save_then` → `close_saved_tabs`. A save/flush can also yield before final departure. The accepted L03 requirement rejects silently lost grid edits.

- [ ] Extract the existing save-gate condition into the pure helper without changing behavior first. Route the normal guard through the helper. Prove baseline RED for the actual action with the following regression, including a separate still-open cell editor built with the fields in `ui-grid/src/grid_element.rs`:

  ```rust
  #[test]
  fn table_departure_keeps_pending_grid_edits_in_their_owner() {
      use super::{AfterSave, pending_grid_edit};
      use crate::workspace::TabState;
      use sql_intel::SchemaCache;
      use ui_core::theme::Theme;

      let cache = Arc::new(SchemaCache::new().unwrap());
      let mut dirty = TabState::table(1, "main", "items", Theme::dark(), cache.clone());
      dirty.connection_id = Some("source".into());
      dirty.current_result.grid.changeset.set_cell(
          0, 1, Some(b"before".to_vec()), Some("staged".into()),
      );
      let clean = TabState::table(2, "main", "items", Theme::dark(), cache);
      let mut tabs = vec![clean, dirty];
      let actions = [
          AfterSave::Close(vec![1]),
          AfterSave::Quit,
          AfterSave::Restart,
          AfterSave::Disconnect("source".into()),
          AfterSave::DeleteConnection("source".into()),
      ];
      for action in &actions {
          assert_eq!(
              pending_grid_edit(&tabs, &[1], action),
              Some((1, "Save or Cancel row edits before continuing")),
          );
      }
      assert_eq!(pending_grid_edit(&tabs, &[2], &AfterSave::Close(vec![2])), None);
      assert_eq!(
          pending_grid_edit(&tabs, &[2], &AfterSave::Disconnect("other".into())),
          None,
      );
      for action in &actions[1..] {
          assert_eq!(
              pending_grid_edit(&tabs, &[2], action),
              Some((1, "Save or Cancel row edits before continuing")),
              "departure includes newly opened tabs in its current scope",
          );
      }
      assert_eq!(pending_grid_edit(&tabs, &[1], &AfterSave::None), None);
      assert_eq!(pending_grid_edit(&tabs, &[1], &AfterSave::Run(1)), None);
      assert_eq!(tabs[1].current_result.grid.changeset.get_modified_value(0, 1), Some("staged"));
      tabs[1].current_result.grid.changeset.clear();
      assert_eq!(pending_grid_edit(&tabs, &[1], &actions[0]), None);
      // Start a cell edit with no staged changes and require the same guard.
      tabs[1].current_result.grid.editing_cell = Some(ui_grid::grid_element::CellEdit {
          row: 0,
          col: 1,
          text: "editing".into(),
          caret: 7,
          anchor: None,
      });
      assert_eq!(
          pending_grid_edit(&tabs, &[1], &actions[0]),
          Some((1, "Save or Cancel row edits before continuing")),
      );
  }
  ```

- [ ] Implement the departure predicate in the helper. Preserve existing script-result action behavior and error text. Apply the table branch only for the actionable targets:

  ```rust
  fn pending_grid_edit(
      tabs: &[TabState], ids: &[u64], action: &AfterSave,
  ) -> Option<(usize, &'static str)> {
      if matches!(action, AfterSave::None) {
          return None;
      }
      let leaves_table = matches!(
          action,
          AfterSave::Close(_) | AfterSave::Quit | AfterSave::Restart
              | AfterSave::Disconnect(_) | AfterSave::DeleteConnection(_)
      );
      tabs.iter().enumerate().find_map(|(index, tab)| {
          let affected = match action {
              AfterSave::Quit | AfterSave::Restart => true,
              AfterSave::Disconnect(owner) | AfterSave::DeleteConnection(owner) => {
                  tab.connection_id.as_deref() == Some(owner.as_str())
              }
              _ => ids.contains(&tab.id),
          };
          let grid = &tab.current_result.grid;
          if !affected || !(grid.changeset.is_dirty() || grid.editing_cell.is_some()) {
              return None;
          }
          if tab.is_script() {
              Some((index, "Save or Cancel query result edits before continuing"))
          } else if leaves_table && tab.table_name.is_some() && tab.column_editor.is_none() {
              Some((index, "Save or Cancel row edits before continuing"))
          } else {
              None
          }
      })
  }
  ```

  Move the existing selection, feedback and notify to `pub(super) fn block_pending_grid_edit(&mut self, ids: &[u64], action: &AfterSave, cx: &mut Context<Self>) -> bool`. Call it in `save_then`, before asynchronously completed saves dispatch `after_save`, and again before final Quit/restart in the flush callback. Use the original target IDs for Close and resolve Quit/restart and owner-scoped actions against the current tabs in those callbacks. Leave the existing column guard and filter/refresh behavior in place.
- [ ] In the async connect result callback, just before replacing an existing session in `connections.rs`, use the owner-scoped decision with an empty captured list. The successful result and its newly created driver can be dropped safely. Keep the current connection and show the existing form error for the rejected retry:

  ```rust
  if this.connections.sessions.contains_key(&conn_id) {
      if this.block_pending_grid_edit(&[], &AfterSave::Disconnect(conn_id.clone()), cx) {
          this.form.test_result =
              Some(Err("Save or Cancel pending row edits before reconnecting".into()));
          cx.notify();
          return;
      }
      this.disconnect_by_id_now(&conn_id);
  }
  ```

- [ ] Run `cargo test -p fastdb-app --locked table_departure_keeps_pending_grid_edits_in_their_owner` before and after the predicate change. Record the failing assertion and passing command. Read all departure callers and launch the app with an isolated profile. Update the table guide and acceptance case L03 to mention the Save/Cancel gate; leave the manual checks pending.

### Task 4: Verification & Docs

**Files:** `docs/WORKFLOW_ACCEPTANCE.md` provider matrix/protocol/evidence, `docs/ROADMAP.md` P0-05, `docs/PROVIDERS.md` live workflow evidence, `docs/guide/navigator-and-tables.md`, `docs/guide/not-built-yet.md`, `docs/testing/0.2.0-postgres-manual-acceptance.md`, this plan.
**Interfaces:** Existing desktop workflow protocol and PostgreSQL runbook case IDs.
**RED:** n/a — no executable behavior for the prose, checked with link and full-diff review.

- [ ] Update the SQLite matrix row and automated evidence to describe the file fixture, interruption, writer-wait rollback proof and reopen. Record the exact PostgreSQL/MySQL versions and distinguish existing assertions from this run. Keep the four disabled provider rows pending.
- [ ] Add a desktop recording template and mapping for connect/browse/run/edit/export/cancel and the cross-cutting ownership, stale-event, pending-edit and failed-transaction cases. Preserve the existing repeatable protocol and link the PostgreSQL runbook's required case IDs. Keep historical SQLite skip visible and current interaction pending. Preserve the roadmap checkbox while desktop acceptance is pending.
- [ ] Review the complete tracked and untracked diff. Run `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked`, `cargo build --workspace --all-targets --locked`, and `FASTDB_REQUIRE_DB_TESTS=1 cargo test --workspace --locked -- --nocapture` with the disposable PostgreSQL 16 and MySQL 8.4 fixtures configured. Inspect skips and real live workflow results. CI runs the tests in its live database job. Record both the CI-version result and the already reproduced PostgreSQL 18.7 baseline query-plan assertion failure.
- [ ] Run `git diff --check` and verify document links. Complete independent code review and the final self-review gates. Record manual desktop acceptance and the unconfigured MariaDB/ODBC and TLS suites as pending. Dispose of temporary test containers.
- [ ] Commit the audited plan and scoped changes, fetch `origin`, verify `git merge-base --is-ancestor origin/main HEAD`, push the feature branch, open the PR, and verify its URL and head/base. The PR must state the enabled-provider decision and evidence limitations.
