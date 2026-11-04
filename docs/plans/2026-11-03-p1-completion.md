# P1 completion

## Scope and authorization

Complete all P1 requirements in `docs/ROADMAP.md`, preserving P1-01 through
P1-04 and implementing the remaining P1-05 through P1-10 workflows. Checkboxes
remain open until their entire scope is implemented and verified.

The user approved necessary public API, backward-compatible configuration and
dependency changes within P1 on 2026-11-03. Verify registry, maintenance, license
and audit status before adding dependencies. Keep secrets in the existing
credential store. Commits were initially unauthorized, so the plan remained
uncommitted during implementation. On 2026-11-04 the user requested commits;
commit this plan separately before the implementation changes. Pushing remains
unauthorized.

CSV import and transfer default to one transaction with full rollback on error.
Batch commit must be an explicit option with partial-commit feedback. Never retry
non-idempotent writes automatically.

## Delivery sequence

1. P1-05: implement dedicated physical sessions for SQLite, MySQL/MariaDB and
   supported ODBC providers; validate isolation from ordinary browsing, temporary
   objects, transactions, savepoints, failure recovery and cancellation. Add
   provider-aware autocommit, isolation and database/schema controls and visible
   session state. Extend procedural/batch parsing with dialect fixtures.
2. P1-06: multi-column sort/filter, column visibility/order, result search and
   record view; qualified FK lookup/navigation; validated multi-cell paste with
   explicit NULL/default semantics. Only enable query-result editing when origin
   and unique-key mapping prove a single writable target.
3. P1-07: bounded text/JSON/XML and binary/LOB editing, file load/save and
   datetime/encoding preferences; preserve exact bytes and NULL distinctions.
4. P1-08: streaming CSV validation/mapping/import, table/query transfer and
   full-source export; progress, cancellation and transaction outcome reporting.
5. P1-09: verified SSH forwarding with deterministic resource ownership; supported
   authentication/client certificates and advanced connection settings;
   connection groups/colors and read-only guards on every write entry point.
6. P1-10: versioned backward-compatible workspace/preferences persistence;
   restore tab identity, layout and unsaved text without executing SQL or
   restoring live transactions; configurable shortcuts/fonts/theme and panel/grid
   settings.

## Evidence and gates

- Read owning code, callers, tests and documentation before each change.
- Add deterministic behavior tests first; use real SQLite and configured provider
  fixtures rather than mocking persistence. Check cancellation and failed writes.
- Update owning guide pages and provider documentation as behavior changes.
- After the last edit, inspect the full diff including untracked files, run
  workspace formatting, Clippy, tests and build, then perform the nine-item
  repository self-review and independent review. Repeat gates after fixes.
- Exercise real application workflows and provider read-back. Distinguish skipped
  live tests and process startup from successful GUI/provider acceptance.
- Preserve the full goal when external credentials, drivers, servers or desktop
  acceptance are missing; record the exact missing evidence instead of closing
  an incomplete milestone.

## Initial source findings

- The working tree was clean at inspection.
- `DatabaseDriver` already exposes `supports_script_session` and
  `open_script_session`; `ScriptSession` and `run_tab_script` provide shared tab
  ownership and cleanup orchestration.
- SQLite currently has one driver-owned connection, runs queries on blocking
  workers and has no script-session implementation. A dedicated session needs
  cancellation checks before queued execution, interruption of active execution,
  and deterministic closure after worker completion. A separate `:memory:`
  connection must not silently replace the user's existing database.
- SSH currently binds a listener without an accept/forward loop; application
  connection construction supplies `ssh_tunnel: None`.
- P1 completion is not yet verified; no milestone is closed by this plan.

## Delivery in progress: SQLite pinned sessions

Added dedicated SQLite tab connections, shared main-database identity (including
unique named in-memory databases), private TEMP state, savepoints and rollback
on closure. Blocking execution stays on workers. Cancellation uses an interrupt
handle plus a progress callback and a closed-session guard. The runner emits
native transaction state after execution, covering outer SAVEPOINT/RELEASE and
SQLite error rollback behavior. Transaction toolbar visibility now follows
session capability independently of PostgreSQL query-plan support.

Evidence so far:

- The initial real SQLite test failed at `supports_script_session()` before the
  implementation.
- `cargo test -p db-wire --test sqlite_session_test --locked`: four tests passed,
  covering main/TEMP isolation, savepoints, committed read-back, rollback on
  finish/abort/owner drop, distinct memory databases, active cancellation and
  successive `run_tab_script` calls.
