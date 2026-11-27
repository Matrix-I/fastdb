# Test findings — 2026-11-23

Protocol: [full-feature test plan](../plans/2026-11-23-full-feature-test-plan.md).
Defects found during the pass are recorded here and were not fixed during it.

## Run: Linux x86_64, automated phases

| Item | Value |
| --- | --- |
| Commit | `359502a` (workspace 0.3.0); plan commit `2f525ec` changes docs only |
| Host | Ubuntu 24.04.5 LTS container, x86_64, 4 CPUs, no display (Xvfb with llvmpipe Vulkan for the launch smoke) |
| Toolchain | rustc / cargo 1.100.0 (stable) |
| Servers | PostgreSQL 16.16 (`postgres:16`), MySQL 8.4.13 (`mysql:8.4`), MariaDB 11.4.13 (`mariadb:11.4`), Docker 29.10.0 |
| Native clients | `sqlite3` 3.45.1 (Ubuntu package), `minisign` 0.11 |
| macOS arm64 | Not run: no Mac was available to this session |
| Manual desktop phase (E) | Pending: GUI acceptance is done by the user |

### Results

| Case | Result | Notes |
| --- | --- | --- |
| A1 fmt | PASS | |
| A2 clippy `-D warnings` | PASS | 6 Cargo manifest warnings, see F-03 |
| A3 build `--all-targets` | PASS | |
| B1 `cargo test --workspace --locked` | PASS | 834 passed, 0 failed, 0 ignored (live tests skipped) |
| B2 `ci/test_bundle_macos.py` | FAIL (environment) | See F-04 |
| C1 live engine/drivers | PASS | 330 passed, `FASTDB_REQUIRE_DB_TESTS=1` |
| C2 `workflow_test` | PASS | SQLite, PostgreSQL and MySQL workflows |
| C3 `fastdb-app` with live servers | PASS | 408 passed; includes 5 live tests that CI never runs |
| C4 `ci/test-tls.sh` | PASS | 6 TLS tests and 2 SSH+TLS tests |
| C5 MariaDB plans | PASS | 5 passed, `FASTDB_REQUIRE_MARIADB_TESTS=1` |
| C6 native backup | FAIL (SQLite), BLOCKED (PostgreSQL, MySQL) | See F-01 and F-05 |
| C7 PostgreSQL schema/database drop | PASS | |
| C8 credential store (Secret Service) | PASS | Private D-Bus session with a disposable gnome-keyring |
| C9 ODBC providers | SKIP | Vendor drivers and servers not set up; desktop providers disabled |
| APP-01 launch smoke (automated part only) | PASS | Debug build ran 15 s under Xvfb without a panic; GUI not inspected |
| A4 release build | PASS | Via `bundle_linux.sh`; executable 34.5 MB (budget 100 MiB) |
| D3 `bundle_linux.sh`, `ci/test-package-deb.sh` | PASS | Binary and `fastdb_0.3.0_amd64.deb` built; no AppImage because `appimagetool` is not installed |
| D4 apt install / remove | PASS (partial) | Installed 0.3.0, desktop entry validates, `/usr/bin/fastdb` ran 10 s under Xvfb without a panic, removal left no files. Launch from a desktop launcher not tested (no desktop session) |

## Findings

| ID | Case | Severity | Status |
| --- | --- | --- | --- |
| F-01 | C6 | Major | Open |
| F-02 | APP-04, UPD-02 | Major (needs GUI confirmation) | Open |
| F-03 | A2 | Minor | Open |
| F-04 | B2 | Minor | Open |
| F-05 | C6 | Test infrastructure | Open |
| F-06 | C3 | Minor | Open |

### F-01 — SQLite restore into an existing empty file fails with `sqlite3` 3.45.1

- Platform / build: Linux x86_64, `359502a`, Ubuntu 24.04 `sqlite3` 3.45.1.
- Steps: `FASTDB_TEST_SQLITE_CLI_DIR=/usr/bin cargo test --locked -p fastdb-app native_backup -- --nocapture`.
- Expected: `native_round_trip_maintenance_and_restore_rejections` passes.
- Actual: the test fails at `restore(&tool, &archive, &empty, …)`
  (`crates/app/src/native_backup/sqlite.rs:474`) with
  `Native database tool failed (exit status: 1)`. Running the same command line
  by hand prints `sqlite3: Error: unknown option: -ifexists`. The option is
  passed at `crates/app/src/native_backup/sqlite.rs:185`. `-nofollow` is
  accepted by this version.
