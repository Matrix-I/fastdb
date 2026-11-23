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