- Existing `wire_test` suite: seven tests passed before the native transaction
  feedback delta; final workspace verification must cover the latest source.
- `cargo clippy --workspace --all-targets --locked`: passed on the latest source.
- `cargo fmt --all` and `git diff --check`: passed.
- Independent review found transaction-button gating, PostgreSQL-only error
  semantics and standalone savepoint state feedback; these were addressed and
  re-reviewed without further actionable findings.
- The first workspace test attempt overlapped source edits and failed to compile
  against an older trait artifact. It is not a validation result; a clean serial
  rerun is in progress, logging to `/tmp/fastdb-p1-tests.log`.
- Local PostgreSQL and MySQL test containers exist; MySQL maps to localhost
  port 55440 and the column-test PostgreSQL container maps to port 55439.

Still required: final workspace test/build gates, app/GUI acceptance, file-backed
and queued-cancellation regression coverage, a runner-level native-state event
assertion, and the remaining P1-05 through P1-10 requirements. This partial
delivery is not release-ready and closes no milestone.

Final workspace test rerun: sandbox execution failed seven updater tests because
local TCP binding/macOS system-configuration access was denied. Running
`cargo test --workspace --locked` outside the sandbox then passed: 400 harness
passes, zero failures (log `/tmp/fastdb-p1-tests-unsandboxed.log`). Live provider
variables were not configured for this run; early-return provider tests are not
live validation. Build/app acceptance and the remaining coverage above are still
pending.

## Delivery in progress: MySQL pinned sessions

Added a dedicated nonpooled connection worker per MySQL/MariaDB SQL tab, a bounded
command queue, native transaction flags, error-state refresh and terminal-session
discard. Existing streamed-row conversion/count/warning handling is shared with
ordinary queries. Cancellation terminates the active server connection using a
separate same-user connection; all cleanup phases have named bounds and both
connections explicitly disconnect to avoid mysql_async's detached pending-result
drain on Drop. Known connection loss never issues KILL; a state-probe timeout is
treated as an interrupted live operation rather than proof of connection loss.

Five live MySQL tests passed, covering session identity/TEMP/settings/savepoints,
caller-runtime lifetime, active server cancellation with rollback/read-back,
constraint error recovery, implicit DDL commit, connection loss without replay,
successive tab runs and server-session disappearance on owner close. The two
initial tests failed before implementation because script sessions were unsupported.

Independent review found unbounded cancellation-connection cleanup, native state
being overridden in the UI and conflation of state-probe timeout with connection
loss. All were corrected and re-reviewed without further actionable findings.

A workspace run passed with 405 harness passes before the final probe-classification
adjustment. Latest MySQL regression and Clippy reruns are in progress, logging to
`/tmp/fastdb-p1-mysql-regression.log` and `/tmp/fastdb-p1-clippy.log`. The first broader
MySQL regression invocation failed because its default `fastdb_test` database does
not exist; the corrected invocation sets `FASTDB_TEST_MYSQL_DATABASE=mysql`.

Next required work includes provider-aware parameter interception: the current
SQL UI mistakes native MySQL `@name` variables for bind parameters. Backend live
session tests do not prove that UI path. Separate MariaDB validation, final build/
workspace/GUI gates, ODBC sessions and the rest of P1 remain open. No milestone
is closed by this delivery.

## Delivery in progress: native MySQL variables through parameter prompts

Execute, Explain preflight and parameter preview now use the same provider-aware
interpretation of `@name`. MySQL/MariaDB preserve native variables; other
providers retain existing named-parameter behavior. `:name`, `?` and existing
generic parameter APIs remain available. The modal captures its provider policy
at creation so preview does not change when another connection becomes active.

Independent review found that filtering generic scanner output left a false `$1`
parameter inside legal `@counter$1`. A regression reproduced that exact output
before the lexical fix. The scanner now consumes whole native variable names,
including numeric starts, `.`, `$`, UTF-8 bytes and quoted forms. Syntax was
checked against the official MySQL 8.4 user-variable documentation.

Validation so far:

- `cargo test -p sql-intel --locked`: 32 tests passed.
- `FASTDB_TEST_MYSQL_PORT=55440 cargo test -p fastdb-app mysql_modal --locked`:
  two tests passed, including real modal preview -> script preflight -> tab
  runner -> MySQL execution -> result read-back (`42`) across two runs using
  `@fastdb_counter$1` and typed integer parameters.
