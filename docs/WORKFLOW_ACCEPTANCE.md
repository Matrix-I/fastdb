# Provider workflow acceptance

Current release scope (2026-11-05): **PostgreSQL acceptance is required**. Follow
[the ordered v0.2.0 PostgreSQL runbook](testing/0.2.0-postgres-manual-acceptance.md).
MariaDB, Oracle, SQL Server and Db2 are disabled with **Feature is coming**.
MySQL and SQLite are outside this manual release gate. The automated data
workflow covers all three enabled providers. Record desktop results for the
build under test using the [case record](#desktop-case-record). Historical
provider runs are recorded in [PROVIDERS.md](PROVIDERS.md).
For the broader Linux run, see the [full-feature acceptance results](testing/2026-11-22-full-feature-acceptance-results.md).

## Provider matrix

| Provider | Connect/browse/run | Edit/save/read-back | Export/read-back | Cancel/disconnect | Desktop status |
| --- | --- | --- | --- | --- | --- |
| PostgreSQL | Automated live: connect, metadata, stream, filter/order and script failure | Automated live: change/insert/delete, row-count and FK failure rollback, fresh-connection read-back | Automated live: exact CSV bytes and SQL replay | Automated live: native cancel, fresh read and rollback releases writer | [Linux X11 L03/L04 smoke complete](#postgresql-desktop-smoke-follow-up-2026-11-23). Full acceptance pending. |
| MySQL | Automated live (same cases) | Automated live (same cases) | Automated live (same cases) | Automated live (same cases) | Pending |
| MariaDB | Pending separately from MySQL | Pending | Pending | Pending | Pending |
| SQLite | Automated file fixture: connect, metadata, stream and script failure | Automated: change/insert/delete, atomic rollback and read-back | Automated: exact CSV bytes and SQL replay | Automated: native interrupt, fresh read, rollback releases writer and reopen | Pending (manual run previously skipped) |
| SQL Server | Pending; vendor ODBC required | Pending | Pending | Pending | Pending |
| Oracle | Pending; vendor ODBC required | Pending | Pending | Pending | Pending |
| Db2 LUW | Pending; vendor ODBC required | Pending | Pending | Pending | Pending; previous live validation incomplete |

## Repeatable desktop protocol

Use an isolated app profile (`FASTDB_CONFIG_DIR`) and disposable databases. Record
the git commit, OS/architecture, display scaling, provider/server/client versions,
fixture name and evidence location. Never record credentials or connection strings.
Create fixtures with a primary key, nullable text, empty text, Unicode, embedded
quotes/newlines, an exact decimal and a foreign key; use native provider types.

1. **Connect:** save a preset, test, connect, restart and reconnect. Try an invalid
   endpoint and invalid credentials. Expect actionable errors without secrets.
2. **Browse:** open the fixture, change fetch size, fetch next, filter/order and
   refresh. Compare displayed rows to an independent SQL client. Check loaded
   empty, unsupported metadata and permission-denied states separately.
3. **Run:** execute a selection, statement at caret and sequential script. Include
   a later failing statement; earlier results must remain distinguishable from
   failed work. Confirm the script uses its own connection identity.
4. **Edit/save/read-back:** change, insert, duplicate and delete fixture rows.
   Save, then read from a fresh independent connection. Force a constraint error
   in a multi-row save and verify atomic rollback. Cancel another staged edit
   and verify that it never reached the database. Use explicit values where
   provider default-only inserts are unsupported.
5. **Export/read-back:** export loaded rows as CSV and SQL to fresh files. Parse
   CSV and compare headers/counts/quoted text. CSV currently conflates NULL with
   empty fields; record that limit. Execute SQL into a disposable compatible
   target and compare values/counts. Do not describe this as a binary backup or
   full-source export. Verify destination errors leave previous files intact.
   Full-source Export All must also be exercised separately from loaded export;
   see the runbook for filtered counts, cancellation and transfer UI gaps.
6. **Cancel/disconnect:** cancel a long read, wait for feedback, then execute a
   fresh read. Disconnect during loading and verify stale completions cannot
   resurrect the session or overwrite another tab.

## Cross-cutting cases

- Open identical schema/table names on two connections with different fixture
  values. Switch tabs during metadata loads, queries and saves; results and
  errors must remain attached to the original owner.
- Close/reopen a tab during asynchronous completion. Late results must not target
  a reused position in the tab list.
- Stage an edit, then request refresh, filter/history, close, disconnect,
  reconnect on the same profile and Quit. Verify the owning grid keeps the
  draft until Save or Cancel. Repeat a departure with an active cell editor,
  then check that a clean tab on another connection can still close. Verify
  dirty column drafts prompt for Save, Discard or Cancel.
- PostgreSQL: use two script runs to create/use a temporary table; Begin, write,
  fail a statement, Rollback and read independently. Repeat with a savepoint.
  Other providers have session implementations, but their desktop/live acceptance
  is outside the current PostgreSQL release gate.
- Columns: inspect long native types/defaults/comments at narrow and HiDPI
  sizes; select a row, copy full details, double-click and press Enter. Repeat
  on same-named objects across connections and after a column disappears on
  refresh. Unknown fields must not be invented.
- Quit: Cmd/Ctrl+Q and menu Quit from table, script, connection and About dialogs;
  repeat with pending saves, save failures and dirty column drafts. Failed saves
  must keep the app open with feedback.

## Desktop case record

For PostgreSQL, use the ordered runbook's [P0-05 release decision
criteria](testing/0.2.0-postgres-manual-acceptance.md#release-decision-and-result-template).
Use the workflow names for MySQL and SQLite and record provider-specific fixture
limits in the results.

| Workflow | PostgreSQL runbook cases |
| --- | --- |
| Connect | C02, C04 |
| Browse | B01–B03 |
| Run | S01, S05 |
| Edit, save and read back | G01–G03 |
| Export and read back | X01–X06 |
| Cancel and disconnect | S06, L02 |
| Connection ownership and stale completion | L01, L02 |
| Pending edits and Quit | L03, L04 |
| Failed transaction recovery | G03, T02, T03 |

Copy one result row per provider and case. Use `Pending` until attempted,
`Pass` when every applicable check matches, `Fail` for a mismatch and
`Blocked` if a fixture or prerequisite prevents execution.

| Provider / version | Build / OS / scale | Case | Expected outcome | Actual outcome | Status | Evidence / limitation |
| --- | --- | --- | --- | --- | --- | --- |
| | | | | | Pending | |

### PostgreSQL desktop smoke (2026-11-22)

FastDB 0.3.0 at `359502a`, Linux x86_64, X11 via XWayland at 150% scale,
PostgreSQL 16.16. The run used an isolated FastDB profile and two disposable
databases loaded with the [acceptance fixture](testing/0.2.0-postgres-fixture.sql).

| Case | Observed result | Status |
| --- | --- | --- |
| L03 · Close with staged grid edits | `Ctrl+W` left the owning Data tab open, retained the staged cell and showed “Save or Cancel row edits before continuing.” Undo restored the original cell, and retrying `Ctrl+W` closed the tab. | Pass |
| L03 · Quit with staged grid edits | `Ctrl+Q` left FastDB and the owning tab open with the draft intact. Independent `psql` reads returned `1\|original` in both fixture databases. After the draft was undone, `Ctrl+Q` exited during the L04 script check. | Pass |
| L03 · Remaining runbook steps | Refresh, Criteria/history, disconnect, profile deletion and reconnect, departure with an active cell editor, closing a clean tab on another connection, and the column-draft prompt await exercise. | Pending |
| L04 · Script close, Quit and reopen | Typing `select 42;` and immediately pressing `Ctrl+W` closed the script tab and saved the exact bytes. Reopening the script, replacing the text with `select 43;` and immediately pressing `Ctrl+Q` saved the exact bytes and exited. The editor displayed `select 43;` on relaunch. | Pass |
| L04 · Menu, window-close and other focus contexts | Menu Quit, window close and the remaining focus contexts await exercise. | Pending |
| L04 · GUI save failure and retry | The isolated script directory was made unwritable and its permissions were restored. After relaunch, keyboard input did not reach the SQL editor, so the attempted edit never triggered a save. Failure feedback and retry still require a GUI check. | Blocked |

These results complete the keyboard smoke subcases. The full L03 and L04
runbook cases and PostgreSQL release acceptance remain open.

### Automated smoke follow-up (2026-11-23)

FastDB 0.3.0 at `359502a` on Linux x86_64.

- `cargo test --workspace --locked --offline` passed 834 harness tests across
  42 suites, including all four script-save tests for the departure guard,
  atomic replacement and retry. Tests requiring unconfigured live providers
  returned early. The initial sandboxed run had 16 spill-file failures because
  `/var/tmp/fastdb` was read-only and one SSH test failed to bind loopback. All
  17 passed when the suite was rerun with access to those resources.
- With both disposable loopback servers configured,
  `FASTDB_REQUIRE_DB_TESTS=1 cargo test -p db-wire --test workflow_test --locked --offline -- --nocapture`
  passed the three PostgreSQL 16.16, MySQL 8.4.13 and SQLite file workflows.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked --offline`
  and `cargo build --workspace --locked --offline` passed. Cargo reported
  existing unused-dependency warnings.

### PostgreSQL desktop smoke follow-up (2026-11-23)

This smoke run is scoped to Linux X11. Native macOS routes are deferred from
this run. Native Wayland was outside the tested configuration.

FastDB 0.3.0 at `359502a`, Linux x86_64, X11 via Xvfb at 1150×760,
PostgreSQL 16.16. Pointer and keyboard input went through the app on an
isolated display with a disposable profile and databases. Row values were
read separately with `psql`.

| Case | Observed result | Status |
| --- | --- | --- |
| L03 · Grid result replacement | Refresh and Criteria showed Save or Cancel feedback. History Back was disabled during the draft and available again after Save or Cancel. The draft stayed on Accept A. | Pass |
| L03 · Departure and reconnect | Disconnect and profile deletion were refused. Connect Database in Edit Connection showed a pending-edits error with the owning Data tab selected behind the dialog. Closing B's clean tab left A's draft intact. `Ctrl+W` with the cell editor active kept the typed value and editor open. | Pass |
| L03 · Cancel, retry and Save | Cancel restored the original value and allowed Refresh and Disconnect. A later reconnect succeeded and `psql` still returned `1\|original`. Before the separate Save, A and B both returned `1\|original` and `2\|second`. Save changed A's ID 1 to `saved-a` while ID 2 and both rows on B kept their original values. | Pass |
| L03 · Column draft | Closing the column tab offered Save, Discard and Cancel. Cancel kept the column view and `smoke-draft` comment. Discard on a second close dismissed the draft. | Pass |
| L04 · Save failure and retry | With the isolated script directory unwritable, `Ctrl+Q` showed `Permission denied`, left `select 143;` in the editor and preserved the file's exact `select 142;` bytes. Restoring permissions and retrying Quit exited, wrote the exact new bytes and displayed `select 143;` after relaunch. | Pass |
| L04 · Linux Quit and window close | `Ctrl+Q` quit from the table, SQL editor and connection dialog. X11 `WM_DELETE_WINDOW` closed FastDB from all three contexts. Closing the editor immediately after typing persisted the exact `select 144;` bytes. | Pass |

The old “Save or Cancel row edits before continuing” banner stayed visible after
Cancel, successful Disconnect and reconnect. The grid was clean and `psql`
confirmed the original values.

See the [L03/L04 runbook](testing/0.2.0-postgres-manual-acceptance.md#10-connection-ownership-async-lifecycle-and-dirty-guards--p0-0508)
for the case definitions. Full PostgreSQL release acceptance remains open.

## Automated evidence boundaries

`cargo test -p db-wire --test workflow_test --locked` exercises a temporary
SQLite database file. It pings, reads metadata, streams rows, stops a sequential
script after its failing statement, plans an update/insert/delete, and tests
atomic rollback on a later failed batch. It checks exact CSV bytes and replays
SQL into an independent database. A separate same-named table stays unchanged.
After interrupting a long read the test reads the saved rows again, verifies
that closing the tab session releases the writer, and reopens the file to
read back committed edits.

The same suite has `postgres_live_workflow` and `mysql_live_workflow`, which run
when `FASTDB_TEST_PG_PORT` / `FASTDB_TEST_MYSQL_PORT` are set (plus optional
`_HOST`, `_USER`, `_PASSWORD`). Each run recreates two disposable databases,
`fastdb_wf_a` and `fastdb_wf_b`, so the account needs `CREATE DATABASE`. It covers
the following steps in order:

1. **Connect:** a ping succeeds and a refused port returns an error.
2. **Browse:** metadata, a streamed read, and a filtered/ordered read.
3. **Run:** a script whose failing second statement stops the run while the
   first result stays separate.
4. **Edit/save:** a planned change, insert and delete. Row-count and
   foreign-key failures roll back atomically, and the data is read back
   through a fresh connection.
5. **Export:** exact CSV bytes, and SQL INSERTs replayed into another table.
6. **Identity:** the same-named table in the other database is unchanged.
7. **Cancel:** a long read reports the provider's native interruption, followed
   by a fresh read on the same connection.
8. **Disconnect:** closing a tab session rolls back its open transaction. The
   pinned session can see its write, another connection cannot see it before
   close, and that connection can insert the same primary key after close.

The workspace unit tests cover operation ownership and stale generation
rejection across two distinct connections, and the save gate's rejection of
departures with pending grid edits. See the desktop smoke record above for the
keyboard routes exercised on Linux. MariaDB, ODBC and TLS require separate fixtures.
See [live runs and server versions](PROVIDERS.md#workflow-acceptance-2026-11-22).

`cargo test --workspace --locked` runs deterministic unit tests and SQLite
integration tests. Live tests often return early when their `FASTDB_TEST_*`
variables are unset; a harness `ok` line is not proof that a server was exercised.
Capture `--nocapture` output and configure required-test flags for actual provider
validation, as described in [PROVIDERS.md](PROVIDERS.md).

Relevant suites include `wire_test` (SQLite streaming, cancellation and atomic
edits), `filter_test`, `metadata_capabilities_test`, `postgres_test`, `mysql_test`,
`script_session_test`, `catalog_test`, `column_test`, `provider_test` and `tls_test`.
Export unit tests include SQLite SQL read-back and writer-error checks. None of
these alone verifies focus routing, clipboard integration or desktop layout.