- Impact: **Restore Database…** into an existing empty SQLite file is expected
  to fail with the default Ubuntu 24.04 client. Backup, restore into a new file
  and the scoped backup test passed. `docs/PROVIDERS.md` documents `sqlite3`
  3.37 or newer for scoped backups but no minimum for this restore path.
- macOS: check the system `/usr/bin/sqlite3` version and behavior in the macOS pass.

### F-02 — About and manual update check have no entry point on Linux (static finding)

- Platform / build: Linux x86_64, `359502a`. Found by reading the source; not yet confirmed in the GUI.
- Evidence: About and **Check for Updates…** are added only as native menu
  items through `cx.set_menus` (`crates/app/src/main.rs`, FastDB menu). In
  GPUI 0.2.2, the Linux `set_menus` stores the menus and draws nothing
  (`gpui-0.2.2/src/platform/linux/platform.rs:474`). FastDB has no other
  reference to these actions and no key binding for them. Theme, sidebar,
  zoom and Quit have key bindings.
- Expected: Linux users can open About and run a manual update check.
- Likely actual: neither is reachable on Linux. The automatic startup check and
  its top-bar badge still apply. `docs/guide/updates.md` describes the menu path
  without a Linux note.
- Next step: confirm in the GUI with plan case APP-04.

### F-03 — Cargo reports unused dependencies with Rust 1.100

- Steps: `cargo clippy --locked --workspace --all-targets -- -D warnings` (exit 0).
- Actual: the `cargo::unused_dependencies` lint warns about `bytes` and
  `memmap2` in `mem-engine`, `thiserror` in `platform-hal`, `thiserror` and
  `tracing` in `sql-intel`, and `platform-hal` in `ui-editor`. `-D warnings`
  does not apply to these Cargo lints, so CI stays green, but every build prints
  them.

### F-04 — `ci/test_bundle_macos.py` cannot fully pass on Linux and is not run by CI

- Without `minisign`, 4 signing tests fail by design (`minisign is required for signing tests`).
- With `minisign` installed by apt (`/usr/bin/minisign`),
  `test_missing_minisign_leaves_unsigned_checksums` fails: the test keeps
  `/usr/bin` on `PATH`, so it cannot hide `minisign`, and the script warns about
  the missing secret key instead of a missing tool.
- No workflow in `.github/workflows/` runs this test, so it is checked only by hand on macOS.

### F-05 — PostgreSQL and MySQL native backup live tests lack a checked-in fixture

- `live_backup_restore_scope_and_maintenance` (PostgreSQL) and
  `live_backup_restore_and_maintenance` (MySQL) return early and print `ok`
  without their variables. Running them needs pre-seeded databases and
  accounts (for example `p202_source` with specific tables, role
  `p202_fixture`, and per-target MySQL accounts). The setup is not in the
  repository or the docs, so this pass could not run them. Status: BLOCKED.

### F-06 — One app live test ignores `FASTDB_REQUIRE_DB_TESTS`

- `native_materialized_view_reaches_the_read_only_properties_metadata_pipeline`
  (`crates/app/src/workspace/connections.rs`) returns silently when
  `FASTDB_TEST_PG_PORT` is unset. It does not check `FASTDB_REQUIRE_DB_TESTS`,
  unlike the other live tests. Required runs can therefore pass without
  exercising it. It did run in C3.

## Linux continuation — 2026-11-26

The run at `359502a` above is the original evidence. The continuation
uses the Ubuntu 24.04 x86_64 DEB built at `7c248df` on isolated Xvfb,
with disposable provider fixtures. macOS is deferred by scope. The
[full acceptance report](2026-11-22-full-feature-acceptance-results.md)
records the environment, reproduction and independent read-back.

### E0 failed or blocked cases