- `cargo fmt --all --check`, workspace/all-targets Clippy and app build passed.
- Independent re-review found no further actionable issue in this delta.
- Final workspace tests are running. The built app launched with an isolated
  temporary config and stayed running without startup errors. Native GUI
  inspection is blocked because the Mac is locked; the user was asked to unlock
  it while independent code work continues.

Self-review of this delta:

1. Correctness: tests assert exact native-variable preservation, explicit parameter
   replacement, NULL, missing bindings, quoted sigils and live cross-run values.
2. Performance: a linear scanner runs on explicit execution/preview, with no new
   database queries or work in rendering.
3. Security: generated values retain existing typed validation; no credentials or
   SQL text are logged and no permissions change.
4. Robustness: old public helpers preserve behavior, no dependencies or persisted
   formats change, and the modal captures stable provider policy.
5. Syntax/naming: formatting, Clippy and build pass with existing Rust conventions.
6. Modern APIs: ordinary stable Rust APIs; native variable grammar checked against
   the vendor documentation rather than guessed.
7. Business clarity: native MySQL/MariaDB @variables follow server semantics;
   client prompts use :names or question marks, documented in the SQL guide.
8. Coverage: library, modal and real provider integration pass; desktop routing
   and separate MariaDB runtime acceptance remain unverified.
9. Design: one explicit policy shared by detection and substitution avoids
   separate scanners and keeps provider mapping in the app.

Remaining P1 work includes ODBC sessions, session controls/vendor script grammar,
grid/value/import/connection/preferences workflows and complete acceptance. Also
verify provider-specific string parameter round-trips under supported SQL modes;
the integer live test does not prove that wider behavior. No milestone is closed.

Final validation for this delta: `cargo build -p fastdb-app --locked` and
`cargo test --workspace --locked` passed outside the sandbox, with 409 harness
passes and zero failures. Live-provider variables were unset for the full-suite
run; the separate MySQL modal integration above actually exercised MySQL.
`git diff --check` passed. The isolated app startup process was stopped with
Ctrl+C after remaining alive without errors. GUI acceptance remains pending
because the desktop was locked. No commits or pushes were made.

## Delivery in progress: SQL tab isolation controls

Added an Isolation toolbar action using a native choice dialog with generated
SQL preview. It captures the tab ID/provider/driver, revalidates them after the
dialog, rejects active/unknown transactions and applies settings plus native
read-back on the pinned session. PostgreSQL offers RC/RR/Serializable,
MySQL/MariaDB also offer RU, and SQLite offers Serializable only. Settings have
a fixed 15-second per-statement deadline; ordinary SQL/plan timeout choices are
preserved. Closing/replacing a session does not reapply settings automatically.

Test-first capability/guard tests initially failed to compile before implementation.
Three owning-app tests then passed with real SQLite plus local PostgreSQL (55439)
and MySQL (55440), checking every offered level and unchanged browsing-session
settings. A GPUI test-macro glob collision was corrected with explicit imports.
Independent review found a receiver-name mistake during method extraction; it
was corrected. Latest workspace/all-target Clippy and diff whitespace checks
pass, and independent re-review found no further actionable defects. Final build
and full-suite validation follow this entry. Desktop dialog and separate MariaDB
runtime acceptance are still pending.

Self-review:
1. Correctness: supported choices, idle guards, exact native values and browsing
   isolation have tests; delayed choices recheck tab and physical-driver identity.
2. Performance: two fixed setting/read-back statements per explicit action; no
   result/table scans, new background queues or render-path database work.
3. Security: SQL comes only from fixed enum choices, without user interpolation;
   no credentials/logging/permission changes.
4. Robustness: per-statement deadlines and existing cancellation/cleanup ownership;
   no new dependencies or persistence format; session settings stay session-local.
5. Syntax/naming: formatting and workspace Clippy pass after receiver/import fixes.
6. Modern APIs: existing GPUI prompt/task APIs and vendor-documented isolation SQL.
7. Business clarity: settings are future-transaction defaults and require idle state;
   autocommit re-enable behavior was asked separately and is not guessed here.
8. Coverage: real SQLite/PG/MySQL read-back tests pass; native dialog routing and
   MariaDB runtime remain unverified, so P1 completion is not claimed.
9. Design: provider SQL, prompt orchestration and guarded application are separate;
   ordinary execution reuses its existing timeout source.

Autocommit, active database/schema selection, ODBC sessions and vendor syntax
remain part of P1-05; P1-06 through P1-10 remain open. No milestone is closed.

