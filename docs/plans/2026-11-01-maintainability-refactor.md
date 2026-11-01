# Maintainability Refactor Implementation Plan

Date: 2026-11-01

## Goal

Make connection, query, grid and script changes easier to implement and verify before adding more features. Preserve the existing eight-crate architecture while replacing shared workspace state with explicit ownership and small, testable interfaces.

This document authorizes no implementation, release or deployment by itself. Implement the phases incrementally when requested; keep this plan in its own documentation commit.

## Review baseline

The source review found:

- `Workspace` has 72 public fields and 26 `impl Workspace` blocks across a workspace directory of approximately 11,000 lines. File extraction has not established state ownership.
- Connection form selection and query/edit SQL generation share `selected_driver_kind`. Editing a form must not change the dialect of an existing session.
- Active connection state is moved field by field into and out of `LiveConnection`, while tabs, schema completion and query execution depend on workspace-global state.
- Identifier quoting and pagination live in the app, while database write SQL generation lives in `ui-grid::Changeset`.
- `grid_element.rs` has 1,464 lines including tests; `update_dialog.rs` has 1,108, `navigator.rs` 980, `properties_panel.rs` 964 and `connections.rs` 916. Responsibilities and long render functions matter more than raw line counts.
- A 530 ms cursor timer also checks script saves and notifies the workspace. Script writes are synchronous on the UI thread.
- Connection, query and DDL tests are mixed into `assets.rs`. Properties rendering also constructs placeholder metadata.
- `cargo test --workspace --offline` passed locally. Tests requiring external databases may return early when their environment is unset; this is not live-provider validation.
- Formatting checks failed. Clippy failed on `clippy::unused_io_amount` in the updater HTTP test helper (`updater.rs:731` at review time), and reported additional warnings. CI does not currently enforce formatting or Clippy.

Line numbers and counts are review snapshots, not acceptance targets.

## Constraints and intended ownership

- Keep the current crates and streaming/paged result architecture. Do not introduce a new framework, generic event bus or service container.
- `Workspace` remains the GPUI composition root: it owns child entities, routes actions and coordinates workflows through explicit methods.
- A `ConnectionSession` owns its driver, provider kind, schema cache and metadata. A connection manager owns sessions by stable connection ID and the selected ID. Navigator expansion/selection is separate UI state keyed by connection ID.
- `ConnectionFormState` owns editable fields, selected preset, validation, field history and test feedback. Form changes cannot mutate an established session.
- Every table and script tab records its owning connection ID. Connection activation selects an existing session rather than transferring metadata between parallel sets of fields.
- Query execution records the owning connection ID, tab ID, run ID and driver. Form selection or active-tab changes cannot redirect execution or cancellation.
- `ui-grid` owns presentation, selection and staged edits. `db-wire` owns provider SQL syntax and database write planning. `mem-engine` continues to own paged row storage.
- Persistence works on immutable snapshots outside GPUI mutation callbacks. Completion events update state only when their identity/revision is still current.
- Reduce blanket `use super::*` imports at extracted boundaries. Prefer private fields and narrow `pub(super)`/`pub(crate)` APIs over exposing mutable internals.
- Preserve existing saved-connection JSON and script paths. Keychain migration, new providers and unrelated product features are separate work.

## Phase 1: Establish reliable verification

Implementation note: the requested one-commit-per-phase structure combines baseline formatting, lint cleanup, test relocation and CI changes in this phase. Form/session regression tests land with Phase 2.

- [x] Fix the updater test server to handle partial reads and detect the end of the HTTP request headers, EOF and read errors. Do not replace the call with a fixed-size `read_exact` that can wait forever on a shorter request.
- [x] Apply `cargo fmt --all` in a dedicated formatting commit before moving code.
- [x] Review existing Clippy warnings; fix them in focused changes and document any narrowly scoped allowance. Avoid mass automated fixes mixed into structural moves.
- [x] Add `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D warnings` to the macOS CI job once the workspace is clean. Install the required toolchain components explicitly.
- [x] Move connection tests to connection persistence, stream/fetch tests to query/result state, and DDL tests to the SQL/DDL owner. Leave asset-loading tests in `assets.rs`.
- [x] Add regression tests for form/session isolation and tab connection ownership alongside the fixes in Phase 2; use fake drivers where server behavior is irrelevant.

Acceptance: formatting, strict Clippy and workspace tests pass. Existing test assertions survive relocation. CI fails on formatting drift or newly introduced warnings.

## Phase 2: Separate connection forms, sessions and navigator state

Suggested modules under `crates/app/src/`: `connections/{config,form,session,manager}.rs`; retain view code under `workspace/` until Phase 6. These names are guidance, not a requirement to create every file immediately.

