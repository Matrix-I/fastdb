# P2-03 PostgreSQL Session Monitor Implementation Plan

> For agentic workers: use executing-plans-strict. Track progress with checkboxes.

**Goal** Show PostgreSQL sessions, current queries, waits and blocking chains. Confirm query cancellation and session termination, verify the target, and refresh live state.

**Architecture** A connection-scoped workspace dialog owns a dedicated PostgreSQL connection, pinned to the same server throughout the dialog lifetime. The driver reads activity and checks current identities before signaling. The dialog binds confirmation to the selected snapshot row and original live connection.

**Tech stack** Rust, GPUI, tokio-postgres, PostgreSQL system views and signaling functions.

**Branch** `feature/p2-03-session-monitor`, based on `f1945ab`.

## Confirmed requirements

- PostgreSQL is the provider for P2-03.
- Initial load, manual Refresh and automatic refresh after an action.
- Display sessions, active queries, waits and blocking chains. Require explicit confirmation before canceling a query or terminating a session. Verify both actions against the selected live session.

## Global constraints

- Follow `CLAUDE.md` and the supplied `AGENTS.md`. Use parameterized values, bounded calls and results, explicit errors, least privilege and deterministic resource cleanup. Keep query text out of logs and persistence.
- Preserve existing driver and config contracts. Read-only profiles can inspect sessions and cannot signal them. PostgreSQL determines native visibility and permissions.
- Use test-first owning-crate coverage, format, Clippy, workspace tests and build, app startup, and live PostgreSQL checks. Manual GUI verification belongs to the user. Obtain independent review and check SQL plans with the live engine.
- Commit only when requested. The plan and feature remain ready for separate commits.

## Technical defaults

- Open **Session Monitor…** from a connected PostgreSQL profile's context menu. Use the connection credentials and transport already in use.
- Show server-wide client sessions with database labels. Bound the snapshot, identify omitted or invisible blockers, and represent prepared transactions explicitly. Use server-provided time for active duration.
- Retain a PostgreSQL connection for the dialog's lifetime so a refresh or action cannot silently switch servers. Show errors if the monitor connection closes and require reopening. Treat results as invalid if the originating profile connection changes.
- Cancel compares PID, backend start and active query start. Terminate compares PID and backend start. Both check the target on the pinned server immediately before signaling. PostgreSQL signaling accepts only a PID, so snapshot validation cannot make signaling and query transitions atomic. Report acknowledgment as signal sent and refresh without automatic signal retries. The live test waits for observed target disappearance and proves that the bystander remains responsive.

## Risks and rollback

- A terminated session can lose uncommitted work. The confirmation identifies the server profile, role and PID, with a distinct termination warning. A cancellation is bound to the observed active statement.
- Activity can be hidden by PostgreSQL permissions and can change during a snapshot. Display unavailable fields and incomplete blocker chains explicitly. Respect native permissions even on writable profiles.
- Busy, disconnected or superseded UI state cannot start another signal. A failed refresh clears actionable state. Ignore asynchronous results after a dialog closes or its profile is replaced.
- Roll back the dialog and isolated driver module together. No saved data, protocol, dependencies or schema changes are required.

## Task 1 · PostgreSQL activity and signals

