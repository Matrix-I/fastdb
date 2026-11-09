# v0.2.0 pre-release verification

Audit date: 2026-11-05. Source commit: `84dcfb0bf9ab7b6769366c2b6b253a7057eef153`.
Environment: macOS arm64, rustc/cargo 1.99.1. Initial working tree was clean.

**Decision: release acceptance is incomplete.** Implementation checkboxes in the
roadmap do not mean desktop acceptance passed. No version bump, commit, tag,
installation, or publication was performed by this audit.

## Scope update after the audit

The user subsequently limited the v0.2.0 provider acceptance gate to PostgreSQL.
MariaDB, Oracle, SQL Server and Db2 are now disabled in the connection dialog with
**Feature is coming**; their missing live verification no longer blocks this
provider gate. MySQL and SQLite remain selectable but outside the manual gate.
The original audit evidence below remains historical. Use the
[ordered PostgreSQL runbook](testing/0.2.0-postgres-manual-acceptance.md) for the
current manual pass. Its source review identified incomplete P1-08 desktop
transfer and configurable CSV mapping/encoding/NULL-token controls; these need
implementation or explicit release deferral rather than a passing checkbox.

## Update 2026-11-09: points still to verify before release

Source: `main` at `de2c054`. P2 work waits until this verification is done.

- **P1-08 gaps are now implemented**, so they need GUI checks, not a release
  deferral. The runbook covers them in X04 (transfer entries, destination picker,
  read-query guard), X05 (full export Cancel and live count) and X06 (encoding,
  NULL token, manual mapping, conversions, skip invalid values, cancel without
  losing the outcome).
- **Work merged on 2026-11-06/07 has no GUI evidence yet.** The runbook covers it
  in W01–W07: the workspace header, navigator search modes, table tab
  transactions, the Transaction log, script result sizing, Record/Text and the
  connection dialog tabs.
- **P0-05 data paths are automated.** `workflow_test` passed live against
  PostgreSQL 15.19 and MySQL 8.4.13 on 2026-11-09. The P0-05 desktop cases
  (C02/C04, B01–B03, G01–G03, S01/S05/S06, T02/T03, X01–X06, L01–L04) still
  decide the checkbox.
- **The full automated suite must be re-run on the release commit.** Run
  `cargo test --workspace --locked` with `FASTDB_REQUIRE_DB_TESTS=1` and live
  PostgreSQL/MySQL, plus `./ci/test-tls.sh`. The evidence below is from
  `84dcfb0` and does not cover later commits.
- **These release gates are still open:**
  - Version bump in `Cargo.toml` and in `ci/build-*.sh`, which hard-code 0.1.0.
  - A real 0.2.0 bundle, launched from Finder.
  - Keychain behavior across updates.
  - A clean-user DMG install.
  - A Linux build on Linux.
  - Release `panic = "abort"` recovery.
- **Known limits to record in the release notes if accepted:**
  - Transfer/import write only into existing tables.
  - Mapping choices are not saved between runs.
  - The read-query guard refuses `FOR UPDATE`.
  - Value editing caps at 256 KiB.
  - Smart commit is not implemented.
  - MariaDB, Oracle, SQL Server and Db2 are disabled.