- [x] Extract saved-connection serialization, legacy migration and atomic config writing from `impl Workspace` into a repository with an injectable config path. Preserve the current file format and test with temporary directories.
- [x] Extract `ConnectionFormState`, including preset selection and form-to-config validation. Capture form identity and a request ID before asynchronous test/connect operations; reject stale responses after the form is edited, closed or replaced.
- [x] Introduce `ConnectionSession` and a session registry. Remove the duplicate active metadata fields and field-by-field `park_active_connection`/`activate_live_connection` transfers.
- [x] Give each session its own schema cache. Bind editor completion to the tab's session; switching the navigator must not clear or rebuild unrelated editors' caches.
- [x] Store navigator expansion and selection per connection, independently of connection persistence and driver lifetime.
- [x] Bind table tabs as well as script tabs to stable connection IDs. Resolve query dialect, metadata and write targets through the owning session; never through the connection form.
- [x] Preserve tab results and pending edits when switching connections. If a session is disconnected, retain tab contents and require reconnection to that tab's connection before execution or Save.

Regression cases: edit/cancel a different provider preset while a session stays connected; switch between two connections with identical schema/table names; switch back to a tab with staged edits; receive an old connection-test response after reopening the form; disconnect/reconnect without changing a tab's owner.

Acceptance: form state has no path to select an established session's SQL dialect. Activating a connection does not manually copy its metadata into `Workspace`. Results, completion and edits remain associated with the correct connection.

## Phase 3: Extract query execution and tab result state

Depends on Phase 2 session identity.

- [x] Split `query.rs` into tab/result state, fetch planning, execution coordination and script-tab actions. Keep pure state transitions independent of `Context<Workspace>`.
- [x] Replace `is_running_query`, `cancel_requested` and generation fields with an explicit run state carrying run/tab/connection identity. Keep the existing one-active-operation-per-workspace policy for this refactor; per-session concurrency is a separate feature.
- [x] Capture the actual driver in every run/cancel/save operation. Switching the selected connection must not cancel or write through a different driver.
- [x] Route streaming events by stable identities and reject stale events. On closing a running tab, cancel its operation through the captured owner; handle terminal errors and a channel closing without a terminal event.
- [x] Model row-edit Save as an owned asynchronous operation too. Report failure to the originating tab, retain pending edits on failure, and prevent edits during Save or clear only the saved revision on success.
- [x] Keep row appends incremental and preserve the pager, fetch size, ordering and error-banner behavior.

Regression cases: tab activation/reordering/closure during a run; connection switch before Cancel; disconnect/reconnect during a run; late completion from an obsolete run; Save failure; tab closure or edits while Save completes; fetch-next append and schema mismatch.

Acceptance: terminal events affect only their operation and tab. Query and Save lifecycle tests can use deterministic fake drivers without constructing the full workspace UI.

## Phase 4: Consolidate SQL dialect and write planning

Depends on Phase 2 so provider identity is authoritative.

- [x] Add a small provider-aware SQL module in `db-wire` for quoting identifiers/qualified names, table pagination and existing row-write syntax. Consolidate duplicated implementations in app query code and `ui-grid::SqlDialect`.
- [x] Keep UI filter text and script statement selection in the app/`sql-intel`; pass explicit inputs into SQL planning rather than passing a workspace or tab object.
- [x] Translate staged grid edits into a database-neutral change request at the app boundary. Move SQL generation into `db-wire` without making `db-wire` depend on `ui-grid` or adding a reverse dependency.
- [x] Preserve original row/key values, deterministic statement ordering, NULL/default distinctions and the driver contract that each write affects exactly one row or the transaction rolls back.
- [x] Move `generate_table_ddl` to the SQL owner if it generates executable provider-specific SQL. Keep badges and visual formatting in UI modules. Represent unsupported or incomplete metadata explicitly instead of claiming accurate DDL.
- [x] Add SQL output tests for PostgreSQL, MySQL/MariaDB, SQLite, SQL Server, Oracle and Db2, including embedded identifier delimiters, literal escaping, pagination and empty/default inserts. Return explicit errors for unsupported syntax rather than treating all providers as interchangeable ANSI SQL.
- [x] Preserve live transaction tests and extend affected provider tests where environments are available. Document unvalidated provider behavior in `docs/PROVIDERS.md`.

Validation note: generated writes are exercised against bundled SQLite. External provider/TLS tests require configured servers; Oracle/Db2 default-only inserts now fail explicitly rather than generating unverified syntax.

Acceptance: `ui-grid` no longer generates SQL or selects a provider dialect. Query and edit planning derive syntax from the same session-owned provider. Existing streaming/export behavior remains intact.

## Phase 5: Decouple autosave, filesystem I/O and cursor rendering