Final isolation validation: app build, workspace/all-target Clippy, formatting and
diff checks passed. `cargo test --workspace --locked` with local PG/MySQL PORT and
DATABASE variables passed with 412 harness passes and zero failures (log:
`/tmp/fastdb-p1-tests-live-workspace.log`). This run exercised the PG/MySQL session
fixtures; ODBC/vendor/keychain environment checks still skip when unconfigured.
The latest app remained running without startup errors using a temporary config
and was stopped with Ctrl+C. Native-dialog acceptance remains pending because
the Mac was locked. No commits or pushes were made.

## Delivery in progress: loaded-result Find (P1-06 subset)

Added row-major literal, case-sensitive Next/Previous with wrap for table and
script result grids, matching the existing SQL Find convention. The search
includes displayed staged edits/new rows and rows marked for deletion, skips
NULL/unset defaults, and never collects a match list or full result copy. A
single cancellable background worker per workspace reads paged/spilled values;
errors remain explicit. Input is bounded to 4,096 UTF-8 bytes. Results apply only
to the original tab/pager with unchanged rows/edits and no active cell editor.
Close/Quit/query edits cancel pending matches. Normal staged edits are preserved.

Independent review found that programmatic match reveal could trigger bottom
auto-fetch, and the initial Find control was active without a pager. The control
is now disabled without results. Opening Find and revealing a match pause
auto-fetch on that grid; the pause survives dialog closure, clears on manual
vertical wheel/scrollbar navigation or pager replacement, and follows a grid
when script results switch. Deferred fetches recheck pause, query state, tab and
pager identity before execution. The reveal/auto-fetch regression failed before
the pause line and passed after the fix. Re-review found no further defects.

Validation so far: 22 ui-grid tests and three app result-search tests passed,
covering forward/backward/wrap, Unicode/case/NULL, staged values, malformed row
positions, real spill-page reads, cancellation, stale identities/edits, worker
cleanup and auto-fetch suppression. Formatting, diff checks and full workspace
Clippy passed without warnings. Final full build/tests follow this entry.
The Mac was rechecked and is still locked; native GUI acceptance remains pending.

Self-review:
1. Correctness: row/cell mapping and stale-result guards have deterministic tests;
   NULL is distinct from literal text and search never mutates a changeset.
2. Performance: one worker/snapshot at a time, one page borrow per cell and no
   match/result collection; disk access stays off render and input stays bounded.
3. Security: needle text is never SQL, no new database/auth calls or content logs.
4. Robustness: drop guards cancel work/release capacity, spill errors propagate,
   and invalid staged positions return errors instead of panicking.
5. Syntax/naming: surrounding Rust/GPUI patterns, formatter and Clippy pass.
6. Modern APIs: reuses installed GPUI scrolling/input/task APIs without new deps.
7. Business clarity: literal/case-sensitive loaded-value scope follows existing
   Find; byte limit, NULL behavior and automatic-fetch pause are documented.
8. Coverage: real pagers/spill files and state tests pass; native dialog/clipboard/
   manual-scroll routing and forced spill-read failures remain unverified.
9. Design: scanner, job ownership, stale-result validation and cell reveal are
   separate; the shared Data panel exposes the same action for scripts/tables.

P1-06 remains unchecked: the other grid workflows and GUI acceptance are still
required. All remaining P1-05 and P1-07 through P1-10 scope is preserved.

The build/full-suite attempt after result Find was initially rejected because the
automatic approval reviewer hit its usage limit. After the stated retry time
passed, the same reviewed command ran successfully: build and workspace tests
with local PG/MySQL fixtures passed, 420 harness passes and zero failures. The
rejection was not bypassed. Find also now blocks auto-fetch while its dialog is
open, even if a global wheel capture clears the grid hold, and close paths
reassert the hold before discarding the dialog. This follow-up was reviewed.

## Delivery in progress: native MySQL/MariaDB autocommit controls

On 2026-11-04 the user chose to block enabling autocommit until Commit/Rollback,
rather than prompting to commit existing work. The native control now previews
Off/On, omits On while transaction state is active/unknown, rechecks that rule
after the prompt, and executes fixed SET/read-back statements on the same SQL
tab session. Disabling can retain existing transactions. The shared settings
dispatcher preserves tab/provider/driver identity guards and the 15s deadline.
Closing a physical session leaves the next connection to use server defaults.

