# Full-feature test plan: macOS arm64 and Linux x86_64

Candidate: `main` at `359502a` (workspace version 0.3.0). Targets:
`aarch64-apple-darwin` (macOS on Apple Silicon) and `x86_64-unknown-linux-gnu`.
Linux aarch64 is not a build target and is out of scope.

This plan is a protocol, not a result. Results and defects are recorded in the
findings file (section 10). During a test pass, defects are recorded and not fixed.

## 1. Baseline status (2026-11-23)

| Source | Scope | Result |
| --- | --- | --- |
| CI run #111 (`main`, `359502a`) | Live PostgreSQL 16 / MySQL 8.4 for `mem-engine`, `db-wire`, `platform-hal`, `sql-intel`; `ci/test-tls.sh` | success |
| CI run #111 | macOS 15: fmt, clippy `-D warnings`, build, `cargo test --workspace` | success |
| CI run #111 | Ubuntu 24.04: clippy `-D warnings`, build, `cargo test --workspace` | success |
| Local Linux x86_64 run (this pass) | Phases A–C below | see findings file |

Gaps in the current automation (from `.github/workflows/ci.yml` and test gates):

- The live-database CI job does not test the `fastdb-app` crate. Its live tests
  (`connections.rs`, `params_modal.rs`, `session_controls.rs`,
  `transaction_mode.rs`, `main.rs`) skip in every CI job.
- No CI job runs MariaDB, ODBC providers, native backup/restore round trips,
  PostgreSQL schema/database drop, or the OS credential-store round trip.
- The macOS job has no database servers, so the macOS TLS client-identity path
  (Security framework validation in `db-wire/src/traits.rs`) and Keychain are not
  exercised in CI.
- There are no automated GUI tests. Desktop behavior is accepted manually.

## 2. Rules

- Test one candidate build per platform. Record the commit, About dialog version,
  OS version, architecture, display scale, server and native client versions.
- Use disposable databases and an isolated profile (`FASTDB_CONFIG_DIR`). Do not
  record passwords, connection strings or credential-store contents.
- Status values: `PASS`, `FAIL`, `BLOCKED` (prerequisite missing), `SKIP` (out of
  scope, with reason), `PENDING`.
- A live test that prints `ok` is evidence only if its `FASTDB_REQUIRE_*` flag
  was set or its `--nocapture` output shows the server was used.
- Do not fix during the pass. Record each failure in the findings file with
  reproduction steps. Fix in separate changes, then retest the affected cases on
  a new candidate.
- After the pass, remove containers, temporary profiles and debug artifacts.
  Keep the runnable binary and reusable build dependencies.

## 3. Environments

| Item | macOS arm64 | Linux x86_64 |
| --- | --- | --- |
| OS | macOS 15 on Apple Silicon | Ubuntu 24.04 desktop (GNOME) |
| Display | Retina 2x; 1x external display if available | X11 at scale 1 and 2 (`GPUI_X11_SCALE_FACTOR`); one Wayland session |
| GPU backend | Metal | Vulkan (`vulkaninfo --summary` lists a device) |
| Credential store | login Keychain | Secret Service (unlocked gnome-keyring); also a session without it |
| File dialogs | native panels | `xdg-desktop-portal` with a GNOME/GTK or KDE backend |
| Monospace font | Menlo | DejaVu Sans Mono (`fonts-dejavu-core`) |
| Config directory | `~/Library/Application Support/com.fastdb.FastDB` | `~/.config/fastdb` |
| Spill directory | `$TMPDIR/fastdb` | `/var/tmp/fastdb` |
| Containers | Docker Desktop, Colima or Podman (arm64 `postgres:16`, `mysql:8.4`) | Docker |
| Native clients | Homebrew `libpq` or `postgresql@16`, `mysql-client`, system `sqlite3` | `postgresql-client-16`, `mysql-client`, `sqlite3` |
| Build prerequisites | Xcode command line tools | packages in [building.md](../development/building.md) |
| Packaging tools | `python3`, optional `minisign` | `dpkg-dev`, `binutils`, `desktop-file-utils`, optional `appimagetool`, `minisign` |

## 4. Fixtures

