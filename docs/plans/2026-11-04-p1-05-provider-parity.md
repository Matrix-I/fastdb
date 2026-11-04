# P1-05 provider and transaction parity

## Scope

Complete the remaining P1-05 work: database/schema context controls, dedicated
ODBC tab sessions, provider transaction controls and dialect-aware script batches.
Preserve existing PostgreSQL, MySQL/MariaDB and SQLite behavior. Do not commit or
push in this task. The earlier P1 plan records authorization for necessary API
changes; no dependency or persisted configuration changes are planned.

## Delivery and verification

1. Add test-first context planning and same-session schema read-back. Database
   changes create an empty SQL tab with a distinct driver/session; schema changes
   require a known idle transaction. Revalidate tab/driver ownership after prompts.
2. Add dialect fixtures for vendor batch delimiters and procedural bodies; wire
   the same provider policy through execution and parameter preflight.
3. Add isolated ODBC sessions with cancellation, close/rollback ownership and
   truthful transaction-state feedback, verified against available vendor servers.
4. Update SQL editor/provider guides and roadmap with actual support and limits.
5. Read the complete diff, run formatting, workspace tests/build/Clippy, exercise
   real provider flows and the application, then obtain independent review.

Missing vendor drivers/servers or desktop acceptance must be reported explicitly;
unchecked acceptance never becomes a supported-provider claim.

## Implementation and acceptance

Database selection opens a temporary editor with a separate driver configured
for the selected database. The first execution opens its physical session.
The tab has no autosaved file because reopening a file owned by the original
connection could silently target its original database. Save SQL copy remains
available. The original tab and session keep their transaction and temporary
objects. Disconnect or replacement invalidates the temporary tab's driver.
Schema selection requires idle state and checks that the provider confirms the
selected schema. MySQL/MariaDB uses the database selector because its database
and schema names identify the same context.

PostgreSQL live context integration passed. It changed the search path and read
back the selected schema, opened a second physical session in another database,
and verified that the first session still read an uncommitted temporary row that
the second could not access. Catalog plans on the small PostgreSQL 16 fixture
used a scan of three database entries and four schema entries, then a 25 kB sort.
Execution measured 0.054 ms and 0.028 ms respectively. These are fixture
measurements. Large catalog and other-provider plans remain unverified.

Vendor parser fixtures cover whole procedural bodies, terminators inside quotes,
malformed directives, and rejection of replay counts. The opt-in provider
integration test additionally checks savepoint rollback, commit and rollback
on session closure through a separate connection. SQL Server also checks that
a rejected Begin under implicit mode and XACT_ABORT leaves pending work intact.

Local Oracle failed before accepting connections with ORA-27104 both at 512 MiB
and 2 GiB shared memory. The Podman VM itself has 1 GiB of RAM. Its memory
configuration was left alone pending approval for the VM restart. SQL Server
Developer and Db2 Community profiles require their respective vendor terms to
be accepted. The live ODBC checks, separate MariaDB validation and desktop
acceptance remain open, so the roadmap milestone remains unchecked.

## Self-review

1. Correctness. Context changes use stable tab and driver identity, an idle-state
   guard, and native read-back. Vendor failure and cancellation still need live ODBC tests.
2. Performance. Catalog discovery returns at most 65 rows per request. PostgreSQL
   fixture plans are recorded above. Other provider catalog plans remain open.
3. Security. Context identifiers use the provider quoting helper, database names
   travel in driver options, and temporary tabs reuse existing credential handling.
4. Robustness. Cancellation discards the private session. Query and cleanup waits
   are bounded, but an ODBC vendor can retain a blocking worker until it returns.
5. Syntax and naming. Cargo formatting and workspace Clippy are final gates.
6. Modern APIs. The implementation uses the pinned ODBC library's timeout,
   transaction and result APIs. Its raw result advance is limited to zero-column
   SQL Server batch results where no Rust cursor or bound buffers remain.
7. Business clarity. Temporary tabs use Save SQL copy. Provider selector
   limitations and rejected batch directives are documented in the SQL guide.
8. Test coverage. Parser fixtures and PostgreSQL integration cover the main
   paths. SQL Server, Oracle and Db2 live tests plus desktop acceptance remain pending.
9. Design. Dialect splitting, context discovery and ODBC session ownership live
   in separate modules. The existing script runner owns execution and cancellation.

Independent review covered the working diff and found issues in comment-aware
transaction boundaries, native variables, implicit transactions, cancellation,
schema read-back and stale driver ownership. The fixes are included. The SQL
Server safety regression requires its live vendor fixture.

The user deferred live ODBC testing to Linux and chose to keep the local VM
unchanged. Follow [provider setup](../PROVIDERS.md) on Linux and use the matching
ODBC library architecture. Set the provider's FASTDB_TEST variables and run:

```bash
FASTDB_TEST_PROVIDER_WRITES=1 cargo test -p db-wire --test provider_test --locked -- --nocapture
```

Run against disposable databases. The SQL Server and Db2 profiles remain
deferred along with Oracle. P1-06 work can proceed independently.