- [x] Files: `crates/db-wire/src/postgres/mod.rs` module declarations; new `crates/db-wire/src/postgres/session_monitor.rs` and SQL files; new `crates/db-wire/tests/session_monitor_test.rs`.
- [x] Interface: `SessionMonitor::connect(config: ConnectionConfig) -> Result<Self>`, `snapshot(&self) -> Result<SessionSnapshot>`, `signal(&self, target: &SessionIdentity, action: SessionSignal) -> Result<()>` (async). `SessionSnapshot` owns bounded rows and a truncation flag. `SessionIdentity` carries server PID and its start time, with optional active query start.
- [x] First add live tests using dedicated fixture connections with three advisory-lock participants (root holder, blocked holder, leaf waiter) plus a bystander. Assert the exact edges, attempted stale/unsupported actions and their unchanged targets, and that cancel interrupts only the selected query (SQLSTATE 57014); terminate closes only the chosen backend. Add a restricted role without statistics visibility, then grant it `pg_read_all_stats` and verify native denial with `42501` while the target is visible and remains connected. Run `cargo test -p db-wire --locked --test session_monitor_test --no-run`; expect the missing monitor interface.
- [x] Implement on `PostgresDriver::connect_with_task` and `SocketGuard`, with a bounded statement timeout, bounded client waits, and read-only session defaults. Fix the SQL search path to `pg_catalog`. Use `pg_stat_activity` with server duration and call `pg_blocking_pids` only for lock waiters in the limited result. Bound blocker data and indicate omissions. Use parameterized PID and timestamps for the signal guard. Distinguish stale, denied and failed signaling, with no retries.
- [x] Run the suite against an isolated PostgreSQL cluster, inspect `EXPLAIN (ANALYZE, BUFFERS)` for listing and stale-target fixtures, and record evidence. Ready to commit with the workspace UI as `feat(app): monitor postgresql sessions and blockers`.

## Task 2 · Workspace monitor and confirmation

- [x] Files: `crates/app/src/workspace/{mod,keyboard,nav_context_menu}.rs` plus `script_saves.rs` and new `session_monitor.rs`. Read active transport and driver identity from `ConnectionSession` in `connection_state.rs`.
- [x] Interface: `can_open_session_monitor(&self, owner: &str) -> bool`, `open_session_monitor(&mut self, owner: &str, cx: &mut Context<Self>)`, `render_session_monitor(&self, cx: &mut Context<Self>) -> impl IntoElement + use<>`, and `session_monitor_key(&mut self, key: &Keystroke, cx: &mut Context<Self>)`.
- [x] First add model tests for selected identity, confirmation binding, read-only action suppression and cycle/missing/prepared blocker behavior. `cargo test -p fastdb-app --bin fastdb --locked session_monitor` should fail before model implementation.
- [x] Add a bounded, scrollable list and selected activity details with wait and blocker chain. Show explicit loading/empty/error/limited states. Use a captured confirmation, verify live owner before dispatch, serialize operations, and refresh after the action result; prevent stale callbacks or keyboard shortcuts from affecting underlying views. Run focused app model tests. Ready to commit with Task 1.

## Task 3 · Verification and docs

