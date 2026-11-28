# FastDB Full Feature Acceptance Plan

> **For agentic workers:** REQUIRED SUB-SKILL: subagent-driven-strict or executing-plans-strict. Tasks are numbered and steps use `- [ ]`.

**Goal:** Verify all implemented FastDB feature families and record the supported provider, platform, and verification level for each result.
**Architecture:** Run existing workspace and live-driver suites against disposable fixtures. Exercise the shipped GUI with automated desktop input and independently verify persisted effects. Verify native distribution, updates, and performance on their real targets.
**Tech stack:** Rust, Cargo, GPUI, PostgreSQL, MySQL, MariaDB, SQLite, vendor ODBC, Linux, macOS, Minisign.
**Branch:** `chore/full-feature-acceptance` for an authorized execution branch. Capture the exact commit at execution time.

## Confirmed Requirements

- Execution scope as of 2026-11-23 is Linux. macOS checks are deferred for this pass. Record X11 and native Wayland evidence separately.
- Track observed results and remaining gaps in the [execution record](../testing/2026-11-22-full-feature-acceptance-results.md). Base-candidate evidence and fixed-build retests have separate provenance.

- Test all implemented functionality, including shipped GUI paths, driver-only capabilities, native packages, and updater paths. The user requested a plan now.
- Automate desktop interaction end to end, including editing, Save/Cancel, and independent read-back. The user authorized GPT-6 for this plan and requested automated desktop interaction in the preceding smoke run.
- Preserve the supported scope of each feature. PostgreSQL, MySQL, and SQLite are enabled in the desktop picker. MariaDB, SQL Server, Oracle, and Db2 driver paths are tested independently, and the GUI availability gate remains in scope.
- Treat the previous L03/L04 keyboard smoke, P2-03 desktop pass, and P2-05 desktop pass as evidence tied to their runs. Recheck the same paths on the candidate before accepting it.

## Decision Log

| Decision | Reason and owner | Ticket status |
| --- | --- | --- |
| Full implemented scope | User requested coverage of every delivered feature. The plan follows the provider capability declarations and shipped platform targets. | Not posted. No ticket or PR was requested. |
| Automated desktop actions | User requested automation after a pointer-input limitation interrupted the first smoke run. | Not posted. No ticket or PR was requested. |
| GPT-6 planning | User selected GPT-6 for this plan in the session. | Not posted. No ticket or PR was requested. |

## Scope Map

Every reachable implemented action must produce its documented result, error, or cancellation on its owning connection, tab, or file. A database or filesystem change requires independent read-back.