Two app tests passed against real MySQL: guards reject enabling for active/
unknown state; Off/On read back 0/1; disabling during an explicit transaction
preserves its write; manual INSERT without BEGIN opens a transaction; blocked
enabling leaves pending work for explicit rollback; rollback reads back zero;
On commits subsequent work normally; browsing-session settings remain unchanged.
The initial test failed before the new planning helper existed. Independent
review requested implicit-transaction coverage; it was added and re-reviewed
without remaining findings. Final workspace validation follows this entry.

Self-review of the native control:
1. Correctness: enabling requires known idle/closed state at dispatch; native
   read-back and explicit/implicit transaction behavior have real-provider tests.
2. Performance: two scalar statements per explicit setting; no catalog/table
   query or index work in the production control, so no query plan is applicable.
3. Security: SQL is generated from a bool/enum, without user SQL interpolation,
   credentials or content logs; no auth/permission changes.
4. Robustness: existing single-operation ownership, per-statement timeout and
   deterministic session cleanup; no dependencies/config formats introduced.
5. Syntax/naming: existing Rust/GPUI patterns; formatter/check pass, final Clippy
   and build/test gates are run after the latest edit.
6. Modern APIs: native session variables and existing GPUI prompts/tasks.
7. Business clarity: user explicitly selected blocking On for unfinished work;
   no automatic Commit or Rollback is introduced by this control.
8. Coverage: actual MySQL values/work/rollback are asserted; native GUI and
   separate MariaDB runtime acceptance remain unverified.
9. Design: isolation/autocommit share guarded dispatch, while provider planning
   and prompt presentation remain separate.

Next implementation work still includes PostgreSQL/SQLite client-managed
autocommit, active database/schema context, ODBC sessions and vendor syntax,
the remaining P1-06 workflows and P1-07 through P1-10. No milestone is closed.

Final validation after native autocommit and Find dialog-lifecycle changes:
formatting, diff checks, workspace/all-target Clippy, app build and workspace
tests with local PostgreSQL/MySQL fixtures passed: 422 harness passes, zero
failures (`/tmp/fastdb-p1-tests-live-workspace.log`). Unconfigured ODBC/vendor/
keychain checks are still not validated support. The latest binary remained
running without startup errors and was stopped with Ctrl+C.

Desktop availability changed on 2026-11-04: the Mac is unlocked, but selecting
the test app through Computer Use returned "Computer Use permissions are not
granted". Native interaction is therefore still unverified; the user was asked
to grant permission or defer GUI acceptance. A temporary isolated test bundle
was prepared at
`/var/folders/vy/mbk70rt11kg4xqq65ly8hq2w0000gn/T/fastdb-p1-ui-n22nnxac/FastDB P1 Smoke.app`;
its binary SHA-256 matches the current build
(`6fc23dffbdb2cd704ca9941fda8ac7db3b27808037a18f0296b51e986b254f9d`).
It uses a temporary config path via Info.plist LSEnvironment; successful native
launch/interaction through that bundle has not been verified. No installed app
was replaced and no commits or pushes were made.

## Next delivery: PostgreSQL/SQLite client-managed autocommit

The user deferred native GUI acceptance on 2026-11-04; retain it as pending,
continue implementation and automated/live-provider verification.

Technical convention for PostgreSQL/SQLite: manual mode belongs to the open SQL
tab. It lazily inserts BEGIN before ordinary user SQL when the acknowledged
transaction state is idle, including after Commit/Rollback and session closure.
Explicit transaction controls are preserved. Failed/unknown states must not let
writes escape into autocommit; recovery controls remain usable. Mode selection
does not execute SQL immediately and no SQL is replayed on disconnect/restart.
Settings actions stay outside this wrapping; Explain/Analyze participate in the
same manual transaction. Native MySQL/MariaDB behavior stays server-managed.
Enabling still requires idle/closed state, per the user's confirmed policy.

Test planning boundaries first, then real SQLite/PG rollback/commit, cross-run
state, savepoints, error recovery, plans and cancellation. Keep lazy BEGIN visible
in execution feedback without attempting to parse it as a query plan.


### Client-managed autocommit evidence and review

PostgreSQL/SQLite controls are wired. Six transaction-mode tests pass, including
real SQLite and PostgreSQL (port 55439): cross-run writes, savepoint failure
recovery, explicit boundaries, browsing isolation and PostgreSQL Analyze in the
same transaction. A separate tab-state test verifies blocked On preserves mode
and transaction state. SQLite uses native state immediately before each ordinary
statement; settings bypass wrapping. The mode remains local to an open tab.