The [runbook index](2026-11-22-full-feature-acceptance-results.md#linux-desktop-runbook-coverage)
lists all 61 cases, with 43 Pass, 16 Fail, 1 Blocked and 1 Deferred.
The [P0-05 release decision](2026-11-22-full-feature-acceptance-results.md#linux-p0-05-release-decision)
records seven required failures.

| Case | Result | Observation and prerequisite |
| --- | --- | --- |
| E0 B01 | FAIL | Paging, scroll to ID 1000, resize/autofit and Grid/Text passed. At 1280 pixels and scale 1, the lines of an LF-containing note were cut by the fixed row bounds. Its clipboard bytes matched PostgreSQL exactly. |
| E0 M01 | FAIL | Properties, full clipboard, native DDL replay and estimate labeling passed. The narrowed scale-2 Columns view wrapped and clipped the comment row. |
| E0 S02 | FAIL | Format, Undo/Redo, selection and Unicode clipboard passed. Required line-edit and Find/Replace controls were absent and the long-paste caret remained clipped. |
| E0 S03 | FAIL | FROM and alias completion passed. Diagnostics appeared only on Execute. Snippets and SQL copy actions had no reachable rail controls. |
| E0 S05 | FAIL | Script boundaries, native errors, counts and timing passed. The documented Execution log was absent and the PostgreSQL notice was not displayed separately. |
| E0 S06 | FAIL | Cancel, alternate-tab busy refusal and recovery passed. The documented Timeout control was absent. Retest setting it to five seconds, cancellation and reset when the control is available. |
| E0 T02 | FAIL | Manual mode began implicitly and its toolbar Rollback and Commit passed independent read-back. No explicit toolbar Begin was available. |
| E0 D03 | FAIL | Verified table and column actions, view create/alter/drop, and index and constraint drop with independent PostgreSQL. The separate index and constraint creation controls were unavailable. |
| E0 V02 | FAIL | Binary grid edit refers to Value Editor, but the candidate has no entry point. Typed edits and file round trips remain unverified. |
| E0 V03 | BLOCKED | Stale Apply requires a reachable Value Editor. |
| E0 X02 | FAIL | Duplicate and cancel durability matched the expected counts for both policies. Both cancel attempts reported cleanup timeout, and the Per-Batch SQL was still running after the GUI outcome. |
| E0 X04 | FAIL | SQL result scope, write-result refusal and duplicate/cancel durability matched native read-back. Cleanup timed out on both policies, and the BLOB runner changed or rejected valid bytes. |
| E0 L03 | FAIL | Dirty guards and durability checks passed. The obsolete Save or Cancel banner survived Cancel and reconnect with no active draft. |
| E0 R01 | FAIL | Theme and zoom restored. The saved hidden Navigator reopened visible. Remaining layout, tabs, logs and timeouts need verification. |
| E0 W01 | FAIL | The native Openbox frame moved, but dragging the documented FastDB title row did not move the window. Double-click zoom, divider resize, separate rows and the narrow scrollable toolbar worked. |
| E0 W02 | FAIL | All mode searches, expanded tree restore and file open passed. Empty Tables search did not offer Browse catalog objects. |
| E0 W04 | FAIL | A synthetic bound value appeared verbatim in Transaction log and matched a search. Paging and retention across tab close and disconnect passed. The later SQLite run verified a fresh session and file removal after normal Quit. Modal resource release remains unverified. |

### Supplemental observations

| Case | Status | Observation |
| --- | --- | --- |
| D4 | PASS | Ubuntu 24.04.5 with Fontconfig. `gtk-launch fastdb` executed the packaged binary. Quit and apt removal preserved the seeded profile file. [Launcher evidence](2026-11-22-full-feature-acceptance-results.md#ubuntu-2404-desktop-launcher-and-removal) |
| D5 | FAIL | The package version matched the workspace. The required About dialog entry was absent on Linux. [Launcher evidence](2026-11-22-full-feature-acceptance-results.md#ubuntu-2404-desktop-launcher-and-removal) |
| APP-01 | FAIL | The centered 1280×800 empty workspace passed. The first nonblack workspace sample took 2,874 ms on Xvfb, over the 2,000 ms budget. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-empty-profile-startup) |
| APP-02 | FAIL | Title and empty-bar double-click worked on X11 and left the window unchanged on native Wayland under nested Sway. Tab selection and close worked on both. The simulated update badge reopened release notes on X11 and native Wayland without changing window geometry. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-title-bar-and-tab-clicks) |
| APP-03 | FAIL | Theme and editor zoom persisted. Ctrl+B hid Navigator and stored width 0, but relaunch showed Navigator at 160 pixels. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-theme-editor-zoom-and-sidebar-persistence) |
| APP-04 | FAIL | Linux GUI follow-up to F-02 found no About or manual Check for Updates entry. F10 and F1 left the workspace unchanged. [Launcher evidence](2026-11-22-full-feature-acceptance-results.md#ubuntu-2404-desktop-launcher-and-removal) |
| APP-05 | FAIL | Shortcut and window close guarded staged edits, a dirty column and a failed connection save. A pending script was saved exactly. The Linux application menu had no reachable Quit entry. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-quit-guards-and-save-recovery) |
| APP-06 | PASS | On X11 at scales 1 and 2, text, the three provider logos, grid lines and both diagram cards rendered successfully. The same foreign-key arrow remained aligned. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-hidpi-grid-and-diagram) |
| APP-07 | FAIL | DejaVu Sans Mono kept columns aligned. After removing the DejaVu font packages and adding Noto Sans for UI startup, the Text view used proportional spacing and the same markers drifted by 32 pixels. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-plain-text-font-fallback) |
| APP-08 | FAIL | Native Wayland IBus Unikey, CangJie 5 and Unicode input composed Vietnamese, CJK and emoji in GNOME Text Editor. FastDB received commit events but dropped the text in the SQL editor, filter, cell editor and connection name. [IME evidence](2026-11-22-full-feature-acceptance-results.md#linux-ibus-composition-on-private-wayland) |
| APP-09 | FAIL | X11 SQL, grid, DDL and Record round trips through GNOME Text Editor preserved the expected values. Native Wayland SQL and grid copies passed. Wayland pointer-only Copy DDL and Copy record retained the old clipboard until FastDB received a fresh keypress. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-clipboard-round-trips-on-private-x11-and-wayland) |
| APP-10 | FAIL | Export, import, SVG and plan chooser cancellations wrote no files. With the portal stopped, exports and plan actions displayed chooser errors, but CSV Import Browse failed silently. FastDB still ran SQL. PostgreSQL backup output and client-directory choosers canceled cleanly and displayed errors with the portal stopped. SQL-copy and Value Editor file controls are missing. [Native file evidence](2026-11-22-full-feature-acceptance-results.md#linux-native-file-dialogs-with-the-portal-stopped) |
| APP-11 | PASS | SIGKILL left the transaction log and million-row spill on disk. Relaunch removed both and restored byte-identical autosaved SQL. [Restart evidence](2026-11-22-full-feature-acceptance-results.md#linux-crash-restart-and-temporary-file-recovery) |
| APP-12 | FAIL | Spill files were removed on tab close and Quit. A second Linux UID could not start FastDB after the first created the shared spill directory with mode 0755. [Permission evidence](2026-11-22-full-feature-acceptance-results.md#linux-spill-directory-permissions-for-a-second-user) |
| CON-01 | PASS | Invalid password, closed port, Test, Connect, restart and saved MySQL reconnect each exercised. [Connection evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-connection-profiles) |
| CON-02 | PASS | Existing and new files, missing read-only refusal and two isolated memory profiles verified. [Connection evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-connection-profiles) |
| CON-03 | FAIL | MySQL used its Unix socket. PostgreSQL found its Unix socket but returned Not a directory, while psql connected directly. Enabling TLS forced TCP on both profiles. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-unix-socket-discovery-and-tls-override) |
| CON-04 | PASS | Both PostgreSQL and MySQL accepted PEM, DER and system trust. Both refused untrusted issuers, a wrong hostname and plaintext-only servers. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-tls-trust-and-refusal-matrix) |
| CON-05 | PASS | PostgreSQL and MySQL accepted protected P12 and empty-password PFX archives, rejected wrong passwords and certificate-only archives, and reconnected with saved credentials. Clearing removed the identity and stored archive password. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-pkcs12-client-identity) |
| CON-06 | PASS | PostgreSQL and MySQL accepted SSH password and encrypted-key authentication, rejected an unrelated fingerprint, and rejected an incorrect database TLS hostname. Live sessions reported active ciphers. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-ssh-authentication-and-database-tls) |
| CON-07 | PASS | SQL and schema writes were refused, normal grid editing stayed disabled, and read-only tables were omitted from Transfer destinations on all enabled providers. [Connection evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-connection-profiles) |
| CON-08 | PASS | Invalid RGB was rejected. Group and color persisted across restart. [Connection evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-connection-profiles) |
| CON-09 | PASS | MySQL validated the 1–120 second range and timed out on both stalled handshakes and a silent neighbor. An immediate kernel routing error surfaced early. [Deadline evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-connection-deadlines-without-a-responsive-peer) |
| CON-10 | PASS | Unavailable Secret Service showed the save warning and blocked Quit. Restoring the service and retrying saved a keyring reference, then restart connected without password re-entry. A locked item remained intact and did not authenticate after the unlock prompt was dismissed. Interactive unlocking was not exercised. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-secret-service-restart-lock-and-save-retry) |
| CON-11 | PASS | Local directories were 0700 and the credential file was 0600. Restart and return to local mode reused the original credential. Switching to Secret Service left it empty until an explicit native Save, which left local bytes unchanged. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-local-and-native-credential-isolation) |
| CON-12 | PASS | Both plaintext formats were preserved byte-for-byte without Secret Service. With the isolated local store available, each migrated and reconnected, and the obsolete legacy file was removed. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-legacy-plaintext-credential-migration) |
| CON-13 | PASS | Deleting one PostgreSQL profile removed its local database credential and diagram directory. The other profile kept its data, reconnected and restored its diagram after relaunch. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-connection-deletion-and-diagram-cleanup) |
| CON-14 | PASS | Two-step picker, draft retention, visible-field keyboard order, small window footer and Copy details verified. [Connection evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-connection-profiles) |
| NAV-01 | PASS | All three providers grouped fixture relations correctly and displayed the expected columns, constraints, foreign keys and indexes. PostgreSQL also grouped its materialized view and foreign table. [Navigator metadata evidence](2026-11-22-full-feature-acceptance-results.md#linux-navigator-folder-and-key-metadata) |
| NAV-02 | FAIL | Connections, Files and Tables stopped at 100 in over-limit fixtures. PostgreSQL Catalogs and schemas returned 101 when the active database and 100 schemas shared the search prefix. Exact searches still found item 120 in all four modes. [Navigator search evidence](2026-11-22-full-feature-acceptance-results.md#linux-navigator-caps-in-connections-files-and-catalogs) |
| NAV-03 | FAIL | The documented Catalog Browser row is absent. Enter in the empty search opened the browser, PostgreSQL showed the renamed favorite's missing-object feedback, and MySQL and SQLite reported unsupported discovery. [Catalog Browser evidence](2026-11-22-full-feature-acceptance-results.md#linux-catalog-browser-keyboard-route) |
| NAV-04 | FAIL | With no conflicting filter, Reveal selected the correct owning table or view on MySQL and SQLite, including from column tabs and across connections. A filter that excluded the target remained active after Reveal, so the target stayed hidden. PostgreSQL table and column variants remain unverified. [Reveal evidence](2026-11-22-full-feature-acceptance-results.md#linux-reveal-active-object-across-connections) |
| NAV-05 | FAIL | Table and view creation worked on all three providers, with independent catalog and data checks. SQLite refreshed the seven supported node types and MySQL refreshed the schema. Refresh is missing from the Views folder and SQLite view menus. Remaining MySQL and PostgreSQL branch-level reloads are unverified. [Creation and refresh evidence](2026-11-22-full-feature-acceptance-results.md#linux-navigator-creation-and-refresh) |
| NAV-06 | PASS | Dragging the shared divider changed the Navigator from 280 to 390 pixels. The saved width and the settled window after a normal restart agreed. [Navigator layout evidence](2026-11-22-full-feature-acceptance-results.md#linux-navigator-search-cap-and-layout-retest) |
| NAV-07 | PASS | Blocking chain, both prompts, read-only and restricted visibility passed. A long list label crossed into the details pane. [Monitor evidence](2026-11-22-full-feature-acceptance-results.md#session-monitor-advisory-lock-chain) |
| NAV-08 | PASS | Eight families and denial against a restricted account exercised. Syntax and permission errors share the cleanup prefix `Cancellation request sent`. Task 8 also records the missing object filter. [Administration evidence](2026-11-22-full-feature-acceptance-results.md#administration-catalog-reviewed-changes-and-rollback) |
| NAV-09 | FAIL | PostgreSQL detected the native client and accepted a directory override. MySQL and SQLite omitted the native-tool actions, despite passing native-client integration tests. [Native-tool evidence](2026-11-22-full-feature-acceptance-results.md#postgresql-native-tool-dialogs) |
| GRID-01 | PASS | PostgreSQL and SQLite reached 250,000 and 1,000,000 grid rows, matching independent source counts. At a million rows peak sampled RSS was 269,908 KiB and 234,912 KiB respectively, with spill files removed on tab close or quit. [Streaming and RSS evidence](2026-11-22-full-feature-acceptance-results.md#linux-large-grid-streaming-and-resident-memory) |
| GRID-02 | PASS | PostgreSQL cancellation was verified in L01/L02. This continuation canceled active MySQL and SQLite reads and verified a fresh read on each same tab. [Provider evidence](2026-11-22-full-feature-acceptance-results.md#cancel-and-scalar-transfer-across-providers) |
| GRID-08 | PASS | PostgreSQL, MySQL and SQLite verified the compact Tab toggle, 64-field paging, Grid/Text continuity and Escape on 66-column fixtures with native read-back. [Record evidence](2026-11-22-full-feature-acceptance-results.md#linux-record-view-on-wide-tables) |
| GRID-13 | FAIL | The GUI verified scalar rows in all three planned directions, duplicate and FK outcomes, cancellation under both commit policies, and self-transfer refusal. MySQL → SQLite reproduced both BLOB rejection and silent byte changes. [Provider evidence](2026-11-22-full-feature-acceptance-results.md#cross-provider-transfer-in-the-packaged-gui) |
| GRID-14 | PASS | All three providers distinguished disconnected tables. MySQL and SQLite displayed distinct not-implemented and unsupported states. PostgreSQL distinguished loaded-empty Policies from a native permission denial. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-table-properties-capability-states) |
| GRID-15 | FAIL | All three exports opened in Firefox with the expected four directed links. Two MySQL SVG headings overran their card borders. The prior restart retest verified position restoration. [Diagram evidence](2026-11-22-full-feature-acceptance-results.md#linux-svg-export-and-postgresql-search-cap) |
| GRID-16 | FAIL | PostgreSQL verified exact values and matching UTC instants. The MySQL and SQLite viewers copied JSON, XML and hex matching native read-back. Both copied 256 KiB exactly and rejected values one byte larger. The shared Value Editor has no GUI entry, blocking the editor limit, file encodings, UTC toggle and stale Apply. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-value-viewer-and-editor-entry) |
| GRID-17 | PASS | PostgreSQL W06 and the MySQL and SQLite retest verified the 2,000-row and 255-character limits and `[NULL]` in the Text clipboard for 2,001 loaded rows. [Plain-text evidence](2026-11-22-full-feature-acceptance-results.md#linux-plain-text-result-limits) |
| GRID-03 | FAIL | The default 1280-pixel window hides Search table data with Navigator open. At wider sizes, all three providers verified matching, preview and Cancel during a lock wait. SQLite displayed Cancelled alongside a database-is-locked request error. MySQL and SQLite recovered after lock release. [Search evidence](2026-11-22-full-feature-acceptance-results.md#linux-table-data-search-on-the-packaged-gui) |
| GRID-04 | PASS | PostgreSQL, MySQL and SQLite verified both wrap directions, case sensitivity, staged values with unchanged native data, the 4,096-byte boundary, and Cancel with recovery on 150,000 loaded rows. [Loaded-result evidence](2026-11-22-full-feature-acceptance-results.md#linux-find-in-loaded-results) |
| GRID-05 | PASS | MySQL and SQLite verified column-menu multi-key sort, provider-specific quote handling in Criteria, and Back/Forward restoring the exact order and filtered rows. [Sort and Criteria evidence](2026-11-22-full-feature-acceptance-results.md#linux-multi-key-sorting-and-criteria-history) |
| GRID-06 | PASS | PostgreSQL, MySQL and SQLite accepted 1,024 cells and 256 KiB, rejected inputs above either limit, and distinguished text `NULL`, SQL NULL and the database default after Save. [Paste and native read-back evidence](2026-11-22-full-feature-acceptance-results.md#linux-tsv-paste-limits-and-empty-value-actions) |
| GRID-07 | PASS | PostgreSQL, MySQL and SQLite used both key columns in forward and reverse navigation. Choose key staged both columns, and Cancel preserved the original rows. MySQL refused numeric lookup with its text-key message. [Composite key evidence](2026-11-22-full-feature-acceptance-results.md#linux-composite-key-navigation-and-picker) |
| GRID-09 | PASS | All three providers pasted TSV, CSV and SQL INSERT into GNOME Text Editor, and native SQL replay preserved NULL and empty text. An 18 MiB TSV selection showed the size-limit error and preserved the prior clipboard. [Clipboard evidence](2026-11-22-full-feature-acceptance-results.md#linux-clipboard-formats-and-output-limit) |
| GRID-10 | PASS | MySQL and SQLite verified Auto and Manual Save, Commit, Rollback, constraint failure isolation and recovery with independent clients. MySQL twice refused to switch back to Auto after Rollback. Reopening the table restored Auto. [Transaction evidence](2026-11-22-full-feature-acceptance-results.md#linux-table-transactions-and-failed-batch-recovery) |
| GRID-11 | PASS | PostgreSQL, MySQL and SQLite produced seven validated exports each. Native SQL replay, chooser and running cancellation, and preservation of existing files after write failure were verified. [Export evidence](2026-11-22-full-feature-acceptance-results.md#linux-export-formats-cancellation-and-write-failure) |
| GRID-12 | FAIL | All three providers verified formats, mapping, conversion, both commit policies and lock-wait cancellation. When batch two was active, MySQL and SQLite Per-Batch Cancel preserved exactly 256 rows and released the writer. PostgreSQL X02 still records a cleanup timeout during an active insert. [CSV import evidence](2026-11-22-full-feature-acceptance-results.md#linux-csv-import-formats-commit-policies-and-cancellation) |
| SQL-01 | FAIL | Autosave, the cross-connection chooser and Navigator Files reopened the correct provider's script. Rename and Delete have no visible SQL menu or rail entry, so collision handling and moving a file to trash remain unverified. [Script reopening evidence](2026-11-22-full-feature-acceptance-results.md#mysql-and-sqlite-split-bounds-and-script-reopening) |
| SQL-02 | PASS | MySQL and SQLite selection, caret, full script, CTE, UNION, INSERT … SELECT and adjacent SELECTs verified. PostgreSQL is covered by S01/S05. [SQL editor evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-sql-editor) |
| SQL-03 | PASS | GUI batch syntax covered MySQL DELIMITER, SQLite trigger bodies and the earlier PostgreSQL dollar-quoted block. [SQL editor evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-sql-editor) |
| SQL-04 | PASS | MySQL session identity, variables, TEMP and implicit DDL commit verified. SQLite TEMP isolation, rollback and per-preset in-memory isolation verified. [SQL editor evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-sql-editor) |
| SQL-05 | FAIL | Database and Schema behavior and native read-back were exercised across the supported Linux fixtures. The transaction menu applied isolation and Auto/Manual directly. The documented preview dialogs are missing. [Session control evidence](2026-11-22-full-feature-acceptance-results.md#linux-sql-session-control-read-back) |
| SQL-06 | FAIL | The documented Timeout selector is missing. PostgreSQL and SQLite long statements could be cancelled and recovered. MySQL `SLEEP(6)` completed in 6,000 ms with the default settings. Configuring and firing an explicit preset remains unverified. [Timeout evidence](2026-11-22-full-feature-acceptance-results.md#linux-transaction-log-pagination-and-timeout-controls) |
| SQL-07 | FAIL | MySQL warning appeared through native SHOW WARNINGS, and SQLite reported the expected row count. The separate Execution log control remains absent. [SQL editor evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-sql-editor) |
| SQL-08 | FAIL | All three providers paged to 130 matches. With a 67,113-byte SQLite script, the modal did not respond and FastDB processed Quit about 50 seconds after the keypress. The log file was removed when the process eventually exited. The exact trigger remains unconfirmed. [Session log evidence](2026-11-22-full-feature-acceptance-results.md#linux-transaction-log-pagination-and-timeout-controls) |
| SQL-09 | PASS | MySQL and SQLite native plan, JSON save/open/compare and capability distinction verified. Earlier PostgreSQL coverage includes malformed snapshots. [SQL editor evidence](2026-11-22-full-feature-acceptance-results.md#linux-mysql-and-sqlite-sql-editor) |
| SQL-10 | PASS | All seven parameter types executed in PostgreSQL, MySQL and SQLite. SQLite rejected an invalid Integer and accepted the corrected value. The MySQL preview and result preserved native `@name` and `@@name`. [Parameter prompt evidence](2026-11-22-full-feature-acceptance-results.md#linux-sql-parameter-prompts) |
| SQL-11 | FAIL | PostgreSQL verified relation and alias completion and Format with Unicode preserved. Snippets, Find/Replace, comment/indent/outdent and Import/Save SQL copy had no reachable editor controls. The 16 MiB SQL copy limit remains unverified through the GUI. [S02 and S03 evidence](2026-11-22-full-feature-acceptance-results.md#postgresql-browse-editor-and-preferences-on-private-xvfb) |
| SQL-12 | FAIL | All three providers ran and omitted oversized statements. Short SQL was present in the per-profile history files. The documented History action has no reachable GUI entry, so persisted entries cannot be recalled. [SQL history evidence](2026-11-22-full-feature-acceptance-results.md#linux-sql-history-and-result-retention-retest) |
| SQL-13 | PASS | The five supported scalar types saved. Primary-key and numeric fields stayed read-only. Eight query or table forms were rejected. A two-row conflict committed neither draft and left a stale prior success status next to the error. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-editable-postgresql-results-and-atomic-conflict) |
| SQL-14 | PASS | All three providers kept the latest 20 results and selected the adjacent result on close. PostgreSQL W05 and the short-query MySQL and SQLite retest verified the 15–85% split limits. [Result retention evidence](2026-11-22-full-feature-acceptance-results.md#linux-sql-history-and-result-retention-retest) |
| SQL-15 | PASS | A private native GNOME 50.1 session verified the documented Linux execution, editor, filter, tab, appearance and Quit shortcuts. No intercept by the exercised GNOME defaults was observed. [Shortcut evidence](2026-11-22-full-feature-acceptance-results.md#linux-shortcut-matrix-on-private-gnome) |
| UPD-01 | PASS | First startup ran an update check. Immediate relaunch and a timestamp under 24 hours suppressed it. A timestamp older than 24 hours allowed a new check. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-startup-update-check-interval) |
| UPD-02 | FAIL | The fixture reported the installed version. Linux exposed no manual Check for Updates control, so the up-to-date confirmation could not be opened. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-manual-update-check-entry) |
| UPD-03 | PASS | The v0.9.0 badge reopened the release notes without resizing the window. The Linux override offered the simulated arm64 DMG and enabled Upgrade. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-simulated-update-badge-and-notes) |
| UPD-04 | BLOCKED | The latest published release remains v0.3.0. A compatible newer production-signed target is required. [Package evidence](2026-11-22-full-feature-acceptance-results.md#published-debian-package-upgrade-from-rc-to-final) |
| UPD-05 | PASS | With the API returning 403, the release binary followed the latest-release redirect and displayed v0.9.0 without an installer. [Evidence](2026-11-22-full-feature-acceptance-results.md#linux-updater-api-fallback) |
| PERF-01 | SKIPPED | This benchmark requires macOS on a fixed display. The current acceptance scope is Linux. |
| PERF-02 | BLOCKED | Xvfb and the current Wayland session do not match the recorded X11 display configuration. [Performance evidence](2026-11-22-full-feature-acceptance-results.md#linux-release-performance) |
| PERF-03 | PASS | One private native Wayland run completed the million-row workload and all 300 scroll intervals. Seven exceeded 100 ms, with a 171.79 ms maximum. The scroll watchdog did not trigger. [Wayland evidence](2026-11-22-full-feature-acceptance-results.md#linux-wayland-benchmark-smoke) |