- [x] Update `docs/guide/connections.md`, `docs/guide/not-built-yet.md`, `docs/PROVIDERS.md`, and `docs/ROADMAP.md` with supported scope, PostgreSQL permissions and limits, live test evidence, and pending manual GUI acceptance.
- [x] After final edits run `cargo fmt --all`, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked`, `cargo test --workspace --locked`, `cargo build --workspace --locked`, focused live provider tests, and a bounded app startup with an isolated configuration. Review every changed and untracked file, then obtain independent review and repeat affected gates after changes.
- [x] App acceptance checklist for the user: open the monitor from PostgreSQL; load and manually refresh a live blocking chain; select the intended backend, dismiss and confirm both prompts, verify the automatic refresh, and inspect restricted/read-only states. Record exact outcomes and leave manual acceptance pending until observed. Clean up temporary fixtures and retain the runnable binary and reusable build dependencies.

## Plan review checkpoint

- Internal review corrected the app test target, native restricted-role case, and acknowledgment wording above. The external Opus call failed DNS in the sandbox and automatic approval denied its external transfer of repository context. This plan records interfaces, assertions, and implementation steps. The full executable test and implementation code resides in the listed source files, instead of being duplicated in the plan as required by the skill.

## Verification evidence

- Test-first checks failed on the missing PostgreSQL monitor API and missing workspace model before implementation. Live PostgreSQL 16.15 exercised the blocking chain, canceled query, terminated session, surviving bystander, stale snapshots, read-only profile, and native SQLSTATE `42501`. MySQL, vendor ODBC and opt-in TLS fixtures were unavailable or unset in the workspace run.
- PostgreSQL 16.15 `EXPLAIN (ANALYZE, BUFFERS)` with 100 client sessions inspected the snapshot and a safely nonmatching signal. The final snapshot returned 100 rows in 3.061 ms, with 109 buffer hits and in-memory 38 KiB sorts. The signal guard returned zero matches in 0.512 ms. The catalog view estimated one row for 100 actual and joined a two-row database catalog. No spill was observed. The initial run measured 0.625 ms and 0.230 ms respectively; these single runs are not a performance comparison.
- Code review round 1 led to a trusted SQL search path, bounded blocker edges, overlay-ordered Escape handling, a Quit guard around pending signals, bounded fixture cleanup after errors, and a model regression for captured confirmation and a replaced profile.
- A repeat PostgreSQL 16.15 plan run on 2026-11-15 used 100 active clients. The snapshot returned 100 rows in 0.562 ms with 109 shared-buffer hits on the execution path and 38 KiB in-memory sorts. The signal guard ran in 0.196 ms with 134 shared-buffer hits and three reads, matching zero rows. The catalog estimate was one row for 100 actual, and the nested loop used the two-row database catalog. These measurements are individual runs of the same queries.
- Independent Codex review found a keyboard routing issue when the Navigator filter retained focus. The regression test first failed because the filter accepted input with Session Monitor open, then passed after the filter yielded to the modal.
- After the keyboard fix, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked`, and `cargo build --workspace --locked` passed. `cargo test --workspace --locked` with an isolated PostgreSQL 16.15 server passed (788 reported cases, including environment-gated cases that return early). Both live cases in `cargo test -p db-wire --locked --test session_monitor_test` ran and passed. The first sandboxed run could not bind the SSH fixture's loopback listener. The full rerun with loopback access passed that test. The rebuilt app stayed up for six seconds with an isolated configuration and emitted no startup diagnostics during that interval.
- Independent Codex re-review approved the full code diff after the keyboard fix. The requested Opus review remains unavailable after the external transfer denial. The automated fixture was stopped and removed. The runnable binary and Cargo build cache were retained.

## Manual desktop acceptance

The user confirmed all four steps passed on 2026-11-15. The checklist covered
Escape with retained Navigator focus, manual refresh and the live blocking chain,
both confirmation paths and their automatic refresh, and restricted and
read-only profiles.

Use a test database and three independent PostgreSQL clients connected to that
same database. In each client, run `SELECT pg_backend_pid();` and note its PID.
Choose an unused two-integer advisory-lock key prefix in the test database. The
examples use `820301`.

| Client | SQL to run |
| --- | --- |
| A | `SELECT pg_advisory_lock(820301, 1);` |
| B | `SELECT pg_advisory_lock(820301, 2);` followed by `SELECT pg_advisory_lock(820301, 1);` |
| C | `SELECT pg_advisory_lock(820301, 2);` |

1. With the Navigator filter field focused, right-click the connected profile
   and open **Session Monitor…**. Verify Escape closes the monitor. Reopen it,
   refresh, select C, and verify `C → B → A`, C's active query, and its lock wait.
2. Select B, open **Cancel query…**, and choose **Go back**. Refresh and verify B
   still waits. Repeat and confirm. Verify B returns SQLSTATE `57014` and remains
   connected, C still waits for B, and the monitor refreshes automatically.
3. Select B, open **Terminate session…**, and choose **Go back**. Verify B stays
   connected. Repeat and confirm. Verify B disconnects, C completes, and A is
   still connected. If the snapshot shows B while the signal is in flight, use
   **Refresh** to confirm B has exited.
4. Open the monitor from a read-only profile and verify that browsing and Refresh
   work while both session actions are disabled. Open from a restricted account
   and verify hidden details get an explicit unavailable state. Close the test
   connections to release their advisory locks.