| ID | Fixture | Used by |
| --- | --- | --- |
| FX1 | `fastdb_accept_a` / `fastdb_accept_b` from `docs/testing/0.2.0-postgres-fixture.sql` | PostgreSQL runbook |
| FX2 | PostgreSQL demo (`docs/testing/postgres-demo/seed.sql`): roles, partitions, RLS, 250,000-row table | Navigator, admin, plans, monitor |
| FX3 | MySQL 8.4 disposable schema built per the protocol in [WORKFLOW_ACCEPTANCE.md](../WORKFLOW_ACCEPTANCE.md#repeatable-desktop-protocol) | MySQL desktop cases |
| FX4 | SQLite file with the same table shapes, plus an in-memory preset | SQLite desktop cases |
| FX5 | Private CA, server and client certificates (same `openssl` steps as `ci/test-tls.sh`), TLS-only PostgreSQL and MySQL containers | TLS cases |
| FX6 | OpenSSH server container with access to the database containers; host key fingerprint from `ssh-keygen -lf` | SSH cases |
| FX7 | Restricted PostgreSQL login with schema visibility and no object privileges | Permission-denied metadata |
| FX8 | `mariadb:11.4` container | MariaDB driver tests only (desktop preset disabled) |
| FX9 | `ci/provider-containers/compose.yml` plus vendor ODBC drivers | Optional ODBC driver tests |

No MySQL or SQLite desktop fixture is checked in. Create FX3/FX4 at test time and
keep their SQL with the run evidence.

## 5. Phase A — static checks (both platforms)

| ID | Command | Pass criteria |
| --- | --- | --- |
| A1 | `cargo fmt --all -- --check` | no diff |
| A2 | `cargo clippy --locked --workspace --all-targets -- -D warnings` | exit 0 |
| A3 | `cargo build --locked --workspace --all-targets` | exit 0 |
| A4 | `cargo build --locked --release -p fastdb-app` | exit 0; executable under the 100 MiB benchmark budget |

## 6. Phase B — unit and fixture tests (both platforms)

| ID | Command | Pass criteria |
| --- | --- | --- |
| B1 | `cargo test --workspace --locked` with no `FASTDB_TEST_*` variables | 0 failed; record passed/ignored counts per crate |
| B2 | `python3 ci/test_bundle_macos.py` | all pass (fake build tools; macOS primary, try on Linux) |

B1 covers: `mem-engine` (slotted pages, LRU, LZ4 spill, load test), `db-wire`
(SQL planning and quoting, script splitting and vendor delimiters, SQLite driver,
export/import/transfer, filters, metadata capabilities, SSH host verification),
`platform-hal` (paths, keymap), `sql-intel` (parsing, aliases, completion),
`ui-core`, `ui-editor`, `ui-grid` (selection, change set), `fastdb-app`
(workspace state, connection form, updater verification, native backup command
construction, exports, diagram model) and `xtask` (benchmark policy).

## 7. Phase C — live automated tests

Run each step with its `FASTDB_REQUIRE_*` flag and `-- --nocapture`; keep logs.

Common servers:

```bash
docker run -d --name fastdb-pg -e POSTGRES_HOST_AUTH_METHOD=trust \
  -p 127.0.0.1:5432:5432 postgres:16
docker run -d --name fastdb-mysql -e MYSQL_ROOT_PASSWORD=fastdb \
  -e MYSQL_DATABASE=fastdb_test -p 127.0.0.1:3306:3306 mysql:8.4
export FASTDB_REQUIRE_DB_TESTS=1 FASTDB_TEST_PG_PORT=5432 FASTDB_TEST_PG_USER=postgres \
  FASTDB_TEST_MYSQL_PORT=3306 FASTDB_TEST_MYSQL_USER=root FASTDB_TEST_MYSQL_PASSWORD=fastdb
```

| ID | Scope | Command / setup | Pass criteria |
| --- | --- | --- | --- |
| C1 | CI parity: engine, drivers, SSH forwarding (in-process russh server) | `cargo test --locked -p mem-engine -p db-wire -p platform-hal -p sql-intel` | 0 failed |
| C2 | Provider workflow (connect, browse, run, edit, export, cancel, disconnect) | `cargo test --locked -p db-wire --test workflow_test -- --nocapture` | 3 workflows pass; output names PG and MySQL |
| C3 | App-crate live tests (not run in CI) | `cargo test --locked -p fastdb-app` with the same variables | 0 failed |
| C4 | TLS and SSH+TLS | `./ci/test-tls.sh` | exit 0; on macOS this is the only automated run of the Security-framework identity path |
| C5 | MariaDB plans | FX8 on port 3307; `FASTDB_REQUIRE_MARIADB_TESTS=1 FASTDB_TEST_MARIADB_PORT=3307 FASTDB_TEST_MARIADB_PASSWORD=… cargo test --locked -p db-wire --test query_plan_test -- --nocapture` | 0 failed |
| C6 | Native backup/restore round trips | PostgreSQL: `FASTDB_TEST_P202_PG_PORT`, `FASTDB_TEST_P202_PG_BIN` (role `p202_fixture`, database `p202_source`; optional `_TLS_CA` and `_SSH_*`). MySQL: `FASTDB_TEST_P202_MYSQL_BIN` (port 3306, `source_db`, `p202_fixture`). SQLite: `FASTDB_TEST_SQLITE_CLI_DIR`. Run `cargo test --locked -p fastdb-app native_backup -- --nocapture` | 0 failed; fixture roles per the test source |
| C7 | PostgreSQL schema/database drop | `FASTDB_TEST_NAV_DROP_PORT=5432` with role `fastdb_drop_test` allowed to create databases; `cargo test --locked -p fastdb-app postgres_live_drop -- --nocapture` | 0 failed |
| C8 | OS credential store | `FASTDB_TEST_KEYCHAIN=1 cargo test --locked -p platform-hal -- --nocapture`. Linux: run inside `dbus-run-session` with an unlocked gnome-keyring | 0 failed; no leftover test item |
| C9 | ODBC providers (optional) | FX9 per [PROVIDERS.md](../PROVIDERS.md#live-tests), `FASTDB_TEST_PROVIDER_WRITES=1` | record per provider; Oracle client is arm64, SQL Server/Db2 clients are x86_64 |

Before C6/C7, read the test bodies for the exact role and password fixtures they
expect; do not reuse those values outside disposable servers.

## 8. Phase D — packaging and installation

| ID | Platform | Steps | Pass criteria |
| --- | --- | --- | --- |
| D1 | macOS | `./bundle_macos.sh --no-install`; `(cd dist && shasum -a 256 -c SHA256SUMS.txt)`; `codesign -dv dist/FastDB.app`; `minisign -V` when signed | outputs listed in building.md exist; checksums match; ad-hoc signature present |
| D2 | macOS | Copy `FastDB.app` from the DMG to `/Applications`, add a quarantine attribute, apply the Gatekeeper command from the install guide, launch from Finder | app opens from Finder; profile is in Application Support; record Keychain prompts |
| D3 | Linux | `./bundle_linux.sh`; `ci/test-package-deb.sh dist/linux/fastdb-linux-x86_64` | binary and `.deb` built; package test passes |
| D4 | Linux | `sudo apt install ./<deb>`; launch from the desktop launcher; `sudo apt remove fastdb` | desktop entry and icon appear; app starts; removal is clean |
| D5 | Both | Compare the About version with `[workspace.package] version` | equal |

Tagging and the release workflow are outside this pass.

## 9. Phase E — manual desktop acceptance

Launch the candidate with a fresh `FASTDB_CONFIG_DIR`, with the OS credential
store enabled unless a case says otherwise. Use `Ctrl` for `Cmd` on Linux.

### E0. Existing runbooks (both platforms)

1. Run every case of the
   [v0.2.0 PostgreSQL runbook](../testing/0.2.0-postgres-manual-acceptance.md)
   (C01–C04, B01–B04, M01–M06, G01–G06, S01–S07, T01–T05, D01–D04, V01–V03,
   X01–X06, L01–L04, R01–R05, W01–W07) with FX1.
2. Run the six-step [desktop protocol](../WORKFLOW_ACCEPTANCE.md#repeatable-desktop-protocol)
   and cross-cutting cases for MySQL (FX3) and SQLite (FX4).
3. Run the [backup and restore checklist](../testing/p2-02-manual-acceptance.md)
   for PostgreSQL, MySQL and SQLite.

The cases below cover features and platform behavior those runbooks do not.

### E1. Application shell

| ID | Check | Platform notes |
| --- | --- | --- |
| APP-01 | Cold launch with an empty profile: centered 1280×800 window, empty workspace, no banner | Release build; compare with the 2 s startup budget |
| APP-02 | Double-click the title or empty top bar to maximize and restore; tabs, close buttons and the update badge keep their click actions | Linux: X11 and Wayland |
| APP-03 | Theme toggle, sidebar toggle, editor zoom in/out/reset; settings persist across restart | |
| APP-04 | Open About (Close, `Esc`, `Cmd/Ctrl+W`) and Check for Updates | macOS: FastDB menu. Linux: menus are defined only with `cx.set_menus` in `crates/app/src/main.rs`; confirm an entry point exists. If none is reachable, record `FAIL` |
| APP-05 | Quit by shortcut, menu and window close; with pending script save, staged grid edit, dirty column draft and failed connection save | Extends runbook L03/L04 |
| APP-06 | HiDPI rendering: text, SVG provider logos, grid lines, diagram | macOS: move the window between Retina and 1x displays. Linux: scale 1 and 2 |
| APP-07 | Plain-text result view uses the platform monospace font and stays aligned | Linux: repeat once without `fonts-dejavu-core`; record the fallback |
| APP-08 | Unicode and IME input (Vietnamese with diacritics, CJK, emoji) in the SQL editor, connection fields, filter bar and cell editor; no duplicated or lost characters during composition | macOS: system Vietnamese input source. Linux: IBus (Unikey or Bamboo) |
| APP-09 | Clipboard round trips with an external editor for SQL, grid copies, DDL and record copies | Linux: X11 and Wayland |
| APP-10 | Every native file dialog: export, import, SQL import/save copy, plan open/save, value load/save, SVG export, backup output, client directory; cancel writes nothing | Linux: with portal running, then with the portal stopped (expect an error, no hang) |
| APP-11 | Kill the process (`kill -9`) during a session, restart: transaction-log temp file and spill files are cleaned up; autosaved scripts remain | |
| APP-12 | Spill files appear under the spill directory during a large result and are removed after close/quit | Linux: run as a second OS user after the first created `/var/tmp/fastdb`; record permission behavior |

### E2. Connections

| ID | Check | Platform notes |
| --- | --- | --- |
| CON-01 | MySQL profile: invalid password, closed port, Test, Connect, restart, reconnect | |
| CON-02 | SQLite profiles: existing file, new file, in-memory preset isolation, read-only on a missing file is rejected | |
| CON-03 | Unix socket discovery for PostgreSQL and MySQL; enabling TLS forces TCP | macOS: `/tmp/.s.PGSQL.*`. Linux: `/var/run/postgresql`, `/run/postgresql` |
| CON-04 | TLS with system trust, PEM CA, DER CA; untrusted certificate, wrong hostname and plaintext-only server are rejected (FX5) | |
| CON-05 | TLS client identity (.p12/.pfx) with and without password; wrong password; certificate-only archive rejected; clearing removes it | macOS validates with the Security framework, Linux with OpenSSL; compare messages |
| CON-06 | SSH with password and with key plus passphrase; wrong fingerprint rejected; SSH plus TLS checks the database hostname (FX6) | PostgreSQL and MySQL |
| CON-07 | Read-only profiles block grid edits, schema changes and transfer destinations; navigator shows the mode | PostgreSQL, MySQL, SQLite |
| CON-08 | Navigator groups and colors; invalid hex is rejected | |
| CON-09 | Timeout 1–120 s; an unroutable host fails at the configured deadline | MySQL (runbook C03 covers PostgreSQL) |
| CON-10 | Credential store: save, restart, reconnect without re-entry; locked store; unavailable store shows the save-failure banner with retry and blocks Quit until a retry succeeds | macOS: record prompts after rebuild (ad-hoc signature). Linux: also a session without Secret Service |
| CON-11 | `FASTDB_LOCAL_CREDENTIALS=1`: files under `<profile>/local-credentials`, directories 0700, files 0600, survive restart; switching modes copies nothing | |
| CON-12 | Legacy plaintext `connections.json` and `.fastdb_config.json` migrate; source kept when secure storage fails | |
| CON-13 | Deleting a connection removes its secrets and saved diagram positions | |
| CON-14 | Dialog layout: two-step frame, Main/SSH/TLS/Advanced tabs, Tab/Shift+Tab order, Copy details on long errors, small window | |

### E3. Navigator and administration

| ID | Check | Providers |
| --- | --- | --- |
| NAV-01 | Tree folders (Tables, Views, Materialized Views, Other) with columns, constraints, foreign keys and indexes | PG, MySQL, SQLite |
| NAV-02 | Search modes (Connections, Catalogs and schemas, Tables/procedures, Files), 100-result cap, clearing restores expansion | all |
| NAV-03 | Catalog browser and favorites, including missing-object feedback; non-PostgreSQL shows unsupported | PG; others for the unsupported state |
| NAV-04 | Reveal active object for tables, views and column tabs | all |
| NAV-05 | Refresh at each node level; Create Table / Create View dialogs on writable schemas | all |
| NAV-06 | Drag the Connections divider; width persists after restart | all |
| NAV-07 | Session Monitor with an advisory-lock chain (FX2 README); Cancel and Terminate confirmations; read-only profile inspects only | PG |
| NAV-08 | Administration families (routines, sequences, triggers, rules, policies, partitions, roles, privileges); reader-role permission errors keep the SQL | PG |
| NAV-09 | Native client discovery for backup tools; Client Directory override | macOS: Homebrew paths. Linux: `/usr/lib/postgresql/16/bin`, `/usr/bin` |

### E4. Table tabs, grid and Properties

| ID | Check | Providers |
| --- | --- | --- |
| GRID-01 | Stream 250,000 rows (FX2) and 1,000,000 rows; resident memory stays bounded (record RSS) | PG, SQLite |
| GRID-02 | Cancel a long read: PostgreSQL cancel token, MySQL `KILL QUERY`, SQLite interrupt; a fresh read then works | all |
| GRID-03 | Search table data with each provider's matching rules and row preview | PG, MySQL, SQLite |
| GRID-04 | Find in result: wrap, staged values, cancel, 4,096-byte limit | all |
| GRID-05 | Column menu, multi-key sort, Criteria with text literals, filter history | MySQL, SQLite (runbook covers PG) |
| GRID-06 | TSV paste limits (1,024 cells, 256 KiB), Set SQL NULL, Use database default | all |
| GRID-07 | Related rows and Choose key, composite keys; MySQL text-key limit | all |
| GRID-08 | Record view with Grid and Text, Tab toggle, Escape, paging of 64 fields | all |
| GRID-09 | Copy TSV/CSV/SQL INSERT into an external editor; 16 MiB limit message | all |
| GRID-10 | Table transactions in Auto and Manual mode: Save, Commit, Rollback, failed batch recovery; independent read-back | MySQL, SQLite (runbook W03 covers PG) |
| GRID-11 | Export CSV, SQL, HTML, Markdown, JSON and Export All (CSV/SQL) with Cancel; failed write keeps the previous file; validate files with external tools | all |
| GRID-12 | CSV import: delimiters, header, UTF-8/UTF-16 LE/BE, NULL token, conversions, both commit policies, skip invalid, cancel | all |
| GRID-13 | Transfer across connections and providers (PG→MySQL, MySQL→SQLite, SQLite→PG); self-transfer and write statements rejected | all |
| GRID-14 | Properties categories and capability states: disconnected, not implemented, unsupported, permission denied (FX7), loaded empty | all |
| GRID-15 | ER diagram: focus layout, card drag and persistence, link selection, SVG export opened in a browser | all; zoom by pinch and `Cmd`+wheel on macOS, `Ctrl`+wheel and `Ctrl`+plus/minus on Linux; middle-drag pan |
| GRID-16 | Value viewer and editor: text/JSON/XML/hex, 256 KiB limit, file load/save encodings, UTC display toggle | all |
| GRID-17 | Plain-text view: 2,000-row and 255-character preview limits, `[NULL]` | all |

### E5. SQL editor and execution

| ID | Check | Providers |
| --- | --- | --- |
| SQL-01 | Script files: autosave location, chooser and all-connections list, rename collision, delete to `.trash`, Files search | all |
| SQL-02 | Execute selection, statement at caret (CTE, UNION, INSERT … SELECT, consecutive SELECTs without semicolons) and Run script | MySQL, SQLite (runbook S01/S05 cover PG) |
| SQL-03 | Batch syntax: MySQL `DELIMITER`, SQLite trigger bodies, PostgreSQL dollar quotes | per provider |
| SQL-04 | Tab sessions: MySQL temp tables, user variables and implicit DDL commit; SQLite private TEMP and per-preset in-memory database | MySQL, SQLite |
| SQL-05 | Database…, Schema…, Isolation… and Autocommit… dialogs with native read-back | per provider support |
| SQL-06 | Timeout presets against `pg_sleep`, `SLEEP()` and a long SQLite recursive CTE | all |
| SQL-07 | Execution log and provider feedback: PostgreSQL notices and error positions, MySQL warnings, SQLite row counts | all |
| SQL-08 | Transaction log modal paging and search; file removed on normal exit | all |
| SQL-09 | Plans: Explain, Analyze (MySQL/PG), SQLite EXPLAIN QUERY PLAN, expensive-node navigation, Save/Open/Compare plan JSON | all |
| SQL-10 | Parameter prompt with each value type; MySQL `@name`/`@@name` stay native | all |
| SQL-11 | Completion with aliases, snippets, Format, Find/Replace, comment/indent, Import/Save SQL copy (16 MiB) | all |
| SQL-12 | History persistence and the 64 KiB omission | all |
| SQL-13 | Load editable result: allowed columns, rejected forms, conflict rollback | PG |
| SQL-14 | Result tabs (latest 20), × close, editor/result split bounds 15–85% | all |
| SQL-15 | Shortcut matrix in [shortcuts.md](../guide/shortcuts.md); note collisions with OS shortcuts | macOS and Linux (GNOME) |

### E6. Software updates

| ID | Check | Platform notes |
| --- | --- | --- |
| UPD-01 | Startup check runs at most once per 24 h | |
| UPD-02 | Manual check when up to date | macOS: native alert. Linux: depends on APP-04 |
| UPD-03 | `FASTDB_OVERRIDE_LATEST_VERSION=0.9.0` shows the badge and release-notes modal | The override builds an arm64 DMG asset name; on Linux record which asset is offered |
| UPD-04 | Download from a signed release: progress, Minisign and SHA-256 verification, tampered file rejected, Retry, Open Installer, Show in Folder | macOS: `.dmg`. Linux: AppImage preferred, then `.deb` |
| UPD-05 | GitHub API unavailable (blocked host or 403) falls back to the latest-release redirect | |

### E7. Performance

| ID | Check | Platform notes |
| --- | --- | --- |
| PERF-01 | `cargo xtask benchmark` with `benchmarks/thresholds.json`, baseline then candidate | macOS on a fixed display |
| PERF-02 | Same with `benchmarks/thresholds-linux.json` per [benchmarks/README.md](../../benchmarks/README.md#linux-release-policy) | Linux X11 |
| PERF-03 | One Wayland run; record stalls | Linux (known open item) |

## 10. Recording defects

Record failures in `docs/testing/2026-11-23-test-findings.md`, one row per case:

| Field | Content |
| --- | --- |
| ID | `F-<nn>` |
| Case | Plan case ID (for example `C3`, `APP-04`) |
| Platform / build | OS, architecture, commit |
| Steps | Minimal reproduction, without secrets |
| Expected / actual | Short statements |
| Evidence | Log or screenshot path |
| Severity | Blocker, Major, Minor |
| Status | Open, Fixed in `<commit>`, Retest pass |

Use the runbooks' own result templates for E0 cases and copy only failed or
blocked rows into the findings file.

## 11. Order and exit criteria

1. Phases A–C on Linux, then macOS.
2. Phase D on each platform.
3. Phase E: E0 PostgreSQL runbook first (release gate), then MySQL/SQLite and
   backup checklists, then E1–E7.
4. Retest failed cases on the fixed candidate; finish with APP-01 and APP-05.

The pass is complete when A–D pass on both platforms, every case has a status,
no runbook P0-05 case is `FAIL`, and each `BLOCKED` or `SKIP` has a reason.