| Paths and capability | Coverage |
| --- | --- |
| [CI](../../.github/workflows/ci.yml), workspace crates, and [driver integration tests](../../crates/db-wire/tests/) | Tasks 1–2 cover builds, unit/integration suites, enabled providers, native driver-only operations, TLS, and SSH. |
| [Connections](../guide/connections.md), [keyboard shortcuts](../guide/shortcuts.md), workspace startup, settings, dialogs, and lifecycle | Task 3 covers P0-05/P0-08, P1-09/P1-10, profile and credential state, and multi-connection ownership. |
| [Navigator and table tabs](../guide/navigator-and-tables.md), Properties, DDL, Diagram, schema editing and search | Tasks 4–5 cover P0-01–04, P0-06–07, P1-01–02, P1-06–07, and the implemented P2-04/P2-05 scope. |
| [SQL editor](../guide/sql-editor.md), scripts, transaction controls, logs, results, query plans | Task 6 covers P1-03–06 and P2-05, including saved query-plan comparison. |
| [Transfer and export checklist](../testing/0.2.0-postgres-manual-acceptance.md#9-import-loadedfull-export-and-transfer--p1-08), [native backup checklist](../testing/p2-02-manual-acceptance.md) | Task 7 covers P1-08, P2-02, and implemented P2-07 formats. |
| [PostgreSQL administration](../guide/navigator-and-tables.md#postgresql-administration) and [Session Monitor](../guide/connections.md#postgresql-session-monitor) | Task 8 covers P2-01/P2-03 and native authorization and read-back. |
| [Release workflow](../../.github/workflows/release.yml), [Software updates](../guide/updates.md), and [build scripts](../development/building.md#packaging) | Task 9 covers Linux and macOS packaging, install, startup, restart, and signed update handling. |
| [Benchmark protocol](../../benchmarks/README.md), [PostgreSQL measurements](../../benchmarks/postgres/README.md), and [P2-10 evidence](../../benchmarks/VALIDATION.md#final-linux-acceptance--2026-11-17) | Task 10 covers release-build performance, live data measurements, and desktop rendering. |
| [Roadmap](../ROADMAP.md) and [known gaps](../guide/not-built-yet.md) | Task 11 reconciles evidence. Schema/data synchronization, charts, task scheduling, virtual keys, schema-aware XML editing, and other documented unimplemented capabilities are outside positive acceptance. Check disabled/unsupported states where exposed. Windows is outside the current target list in [rust-toolchain.toml](../../rust-toolchain.toml). |

## Existing Data & States

| Existing state | Expected behavior and coverage |
| --- | --- |
| Existing profiles, including legacy plaintext and a saved disabled-provider profile | An isolated profile migrates eligible credentials to the selected store, preserves saved metadata, and refuses Test/Connect for disabled presets. Handle missing credentials with the documented presentation and retry path. Task 3. |
| Same relation name on A and B, stale completion, and a dirty or actively edited cell | Operations and drafts stay attached to their owning session. Departure guards preserve the draft, and an independent client sees only committed bytes. Task 3. |
| NULL, empty text, quoted Unicode, decimal, binary, and timezone-aware values | Verify the documented type and encoding boundaries, including CSV NULL/empty behavior, exact native bytes, and unsupported or oversized data. Tasks 5 and 7. |
| Open SQL tabs, unsaved text, transaction in progress, and a save failure | Restore layout and exact script text without replay. Close/disconnect rolls back the session. With verified focus and an unwritable isolated script directory, the failure keeps FastDB open and retry saves the intended bytes. Tasks 3 and 6. |
| Existing destination file, populated restore target, and incomplete transfer | Preserve the existing file or data and show refusal or the documented partial-job state. Read back after failure and cancellation. Task 7. |
| Existing installed app and a signed, invalid, missing, or replaced installer | A valid artifact reaches the native installer. Each verification failure leaves the installed app intact and exposes the documented recovery action. Task 9. |

## Global Constraints

- “Drivers stream rows into `mem-engine` as batches arrive; never collect a full result set in memory.”
- “Tests that need a live database must skip cleanly when the `FASTDB_TEST_*` variables are unset (CI sets `FASTDB_REQUIRE_DB_TESTS=1` to make them mandatory).”
- “Commit or push only when asked.” Keep code, comments, and records in English.
- The user's request for end-to-end automation authorizes input to the isolated test app. Follow the desktop tool's permissions. Keep capture scoped to the test environment or window. Use local test-only credentials and redact evidence before retention.
- Use disposable SQL instances and app profiles. Avoid production services, real saved connections, and the user's installed app. Run the documented release gates separately from this verification plan. If a defect needs an implementation change, invoke `bugfix-strict` before editing.
- “Clean up temporary debug/build artifacts after verification to avoid SSD growth. Keep the current runnable binary and reusable build dependencies.”

## Technical Defaults

- Desktop matrix: run applicable cases against PostgreSQL, MySQL and SQLite on Linux x86_64 and macOS Apple Silicon. On Linux record X11 and Wayland outcomes separately. Exercise focus, dialogs, clipboard and scaling on each target. Use macOS for app-menu actions and Linux for its documented keyboard, window and update-badge paths. PostgreSQL remains the existing release-gate provider under [the ordered runbook](../testing/0.2.0-postgres-manual-acceptance.md#release-decision-and-result-template).
- Use the CI versions PostgreSQL 16 and MySQL 8.4, a temporary SQLite file, and the server/client combinations prescribed in the provider and performance runbooks for native-specific scenarios. Keep transport and vendor test fixtures on isolated instances. Check GUI behavior through product controls and database state through a separate client.
- A result row is `case | commit/build | OS/backend/scale | provider/server/client | expected | actual | status | evidence or blocker`. Record separately for each applicable combination. Use `Pass`, `Fail`, `Blocked`, or `Pending` per the [case record](../WORKFLOW_ACCEPTANCE.md#desktop-case-record). Record a capability exclusion with its guide reference.
- Start with Tasks 1–2 and the L03/L04 cases from Task 3. Independent feature groups can run concurrently with distinct app profiles, databases, and report files. Serialize GUI actions per display and benchmark runs per host. Start each mutating group from a fresh seed or the [documented PostgreSQL reset](../testing/0.2.0-postgres-manual-acceptance.md#reset-and-repeat-from-the-beginning), and verify baseline rows before reusing a fixture.

## Risks & Rollback

- Workspace tests can skip live fixtures or contend for the fixed workflow database names. Require the documented fixture flags and give each concurrent live run its own server. Stop and remove only that run's resources.
- Desktop focus, pointer access, or native prompts can block otherwise healthy action paths. Prove input and capture before data mutations. Record the exact blocked case if a platform capability or permission is missing and continue with independent coverage.
- Fault injection, restore, native installation, or cold-cache measurement can affect shared state. Limit mutation to disposable profiles, targets, and install snapshots. Restore permission changes immediately, roll back the VM or app copy, and use a dedicated benchmark host for OS-level operations.
- A prior acceptance pass or benchmark belongs to its original build and environment. Record repeat results with candidate provenance and compare only compatible reports. This plan changes no product interfaces or persisted formats.

### Task 1: Freeze the candidate and prove desktop automation

**Files:** [CLAUDE.md](../../CLAUDE.md), [build instructions](../development/building.md), [desktop protocol](../WORKFLOW_ACCEPTANCE.md#repeatable-desktop-protocol), [fixture](../testing/0.2.0-postgres-fixture.sql).
**Interfaces:** Consumes the requested candidate checkout and available test machines. Produces the candidate identity, profile and fixture inventory, and one automation preflight row per desktop backend.
**Test level:** Build and desktop E2E preflight.
**RED:** n/a — existing workflows and test setup.

- [ ] Record commit, worktree changes, binary path/hash, FastDB version, OS/architecture/desktop/scale, and provider/client versions. Build with `cargo build --locked -p fastdb-app` and launch with `cargo run -p fastdb-app --locked` using `FASTDB_CONFIG_DIR` under a disposable test directory. Freeze the functional candidate before switching to the release build in Tasks 9–10.
- [ ] Create independent A/B PostgreSQL databases using the [runbook preparation](../testing/0.2.0-postgres-manual-acceptance.md#recording-and-preparation). Confirm baseline fixture counts `1000, 3, 1, 2, 0, 0, 0`. Create MySQL A/B schemas and SQLite A/B files separately. Adapt the runbook's table/row roles to each provider using its [native types](../guide/type-handling.md) and the [workflow fixture cases](../../crates/db-wire/tests/workflow_test.rs). Seed matching object names with different A/B sentinel values, a primary key and supported FK/constraint cases, and NULL, empty, Unicode, decimal and binary samples. Independently record exact baseline rows, native columns, and per-provider expected outcomes before running the equivalent GUI cases. Prepare empty destination databases/files and restricted/read-only profiles.
  The [fixture read-back](../testing/2026-11-22-full-feature-acceptance-results.md#task-1-linux-fixture-read-back) verified the A/B data and PostgreSQL role permissions. The matching GUI profile matrix remains open.
- [ ] Use a dedicated Linux X11 or Wayland desktop/VM with host-console input confined to the guest, or an equivalent dedicated macOS desktop with permitted native input and capture. Record the input channel and permission result for each target. Use a guest console on a fresh VM if the host session cannot deliver reliable pointer input. Record affected GUI cases as Blocked if neither controlled route is available.
- [ ] In a seeded disposable profile on each desktop, connect the fixture by pointer, click **+ SQL**, enter `select 42;`, close it with the shortcut, verify exact on-disk bytes, then reopen it. Execute the reopened statement and wait for a single result row containing `42`. Open the native export destination dialog and Cancel per X05. Confirm no file was created. On macOS try native menu Quit, relaunch, then use window close. On Linux exercise the Quit shortcut and window close in separate launches. If any input or observation fails, record the unmet prerequisite before attempting GUI mutations.
- [x] Preserve the successful keyboard-only L03/L04 evidence from [the post-merge smoke](../WORKFLOW_ACCEPTANCE.md#postgresql-desktop-smoke-2026-11-22) and record what the new automation preflight can exercise. Clean up the preflight profile after capturing the observable outcomes. The [Linux X11 follow-up](../WORKFLOW_ACCEPTANCE.md#postgresql-desktop-smoke-follow-up-2026-11-23) records pointer, keyboard, window close and independent read-back.

### Task 2: Run workspace, live driver, and transport suites

**Files:** [CI jobs](../../.github/workflows/ci.yml), [test guide](../development/testing.md), [provider setup](../PROVIDERS.md#live-tests), [workflow test](../../crates/db-wire/tests/workflow_test.rs), [vendor tests](../../crates/db-wire/tests/provider_test.rs), [native plan tests](../../crates/db-wire/tests/query_plan_test.rs).
**Interfaces:** Consumes the candidate and configured database endpoints. Produces build/check results, suite logs, and one executed or blocked result per provider and transport.
**Test level:** Unit and integration against real disposable servers, SQLite files, SSH and TLS fixtures.
**RED:** n/a — run the existing suites and their success, rejection, and rollback assertions.

- [x] Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo check --workspace --all-targets --locked`, `cargo build --workspace --all-targets --locked` and `cargo test --workspace --locked -- --nocapture` on Linux. Formatting, Clippy, check, build and workspace tests passed on the fixed build. Loopback/temporary spill access and fixture skips are recorded in the results. macOS is deferred by scope.
- [x] Configure the PostgreSQL and MySQL fixture variables from CI and run `FASTDB_REQUIRE_DB_TESTS=1 cargo test --workspace --locked -- --nocapture`. The fixed build passed 836 harness cases. PostgreSQL 16.16, MySQL 8.4.13 and SQLite workflow tests all ran, covering independent read-back, CSV byte comparisons, SQL replay, cancel and rollback.
- [x] Verify PostgreSQL/MySQL TLS and SSH and the Linux credential store. The TLS script passed six direct TLS and two SSH-over-TLS cases. The split SSH rerun passed three live cases and five offline cases using the existing test binaries. The native keychain test passed with `FASTDB_TEST_KEYCHAIN=1` on an isolated D-Bus session.
- [x] Verify driver-only PostgreSQL/SQLite definition search with `catalog_test` and MySQL with `mysql_test`. The required live workspace run and the targeted retest each passed all five cases, covering native source matches, literal handling, scope, and SQLite URI variants.
- [x] Configure `FASTDB_TEST_MARIADB_*` and run the `query_plan_test` MariaDB cases with `FASTDB_REQUIRE_MARIADB_TESTS=1`. Both passed against MariaDB 11.4.13, including native plan metrics, physical session identity, and ordinary/generated metadata. MariaDB-configured probes passed tab lifecycle, native cancellation, [direct TLS](../testing/2026-11-22-full-feature-acceptance-results.md#mariadb-configured-sessions-and-direct-tls) including client identity, and [SSH with TLS](../testing/2026-11-22-full-feature-acceptance-results.md#mariadb-configured-ssh-and-tls) including Cancel and new-connection recovery after losing the tunnel.
- [ ] With installed vendor clients and owned test servers, configure the documented ODBC variables and run `cargo test --locked -p db-wire --test provider_test -- --nocapture`. Use `FASTDB_REQUIRE_PROVIDER_TESTS=1` when all three are provisioned and `FASTDB_TEST_PROVIDER_WRITES=1` only against writable disposable schemas. Confirm executed ODBC reads, metadata, failed-query recovery, opt-in writes/session rollback and SQL Server's estimated-plan assertions. Use `cargo test --locked -p sql-intel vendor_script -- --nocapture` for vendor directive parsing. Carry separate live rows for paging, cancel, transport and batch execution. Mark them Blocked where the suite and available test runner provide no corresponding native probe, with the exact client, fixture or entry point missing.
  The four directive parser cases passed. Live ODBC is recorded as Blocked for SQL Server, Oracle and Db2 because this Linux run lacks the registered drivers and disposable servers.

### Task 3: Verify connections, workspace lifecycle, and departure guards

**Files:** [runbook](../testing/0.2.0-postgres-manual-acceptance.md) C01–C04, L01–L04, R01–R05, W01/W07, [keyboard dialog plan](2026-11-19-keyboard-dialog-routing.md), [connections](../guide/connections.md), [save gate](../../crates/app/src/workspace/script_saves.rs).
**Interfaces:** Consumes isolated A/B profiles, native credential store, and desktop preflight. Produces profiles and ownership/lifecycle case rows, saved-script byte checks, and independent database reads.
**Test level:** Desktop E2E plus native credential and SQL/file read-back.
**RED:** n/a — existing behavior and explicit denial/retry cases.

- [ ] Run C01–C04 and W07. Verify enabled/disabled presets and existing disabled saved profiles, logo appearance, fixed tabbed dialog and visible footer, wrong credentials and closed port, names/groups/colors, timeout, read-only and application-name settings, and persisted reconnect. Exercise both legacy credential migration and local-development credentials in separate profiles.
  The [MySQL and SQLite retest](../testing/2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-connection-profiles) covers reconnect, file and in-memory isolation, read-only guards, group and color, timeout and visible-field navigation.
- [ ] Run L01–L04 in order and complete the pending [smoke subcases](../WORKFLOW_ACCEPTANCE.md#postgresql-desktop-smoke-2026-11-22). With a staged value or active editor, automate Refresh, Criteria/history, Close, disconnect, delete profile, same-profile reconnect and Quit. Check draft ownership, no implicit write, unrelated clean-tab close and column-draft Save/Discard/Cancel. Click grid **Cancel**, verify the original value and retry departure successfully. Compare A/B values using a separate client. Run a separate valid grid **Save** and verify only the intended row changes.
- [x] Exercise immediate SQL tab close, shortcut Quit and window close from the table, editor and connection dialog per L04. Establish that test text reached the editor before simulating a save failure in the isolated profile. Verify visible failure feedback, unchanged saved bytes, retained draft, then restore permissions and verify retry writes exact text and permits departure. See the Linux X11 follow-up. Native menu Quit and About on macOS are deferred by scope.
- [ ] Run R01–R04 and W01. Verify theme, zoom, sidebar, layout and restart, toolbar/dropdown placement, draggable title and maximize/restore, and scroll in a narrow window. Verify reopened scripts and table identity, R02 rollback with no replay, and credential warning/retry behavior. Exercise the reachable routes in the keyboard dialog plan with focus on navigator and editor. Task 9 owns the packaged R05 run.

### Task 4: Verify Navigator, Properties, DDL, and ER diagrams

**Files:** [runbook](../testing/0.2.0-postgres-manual-acceptance.md) M01–M06, D01–D04, W02, [navigator guide](../guide/navigator-and-tables.md), [P2-05 evidence](2026-11-12-p2-05-completion.md#user-gui-acceptance--2026-11-13).
**Interfaces:** Consumes connected/stale/restricted profiles and matching A/B catalogs. Produces navigator and metadata case rows, catalog read-backs, and a parsed SVG export result.
**Test level:** Desktop E2E and native catalog read-back.
**RED:** n/a — existing behavior, including capability and stale-draft refusals.

- [ ] Run W02 and navigator context actions. Test filtering in each mode, relation folders, favorites, reveal active object and refresh ownership, Shift+click range and Escape, and the documented clicked-item behavior for context actions. Verify icons and labels at normal and high scale. Exercise PostgreSQL schema/database delete preview, Cancel, and confirmation only on explicitly disposable objects. Confirm the intended catalog and sibling objects independently.
  The [SQLite range retest](../testing/2026-11-22-full-feature-acceptance-results.md#linux-navigator-range-selection-and-clicked-item-actions) verified inclusive selection, the original anchor, a collapsed anchor, Escape, and clicked-item actions.
  The [live catalog drop test](../testing/2026-11-22-full-feature-acceptance-results.md#postgresql-catalog-drop-retest) verified dependency and busy-session refusals, successful PostgreSQL drops and sibling database preservation. The [desktop catalog deletion retest](../testing/2026-11-22-full-feature-acceptance-results.md#postgresql-desktop-catalog-deletion) verified Navigator previews, Cancel, schema RESTRICT and CASCADE, exact database confirmation and sibling preservation.
- [ ] Run M01–M04 on PostgreSQL and the supported metadata sections on MySQL/SQLite. Compare columns, native constraints, indexes/FKs, dependencies, partitions, permissions and DDL to the actual catalog. Include empty/error/unavailable distinctions, long multiline values, DDL scroll/copy, same names across connections and a disappearing object. Verify scoped DDL replay only where the provider guide promises it.
  The [MySQL and SQLite native DDL retest](../testing/2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-native-ddl-copy-and-replay) verified exact clipboard bytes, definitions and behavior after replay in separate targets.
- [ ] Run M05–M06. Check composite, self, outgoing and incoming FK edges, qualified navigation, grid alignment, zoom/pan, reopen/cancel/reset layout paths and persisted positions. Export the focused diagram as SVG and confirm it opens and includes the expected table names and relationships.
- [ ] Run D01–D04. Verify column draft preview/Persist/Revert and stale conflict against an independently altered catalog. Exercise create/alter/rename/drop, indexes/constraints/views and truncate confirmation on disposable objects for each enabled provider where the UI allows it. Read back native schema/rows on success and verify failed or canceled mutations preserve the documented state.

### Task 5: Verify table browsing, grids, values, and table transactions

**Files:** [runbook](../testing/0.2.0-postgres-manual-acceptance.md) B01–B04, G01–G06, V01–V03, W03/W06, [table guide](../guide/navigator-and-tables.md#table-tabs), [type handling](../guide/type-handling.md).
**Interfaces:** Consumes real tables with typed values and independent read/write sessions. Produces exact row, value, clipboard, search and transaction outcomes.
**Test level:** Desktop E2E with integration read-back over separate sessions.
**RED:** n/a — existing behavior and explicit failure/undo/cancel cases.

- [ ] Run B01–B04 and G01–G06. Check exact rows for filter, order, fetch, Criteria/history and Expand, then verify staged edit/insert/duplicate/delete, default versus NULL, paste rejection, Undo/Redo and atomic Save after a valid batch. Force the G03 failing batch and compare every affected row independently. Exercise copy formats, column/display controls, and FK picker and related-row navigation.
- [ ] Run W06 on table and SQL result tabs. Verify Grid/Text/Record combinations, selection/scrolling, value viewing, copy/wrap, exact decimals and UTC display. Run V01–V03 on writable table tabs for supported source/binary editors, UTF file load/save, invalid encoding, oversized values, stale editor Apply and atomic database read-back. Verify editing controls follow the result's capabilities. Task 6 validates the limited PostgreSQL editable-query-result path. Check keyboard behavior with the value panel and record preview open.
- [ ] Search table data from the footer on all three providers. Compare matching rows and preview limits against the provider-specific cases in the table guide. Test cancellation, empty matches and stale PostgreSQL previews. Record driver-only definition search in Task 2 and confirm the navigator exposes the documented search modes.
- [ ] Run W03 on applicable table tabs. In Auto-Commit, independent reads see Save immediately. In Manual, same-tab reads see pending changes, an independent connection sees only committed values, and Commit/Rollback/close have distinct read-back. Induce a failed savepoint batch and verify the tab retains usable earlier work.

### Task 6: Verify SQL editing, execution, sessions, and query plans

**Files:** [SQL editor guide](../guide/sql-editor.md), [runbook](../testing/0.2.0-postgres-manual-acceptance.md) S01–S07, T01–T05, W04/W05, [query plan suite](../../crates/db-wire/tests/query_plan_test.rs), [P2-05 evidence](2026-11-12-p2-05-completion.md#user-gui-acceptance--2026-11-13).
**Interfaces:** Consumes provider-qualified script tabs and controlled long/failing statements. Produces SQL/file bytes, result/feedback assertions, transaction read-back and plan snapshots.
**Test level:** Desktop E2E and live session integration.
**RED:** n/a — existing action paths with native refusal, cancellation and recovery.

- [ ] Run S01–S06. Verify completion/alias, selected/caret/whole script boundaries, comments/format/indent/snippets/parameters, selection and viewport, Find/Replace and Undo/Redo, chooser/rename/trash/import/save copy/history, command/error feedback, and timeout/cancel/busy handling. Check script and result search focus and vertical action rail. Compare the SQL file bytes and exact returned rows, counts and errors.
  The [Linux parameter prompt retest](../testing/2026-11-22-full-feature-acceptance-results.md#linux-sql-parameter-prompts) verified all seven types on each enabled provider, an invalid SQLite Integer, and MySQL native variables alongside a client parameter.
- [ ] Run W04/W05. Check no result area before execution, bounded split resizing, independent result state and close/adjacent selection, result search and both log limits. Verify safe redaction in Transaction log, the Auto/Manual selector and tab-session controls, and the documented restart behavior of session-only logs.
  The [SQL history and result retention retest](../testing/2026-11-22-full-feature-acceptance-results.md#linux-sql-history-and-result-retention-retest) completed result retention and split bounds on all three providers and verified session log removal on normal Quit in a fresh SQLite workspace. The History UI action is missing.
- [ ] Run applicable T01–T05. Verify temporary objects across runs, isolation and database/schema selectors, savepoints and recovery, read-only and unsupported controls, and PostgreSQL editable query-result identity and complete-key restrictions. Use independent committed-state and stale-write checks. Correlate deferred driver batch behavior with Task 2 results.
- [x] Run S07 and P2-05 again. Verify native estimated/actual capability differences, expensive-node selection, JSON snapshot save/open/compare, invalid version/file rejection and no change to existing results. Use only read-only statements for Analyze in this run. Compare native metrics and the reopened snapshot with the original output.
  Linux [PostgreSQL snapshot results](../testing/2026-11-22-full-feature-acceptance-results.md#additional-postgresql-x11-cases-on-the-fixed-build), [MySQL and SQLite results](../testing/2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-sql-editor), and [file-boundary retest](../testing/2026-11-22-full-feature-acceptance-results.md#linux-query-plan-file-limits-and-provider-checks) verify native metrics and that rejecting or opening plans preserves existing results. The [PostgreSQL node-navigation retest](../testing/2026-11-22-full-feature-acceptance-results.md#linux-postgresql-query-plan-node-navigation) verified estimated and actual ranking, direction, wrap and collapsed-ancestor reveal.

### Task 7: Verify import, export, transfer, backup, restore, and maintenance

**Files:** [runbook](../testing/0.2.0-postgres-manual-acceptance.md) X01–X06, [backup checklist](../testing/p2-02-manual-acceptance.md), [native client setup](../PROVIDERS.md#native-backup-clients), [export implementations](../../crates/app/src/workspace/export/), [native backup implementation](../../crates/app/src/native_backup.rs).
**Interfaces:** Consumes seeded input and pristine target tables/databases/files. Produces byte-level exports, imported/transfer/restored row assertions, and native job status and exit evidence.
**Test level:** Desktop E2E, real filesystem and independently queried targets.
**RED:** n/a — existing normal, error, and cancellation workflows.

- [ ] Run X01–X06. Check CSV mapping, encoding, separator and NULL policy, invalid inputs and rollback. Verify loaded versus full-source scope, source filters, live progress/Cancel, and preservation of pre-existing destinations. Parse CSV, replay SQL in the isolated target, and inspect loaded HTML/Markdown/JSON output for encoding, Unicode, NULL, and row counts.
- [ ] Run same-provider and supported cross-provider table transfers against existing targets. Record mapping/unsupported conversions, atomic and batch outcomes, duplicate or FK failure, Cancel and retries. Compare target rows and schema with independent reads.
  The [cross-provider runner retest](../testing/2026-11-22-full-feature-acceptance-results.md#linux-cross-provider-transfer-retest) verified representative transfers and duplicate commit policy. Native read-back detected a BLOB conversion failure and silent byte changes between MySQL and SQLite. The [MySQL-to-SQLite GUI retest](../testing/2026-11-22-full-feature-acceptance-results.md#cancel-and-scalar-transfer-across-providers) verified scalar rows, atomic duplicate refusal and self-transfer refusal. The [packaged GUI retest](../testing/2026-11-22-full-feature-acceptance-results.md#cross-provider-transfer-in-the-packaged-gui) also verified PostgreSQL → MySQL and SQLite → PostgreSQL scalar rows, duplicate and FK outcomes, and live cancellation under both commit policies. It reproduced both MySQL → SQLite BLOB defects.
- [ ] Run the native backup/restore checklist against PostgreSQL, MySQL and file-based SQLite. Validate full, table and view scope, partition rows and native SQL/SQLite type fidelity, existing-file/populated-target refusal, maintenance output, and empty/inaccessible/restricted target errors. Use the per-provider documented client versions and test transport variants supported by the native tools. Verify every claimed restore in a separate target. Capture explicit unsupported transport results and keep pending ones visible.
- [x] Configure `FASTDB_TEST_SQLITE_CLI_DIR`, `FASTDB_TEST_P202_PG_PORT`, `FASTDB_TEST_P202_PG_BIN`, and `FASTDB_TEST_P202_MYSQL_BIN` for the owned native fixtures. The existing `fastdb-app` test binary passed all 22 `native_backup` cases in four filtered runs with SQLite 3.54.1 and matching PostgreSQL 16.16 and MySQL 8.4.13 tools. Both live cases passed direct TLS, and PostgreSQL also passed SSH + TLS with isolated OpenSSH. The [results](../testing/2026-11-22-full-feature-acceptance-results.md#linux-native-backup-retest) record each transport variant and the remaining desktop verification.

### Task 8: Verify PostgreSQL Administration and Session Monitor

**Files:** [Administration guide](../guide/navigator-and-tables.md#postgresql-administration), [P2-01 evidence](2026-11-16-p2-01-administration.md#verification-evidence), [P2-03 checklist](2026-11-14-p2-03-session-monitor.md#manual-desktop-acceptance), [administration tests](../../crates/db-wire/tests/administration_test.rs), [session monitor tests](../../crates/db-wire/tests/session_monitor_test.rs).
**Interfaces:** Consumes an owned PostgreSQL catalog, restricted profile, and independent clients for known blocking chains. Produces catalog/native privilege reads and exact targeted-session outcomes.
**Test level:** Desktop E2E and real PostgreSQL integration.
**RED:** n/a — existing native mutating, cancellation, and refusal paths.

- [x] Run `cargo test --locked -p db-wire --test administration_test --test session_monitor_test -- --nocapture` with the documented PostgreSQL settings. Verify categorized catalog read-back, native permission errors, stale/incorrect session refusal, and cancel/rollback. All five cases passed on PostgreSQL 16.16 with live coverage required.
- [x] Exercise every Administration family in the guide. Page/filter/refresh, inspect native source and review exact SQL. Dismiss once and compare unchanged objects, then apply to disposable objects and compare their native state. Test invalid SQL/native permission rejection, read-only state, active cancellation and owner replacement. The [Linux results](../testing/2026-11-22-full-feature-acceptance-results.md#administration-catalog-reviewed-changes-and-rollback) record the missing filter, failure messages and coverage of owner replacement.
- [x] Use the P2-03 fixture to create a known blocking chain. Verify displayed ordering and target identity with independent clients. Dismiss and confirm both Cancel and Terminate, verify exact targeted backend state, automatic refresh, and restricted or read-only refusals. See the [Linux GUI and independent PostgreSQL results](../testing/2026-11-22-full-feature-acceptance-results.md#session-monitor-advisory-lock-chain).

### Task 9: Verify packages, install, restart, and signed updates

**Files:** [build and packaging guide](../development/building.md#packaging), [release workflow](../../.github/workflows/release.yml), [per-release checklist](../RELEASE_PLAN.md#32-per-release-checklist), [update guide](../guide/updates.md), [P3-05 evidence](2026-11-21-p3-05-verified-updates.md#execution-checkpoint).
**Interfaces:** Consumes release-mode artifacts, disposable macOS/Linux installs, and a signed test pair for update. Produces package install/remove, restored app state, asset verification and installer handoff results.
**Test level:** Package integration, real artifact/filesystem verification and native desktop E2E.
**RED:** n/a — use the existing signed and rejected artifact fixtures.

- [ ] Build release artifacts with the documented scripts. On Linux run `cargo build --release --locked --target x86_64-unknown-linux-gnu -p fastdb-app` and `bash ci/test-package-deb.sh target/x86_64-unknown-linux-gnu/release/fastdb`. Install/check/remove the candidate DEB in disposable Ubuntu as the release job does. Test launcher, dependencies, and the standalone binary. Smoke-launch a local AppImage if produced by the documented tooling. On macOS package with `./bundle_macos.sh --no-install` and test DMG, ZIP and standalone binary from a clean account. Run R05 using Finder and the packaged executable.
  The [Ubuntu 24.04 launcher retest](../testing/2026-11-22-full-feature-acceptance-results.md#ubuntu-2404-desktop-launcher-and-removal) installed and removed the package, verified its desktop entry and icon, and launched the installed executable through GTK. The app required Fontconfig to be installed separately.
- [x] Run `cargo test --locked -p fastdb-app updater::` and `cargo test --locked -p fastdb-app workspace::software_update`. All 21 automated cases passed, including verification refusal and preservation of the previous destination; native installation remains covered by separate steps.
- [ ] Use a dedicated test account or VM snapshot with its own Downloads, cache, credential store, and install location. Pin the starting app and destination identity before native installation. Verify startup update checks and reachable status/notes/badge/progress, `View on GitHub`, verified package handoff and both manual installer actions. Check About and manual menu update on macOS. On Linux follow the startup/badge route and read the installed version from the tested artifact.
- [ ] For a real signed-update pass, the initiating app must contain P3-05 and the signed target must be newer and compatible. Record initiating/target versions and hashes, manifest verdict, installed version and post-update restart/read-back. An older app without P3-05 can verify an upgrade into the candidate as a separate case. Use the verification fixtures for negative cases that cannot be tested through the public release path. Record Blocked when the required signed pair or native installer environment is unavailable.
  The [published DEB upgrade](../testing/2026-11-22-full-feature-acceptance-results.md#published-debian-package-upgrade-from-rc-to-final) passed through apt from the signed RC to the signed final on Ubuntu 24.04. Both tags predate P3-05. The in-app gate remains Blocked pending a newer compatible signed target for the candidate.
- [ ] Resolve any conflicting status in the installed-build docs against the candidate and update results in Task 11. Keep the release job's publish and key-access controls under the existing release process.

### Task 10: Verify performance and bounds with the release candidate

**Files:** [benchmark guide](../../benchmarks/README.md), [Linux policy](../../benchmarks/README.md#linux-release-policy), [live PostgreSQL protocol](../../benchmarks/postgres/README.md), [P2-10 evidence](../../benchmarks/VALIDATION.md#final-linux-acceptance--2026-11-17).
**Interfaces:** Consumes release binary, empty profile, compatible baseline/threshold policy and dedicated display host. Produces raw reports, comparison verdict, cold-start observation and GUI rendering evidence.
**Test level:** Release desktop and live-driver performance integration.
**RED:** n/a — measure existing benchmarks and report observed limits.

- [ ] Run `cargo test --locked -p xtask` and `cargo build --locked --release -p fastdb-app`. Run the documented `cargo xtask benchmark` and `benchmark-check` commands with the actual display ID and matching policy. Record startup, idle/peak RSS, million-row streaming, scrolling frame times/stutters and binary size. Preserve the selected policy hash and compatible baseline/candidate measurements.
- [x] Use the PostgreSQL release protocol for the 2,000-table catalogs, 20,000-row mixed stream, exact 300,000-byte text/binary read-back and plans. The release driver completed five samples. The real GUI on private X11 rendered and filtered the catalog and reported the Value panel limit. See the execution record.
- [ ] Measure cold start before the warm suite using the documented first-frame probe on a dedicated host. Record it separately from warm medians. Report Wayland stalls or missing macOS/host data as such. Apply the Linux threshold policy only to its approved host and comparable configuration.

### Task 11: Verification & Docs

**Files:** [workflow record](../WORKFLOW_ACCEPTANCE.md), [roadmap](../ROADMAP.md), [known gaps](../guide/not-built-yet.md), [provider evidence](../PROVIDERS.md), [benchmark evidence](../../benchmarks/VALIDATION.md), this plan.
**Interfaces:** Consumes per-case evidence files from Tasks 1–10. Produces an acceptance matrix, failed/blocked cases with reproduction and retest criteria, and updated summaries.
**Test level:** Cross-layer acceptance and documentation integrity.
**RED:** n/a — check the final report against executed cases, the link check, and full diff.

- [ ] Reconcile C01–W07 by exact runbook ID and every supplemental scope-map row. P0-05 requires every applicable release-gate substep in the [runbook](../testing/0.2.0-postgres-manual-acceptance.md#release-decision-and-result-template). Retain independently read row/byte values, installer/version validation, and raw metrics where appropriate. A skipped test or screenshot of a reachable control alone gives no successful action result.
- [ ] Update existing acceptance links and statuses only from observed outcomes. For every Fail/Blocked entry, record exact expected/actual, build/provider/platform, and the retest or fixture needed. Compare current CI job coverage with the matrix. CI runs workspace, live database and TLS jobs. Its package and benchmark workflows have separate triggers, and this full GUI matrix needs its own execution evidence.
- [ ] Verify the final diff, including untracked deliverables, with `git diff --check` and resolve all local Markdown links. Review every task against scope, edge cases, concurrent ownership, error paths, bounds, credential confidentiality, and actual test evidence. If follow-up code changes were authorized, repeat the repo's format, Clippy, workspace build/tests and independent review after the last edit and rerun the affected E2E cases.
- [ ] Preserve redacted reports, remove owned temporary profiles, containers, exports and installers, and retain the runnable binary and reusable build dependencies. Ready to commit the reviewed records as `docs: record full feature acceptance` when authorized.

## Execution evidence

The [2026-11-22 candidate results](../testing/2026-11-22-full-feature-acceptance-results.md) record executed checks and incomplete acceptance. No full task is marked complete while its required platform or provider coverage is open.