## Automated evidence

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo build --workspace --all-targets --locked`: passed (development profile).
- `python3 ci/test_bundle_macos.py`: 7 tests passed; fake build tools test orchestration, not a real release bundle.
- `cargo test --workspace --locked`: passed with local socket access. The initial sandbox run failed to bind an SSH fixture socket; it was an environment restriction.
- `FASTDB_REQUIRE_DB_TESTS=1 FASTDB_TEST_PG_PORT=55439 FASTDB_TEST_MYSQL_PORT=55440 FASTDB_TEST_MYSQL_PASSWORD=<fixture password> cargo test --workspace --locked -- --nocapture`: 570 reported passes across 38 suite groups, zero failures. Fresh isolated PostgreSQL 16/MySQL 8.4 containers were used, with SQLite in process. The first live attempt lacked MySQL's expected `fastdb_test` database; creating that fixture database resolved the setup error before the full rerun.
- `./ci/test-tls.sh`: 6 live TLS tests and 2 SSH-over-TLS tests passed. Podman could not mount the default macOS temporary directory; the successful unchanged-script run redirected `mktemp` to a shared workspace directory. Temporary certificates and TLS containers were cleaned by its trap.
- Reported passes include early-return tests: SQL Server, Oracle, Db2, environment-dependent TLS and native Keychain are not certified by that count. `FASTDB_TEST_KEYCHAIN=1` was not enabled.
- GitHub `gh run list --limit 4 --json headSha,status,conclusion,workflowName`: CI for the audited commit completed successfully. A newer remote commit `88e124c7dad402bba7d0b534cca85044e2206bce` was in progress; this report covers only the local commit.

## Every roadmap P0/P1 acceptance gap

All rows below still require manual desktop evidence. Use disposable databases
and the protocol in [WORKFLOW_ACCEPTANCE.md](WORKFLOW_ACCEPTANCE.md).

| Item | Remaining verification |
| --- | --- |
| P0-01 Capability states | Unsupported, loaded-empty, permission-denied and partial failure presentation; prior manual checks skipped. |
| P0-02 Properties readers | Real PostgreSQL rows, relation navigation, empty/error states and refresh. |
| P0-03 Filter workflow | Criteria SQL/results agree; history, refresh, Expand/Restore and pending-edit guards. |
| P0-04 ER relationships | Composite/self/incoming/outgoing FKs, cross-schema navigation, dragging/saved positions, zoom and connector hit targets. |
| P0-05 Workflow matrix | Still unchecked: connect → browse → run → edit/save/read-back → export/read-back → cancel/disconnect, connection identity and stale replies. Driver integration tests do not complete this desktop gate. |
| P0-06 Columns interaction | Narrow/HiDPI clipping, full-value copy, double-click/Enter and stale-column behavior; prior manual checks skipped. |
| P0-07 Column drafts | SQL preview/Persist/Revert, dirty close, conflict/error recovery and catalog refresh on the owning connection. |
| P0-08 Quit | Menu/shortcut/window close from each dialog/focus context; failed saves keep the app open; prior manual checks skipped. |
| P1-01 Explorer/search | Every object family, literal search, favorites/reveal and same-named objects across connections. |
| P1-02 Schema editing | Form/SQL preview/confirmation, native errors, implicit MySQL commits, rollback and owning navigator refresh. |
| P1-03 SQL editor | Selection/caret execution, Unicode Undo, Find/Replace, native file dialogs, snippets, diagnostics and typed parameters. |
| P1-04 Execution controls | Earlier outcomes survive later errors; timeout, cancellation cleanup/busy refusal, counts/notices and log retention. |
| P1-05 Provider parity | Session TEMP/settings persist; transactions/savepoints, isolation/autocommit, database/schema changes; separate MariaDB and vendor ODBC live tests remain missing. |
| P1-06 Grid | Sort/filter/hide/reorder/find/record, TSV paste, NULL/default, FK lookup and related navigation; PostgreSQL guarded editable-result save/conflict/read-back. Broader query editing stays capability-gated. |
| P1-07 Values | Text/JSON/XML/binary load/save, exact values, empty/NULL/default, size limits and stale apply; ODBC live coverage missing. Oracle empty binary remains unavailable. |
| P1-08 Transfer | Wizard mapping/preview, live progress/cancel, atomic versus per-batch durability, constraint failures, full-source CSV/SQL export and transfer read-back. |
| P1-09 Connections/security | SSH/TLS UI errors, read-only controls, secrets not logged, restart/reconnect and Keychain across ad-hoc updates; separate MariaDB/ODBC live coverage missing. |
| P1-10 Preferences | Restart restores scripts/layout/unsaved text on correct connections without executing SQL or restoring live transactions; theme/font/shortcuts/panel/grid settings. |

## Separate architecture P1 backlog

These IDs are independent of the feature roadmap IDs. None is silently closed.

- **Architecture P1-01:** per-character editor cells and two hit surfaces remain in `crates/ui-editor/src/editor_view.rs`. Element-count/typing-latency baseline and Unicode/cursor acceptance are missing. No speedup is claimed.
- **Architecture P1-02:** operations remain globally serialized. Concurrent-query policy is a product decision; same-connection cancellation, stale completions and save ownership must be verified before enabling overlap.
- **Architecture P1-03:** typed tab state remains a measurement/design task. Trace creation/close/dirty state and measure table-tab cost before choosing a refactor.

Source: [architecture plan](plans/2026-11-03-codebase-architecture-improvements.md).

## Release gates and documentation discrepancies

1. `Cargo.toml` is still 0.1.0. Both `ci/build-macos.sh` and `ci/build-linux.sh` hard-code 0.1.0; bumping Cargo alone will not version their artifacts correctly. `bundle_macos.sh` reads Cargo and produces checksums, but this audit has not built a real 0.2.0 bundle.
2. Linux packaging falls back to host compilation; Linux artifacts need a real Linux build and verification. No `release.yml` exists. Do not claim automatic Linux publication.
3. Release `panic = "abort"` still needs the documented recovery decision. Query worker JoinError handling exists in `db-wire/src/execution.rs`; dev-profile tests do not establish release panic recovery. This is an evidence gap, not a reproduced production bug.
4. Finder-launched signed bundle smoke, working directory `/`, saved connections after restart, Keychain re-prompts across updates, large-result scrolling and clean-user DMG install remain pending.
5. `WORKFLOW_ACCEPTANCE.md` predates provider session parity and full-source transfer/export. Retain loaded-export checks, but also exercise the P1-08 full-source flows above. Its statement that other-provider pinned sessions remain open is stale implementation status.
6. `RELEASE_PLAN.md` describes thin LTO; Cargo currently uses fat LTO. Historical architecture SSH-stub findings are superseded by the forwarding implementation and current tests.
7. Separate MariaDB and SQL Server/Oracle/Db2 are not verified by this run. Either supply their test environments and pass the gates, or explicitly limit/defer their release support claims.

## Review and exceptions

An independent review checked requirements and repository sources and confirmed
the outstanding groups above. No implementation was changed, so reproduction,
new regression tests, query plans, schema/API approval and dependency audits are
inapplicable. No speculative performance or panic fix was made.

Self-review: (1) all eight P0 and ten feature P1 items plus three architecture P1s
are listed; (2) no performance claim or SQL change; (3) no saved credentials read
or logged; (4) only isolated test fixtures used; (5) English Markdown and existing
command conventions; (6) no API introduced; (7) unresolved concurrency/provider
scope is explicit; (8) automated evidence is separated from skipped/manual tests;
(9) no implementation/design change.

GUI end-to-end checks are intentionally omitted under the explicit no-Computer-Use
project preference. Native Keychain, missing providers, release-profile runtime,
and real packaging acceptance remain unverified. No plan commit was created:
this is an audit document, and commits were not requested.