Depends on Phase 3 script/result ownership.

- [ ] Expose editor content revisions/change notifications. Schedule a separate debounce after text changes; cursor movement, selection and blinking do not schedule saves.
- [ ] Capture an immutable script path/text/revision snapshot and write through a background executor. Serialize writes per path so an older slow write cannot overwrite newer text.
- [ ] Use atomic temporary-file replacement for script writes. Update the saved revision only after the matching snapshot succeeds; preserve dirty state if later edits exist.
- [ ] Preserve save-before-run, close and quit semantics. Await the required save snapshot asynchronously; keep a tab open on close-save failure, and do not exit before required saves finish or the user explicitly discards them. Report errors on the owning tab.
- [ ] Move directory listing/script reads and connection-config writes off UI callbacks where they may block. Keep storage interfaces injectable for deterministic ordering/error tests.
- [ ] Restrict the cursor timer to the focused input/editor region. Notify only affected panes for query progress, selection and cursor updates; remove periodic workspace-wide notifications caused by blinking.

Regression cases: rapid edits coalesce; edit during an in-flight write; slow writes complete out of order; write failure and retry; close/quit during debounce; no writes on cursor-only changes. Use controlled completion/fake storage rather than timing-sensitive sleeps.

Acceptance: UI mutation/render callbacks perform no script filesystem I/O. Autosave cannot lose a newer revision, and cursor blinking does not invalidate the navigator/top bar. Verify manually with a large script and multiple open tabs.

## Phase 6: Split large views by responsibility

Build on the ownership established above; file moves alone do not complete this phase.

- [ ] Split grid width measurement/layout, state/edit commands, header rendering and row/cell rendering into cohesive modules. Keep virtualization, column sizing, selection, clipboard and undo/redo tests at their logical owners.
- [ ] Extract navigator connection/schema/table nodes and section rendering. Build its metadata view model on connection/schema changes instead of repeatedly transforming all metadata on every render.
- [ ] Split Properties sections into focused render functions/components using an explicit metadata view model. Remove hardcoded sample-table special cases and fabricated IDs/owners/statistics from production rendering; show unavailable values explicitly.
- [ ] Extract updater check/download state and task orchestration from `Workspace`/`update_dialog.rs`. Render checking, available, downloading, complete and error states through small components. Keep platform installer behavior in `updater`.
- [ ] Group focus/input/layout/menu state under appropriate owners. Reuse the existing text input/history abstractions instead of retaining parallel workspace-level caret/anchor flags.
- [ ] Make extracted GPUI components own their state/subscriptions where useful. Existing cached `Pane` wrappers must not remain the only boundary while all actions still mutate the full workspace.

Acceptance: `Workspace` primarily composes components and routes commands; render functions receive narrow state. Large views have identifiable subcomponents, no database calls/filesystem writes in render, and no placeholder data generation there. Do not split cohesive code merely to meet an arbitrary line limit.

## Delivery and validation

- Implement Phases 1, 2 and 3 before extending connection/query behavior. Phase 4 follows session ownership; Phase 5 follows execution/script ownership; Phase 6 follows the corresponding extracted models.
- Use small logical commits when implementation commits are requested. Keep formatting, CI/build changes and documentation-only work in separate commits; avoid one workspace-wide refactor commit.
- After each extraction, run owning-crate tests and compile affected consumers. Before declaring implementation complete, run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`.
- Run live PostgreSQL/MySQL and TLS checks for affected driver/SQL behavior. Provider tests must skip cleanly without configured environments and be mandatory in the appropriate CI job. Report skips separately from actual database validation.
- Launch the app for UI changes and smoke-test: connect/edit/cancel dialog; two connections with overlapping table names; table/script tab switching; run/cancel/fetch-next; staged edits/Save/undo; script autosave/close/quit; column resize/scroll/copy; Properties; update dialog success/error states.
- Update README behavior descriptions for tab ownership, unavailable Properties values or save behavior when those changes land. Update `docs/PROVIDERS.md` for provider behavior changes.
- Record completed checkboxes and material design deviations in this plan as implementation proceeds.

## Completion criteria

1. Session, form, query, navigator, updater and save lifecycles have explicit owners and tested boundaries.
2. Changing a form or switching tabs cannot redirect SQL, metadata, completion, cancellation or edits to another connection.
3. SQL generation is shared in the database layer; the grid supplies staged data rather than SQL.
4. Autosave preserves revision ordering without synchronous filesystem work on the UI thread or cursor-driven workspace refreshes.
5. Tests live beside their logic, large views are composed from focused parts, and CI enforces the clean formatting/lint baseline.
6. Required automated checks and manual UI scenarios pass, with external-provider validation limitations recorded explicitly.