Independent review found a new-feature boundary defect: a standalone SQLite
SAVEPOINT opened while On, followed by Off and `RELEASE outermost; INSERT;
ROLLBACK`, let the INSERT autocommit. The regression failed with "cannot rollback
- no transaction is active" before the runtime native-state check and passes
after it. HEAD has no client manual mode, so this new-feature regression cannot
run meaningfully on the original base. Re-review found no remaining defect in
this delta. Unknown native state is separately tested to reject ordinary work
while allowing explicit Rollback.

Additional existing-bug reproduction: Darwin arm64, Rust 1.99.1, PostgreSQL
16.15, base ca2905dac12460ac9205235e94bd2abae3a643bd. In an isolated archive of
HEAD, create an isolated table, Run BEGIN/INSERT, pre-cancel the next Run, then
Run COMMIT. Expected count 0 (Cancel closes/rolls back per the SQL editor guide),
actual count 1. The same automated regression failed on the working tree before
the fix. Logs: `/tmp/fastdb-p1-pre-cancel-base.log` and
`/tmp/fastdb-p1-pre-cancel-current-before.log`. Existing sessions now enter normal
bounded cleanup even when the run was already cancelled; no new session is
opened for a pre-cancelled run. No user SQL is replayed. Regression verification
and independent review follow; do not treat this entry as a final gate result.

Self-review for client mode and cancellation:
1. Correctness: state guards preserve unfinished work; native SQLite state handles
   outer-savepoint closure, and the cancellation regression checks persisted rows.
2. Performance: scalar native state checks and at most one BEGIN per boundary;
   no new production data query/index or query-plan analysis applies.
3. Security: fixed BEGIN/transaction controls; no secrets, auth changes or
   untrusted identifier interpolation in production paths.
4. Robustness: injected statements use existing deadline/cancel/cleanup paths;
   no dependency/config change; early cancellation retains cleanup ownership.
5. Syntax/naming: formatter and full workspace build/Clippy are final gates.
6. Modern APIs: existing Rust/Tokio/GPUI and SQLite native state APIs are reused.
7. Business clarity: user explicitly requires Commit/Rollback before enabling;
   lazy BEGIN and open-tab lifetime are documented technical conventions.
8. Coverage: real-provider tests assert stored values and transaction boundaries;
   native GUI is deferred by the user, so UI acceptance remains incomplete.
9. Design: mode planning, guarded UI settings and provider execution remain
   separate; injected BEGIN shares the ordinary runner rather than duplicating it.

The temporary GUI bundle documented above predates these changes and is stale.
No installed app was replaced. P1-05 and P1-06 remain partial; P1-07 through P1-10
remain open. Do not mark the full P1 goal complete.


## Confirmed database/schema context policy (2026-11-04)

The user selected this policy:
- Change schema on the current SQL tab only when its transaction state is known
  idle. Active, failed and unknown states require explicit Commit/Rollback first.
- Changing database opens a new SQL tab with a new physical session. Preserve the
  original tab, transaction, temporary objects and session settings.
- Native GUI acceptance remains deferred by the user.

Implementation is pending. First cover provider-specific context planning and
idle guards, then real-provider schema read-back and isolation between the old
and new database tabs. Revalidate tab identity and connection ownership after
asynchronous selection. Do not run SQL implicitly on a target database, replay
SQL, or mutate a saved connection's database merely to create the new tab.
Keep provider limitations explicit, including SQLite's lack of a mutable default
schema and PostgreSQL's requirement for a new connection to another database.

This update records the decision only; it does not close the P1-05 context item.


## Pre-commit validation status (2026-11-04)

The final implementation validation passed `cargo fmt --all --check`,
`git diff --check`, `cargo clippy --workspace --all-targets --locked`,
`cargo build -p fastdb-app --locked`, and `cargo test --workspace --locked`
with local PostgreSQL/MySQL PORT and DATABASE variables: 431 harness passes,
zero failures. Unconfigured vendor/ODBC/keychain tests are not live acceptance.
The early-cancel PostgreSQL regression now passes with persisted count 0;
independent review of that fix and the SQLite native manual-mode queue found
no actionable findings. Native GUI acceptance remains deferred by the user.
A startup smoke of the latest executable stayed alive until Ctrl+C, but emitted
macOS XPC sandbox errors, so it is not evidence of a successful GUI workflow.

The user requested commits on 2026-11-04. Preserve separate logical commits and
keep P1 milestones open. The current database/schema decision is recorded only;
its implementation remains pending. No push is requested.
