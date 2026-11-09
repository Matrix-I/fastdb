# Provider workflow acceptance

Current release scope (2026-11-05): **PostgreSQL acceptance is required**. Follow
[the ordered v0.2.0 PostgreSQL runbook](testing/0.2.0-postgres-manual-acceptance.md).
MariaDB, Oracle, SQL Server and Db2 are disabled with **Feature is coming**.
MySQL/SQLite are outside this manual release gate. Historical matrix rows below
retain their evidence status; no manual case is marked passed by this update.
This matrix is a test protocol, not evidence of successful desktop workflows.
Historical provider runs are recorded in [PROVIDERS.md](PROVIDERS.md); they do
not replace acceptance on the current build.

## Provider matrix

| Provider | Connect/browse/run | Edit/save/read-back | Export/read-back | Cancel/disconnect | Desktop status |
| --- | --- | --- | --- | --- | --- |
| PostgreSQL | Automated live (15.19, 2026-11-09) | Automated live: change/insert/delete, row-count and FK failure rollback, fresh-connection read-back | Automated live: exact CSV bytes, SQL replay read-back | Automated live: cancel + fresh read, tab-session close rolls back | Pending |
| MySQL | Automated live (8.4.13, 2026-11-09) | Automated live (same cases) | Automated live (same cases) | Automated live (same cases) | Pending |
| MariaDB | Pending separately from MySQL | Pending | Pending | Pending | Pending |
| SQLite | Automated in-memory browse/run | Automated planned edit and atomic rollback | Automated CSV output and SQL read-back | Existing driver cancellation tests; desktop disconnect pending | Skipped by user |
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
- Stage edits, then request refresh, filter/history, close, disconnect and Quit.
  Confirm the documented guard and verify no silent loss or unintended save.
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

## Automated evidence boundaries

`cargo test -p db-wire --test workflow_test --locked` exercises one connected
in-memory SQLite fixture from metadata/streaming through planned edits, a failed
multi-statement transaction, read-back, exact CSV bytes and SQL export into an
independent database. It also verifies that a same-named table on another
connection stays unchanged. It does not prove desktop routing, filesystem
reconnection, cancellation/disconnect or other-provider compatibility.

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
7. **Cancel:** a long read is cancelled, followed by a fresh read on the same
   connection.
8. **Disconnect:** closing a tab session with an open transaction leaves no
   committed row.

It does not prove desktop routing, stale UI completions, MariaDB or ODBC
providers. Validation on 2026-11-09 (commit after `5070f94`, macOS 27.2 arm64)
used a local PostgreSQL 15.19 cluster and a `mysql:8.4` (8.4.13) container. Both
tests passed. A mutated expectation made both fail, confirming that the
assertions were exercised.

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

For each completed manual case record: provider, case, expected outcome, actual
outcome, pass/fail/blocked, evidence path, and limitations. Leave unchecked cases
pending rather than treating unavailable credentials/drivers as passing results.
