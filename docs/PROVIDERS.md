# Provider setup and validation

## v0.2.0 desktop availability

PostgreSQL is the required release acceptance provider. MariaDB, Oracle, SQL
Server and Db2 are disabled in the desktop connection dialog with **Feature is
coming**, including Test/Connect for saved profiles. MySQL and SQLite remain
available. The driver setup and test protocols below remain useful for future
provider validation; they do not override this desktop availability policy.

## PostgreSQL metadata

Properties loads PostgreSQL catalog details in the background on connection and
schema refresh. It includes owner, OID, effective tablespace (including the database
default), comments, persistence, RLS/forced RLS, partition keys, ordered constraint
columns, foreign key references/actions/deferrability, and index methods,
expressions, INCLUDE columns, uniqueness, validity and definitions/predicates.
Column types retain precision/length and column comments and PK/FK membership.

Rows are estimates from `pg_class.reltuples`; never-analyzed values remain unknown.
Storage sizes use `pg_table_size`, `pg_indexes_size` and `pg_total_relation_size`;
table bytes include TOAST and auxiliary storage. Sizes apply to the relation itself.
Partition parents and views show unavailable sizes, not totals of their children.
Vacuum/analyze timestamps come from `pg_stat_all_tables` and may be unknown or
reset. These are read-only catalog queries; FastDB does not run COUNT, ANALYZE or
VACUUM to populate Properties. Catalog visibility follows `information_schema`
for its supported relation kinds; materialized views use native ownership and
SELECT/column-grant visibility, including restricted column lists.
Metadata failures appear in the Properties header and Statistics section without
preventing table browsing.

### PostgreSQL DDL

Properties → DDL / Copy DDL exports executable structural SQL from one catalog
query snapshot during metadata refresh. It supports ordinary and partitioned
tables, partition attachment, zero-column tables, views and materialized views.
Materialized views retain their query, native columns, storage settings, indexes,
comments and grants; scripts use WITH NO DATA and require an explicit refresh
after replay. Output preserves:

- Native types/typmods, collations, defaults, NOT NULL, stored/virtual generated
  expressions and ALWAYS/BY DEFAULT identity options and sequence names.
- Owned serial sequences (type, increment, limits, start, cache and cycle), with
  ownership and comments. Current sequence values are intentionally excluded.
- PK/unique/check/exclusion/foreign-key definitions, validation and deferrability;
  standalone expression/partial/INCLUDE indexes without duplicating constraint
  indexes. Invalid indexes are warned about and omitted.
- Table/view/column/constraint/index/sequence comments, persistence, table access
  method, explicit tablespace, relation options, column storage/compression and
  statistics targets, extended statistics, replica identity and ownership.
- RLS flags/policies, user triggers and rules (including enable modes), explicit
  table/column/sequence grants and grant options. ACL grantor identities and
  target-environment default privileges are not reconstructed.

Schemas, roles, extensions, collations, custom types, functions, external/shared
sequences, referenced tables and partition parents must already exist. Create
partition parents before children. An attached partition may inherit constraints,
indexes and triggers from its parent. The script captures the source search path
for deparsed expressions and assumes compatible PostgreSQL and dependency
versions. This is single-object structural SQL, not a full database dump: data,
sequence current values, child partitions, incoming foreign keys, publications,
security labels and extension membership are outside its scope. Typed tables use
OF with the existing composite type dependency. Inherited source is in progress:
basic parent order, local checks and child overrides have replay coverage, but
diverged parent defaults and parent-added column order remain unresolved. Do
not treat inherited source as release-ready. Temporary and foreign tables still
return an explicit unavailable message.
MySQL/SQLite use their native scoped DDL readers; remaining providers retain a
partial metadata preview. Catalog DDL is tested on
PostgreSQL 16; catalog fields require PostgreSQL 14 or later.

Run the DDL round-trip test (creates, exports, drops, recreates and compares
catalog output, including quoted identifiers and partitions):

```bash
FASTDB_TEST_PG_PORT=5432 cargo test -p db-wire --test postgres_test test_postgres_ddl_round_trip
```

Run the metadata integration test against a disposable database with schema/table
creation privileges:

```bash
FASTDB_TEST_PG_PORT=5432 cargo test -p db-wire --test postgres_test test_postgres_catalog_details_and_native_columns
```

Optional settings: `FASTDB_TEST_PG_HOST`, `FASTDB_TEST_PG_USER`,
`FASTDB_TEST_PG_PASSWORD`. The test uses the `postgres` database and creates/drops
its own schema. Without `FASTDB_TEST_PG_PORT`, it skips unless CI sets
`FASTDB_REQUIRE_DB_TESTS=1`.

Catalog references: [pg_class](https://www.postgresql.org/docs/current/catalog-pg-class.html),
[definition functions](https://www.postgresql.org/docs/current/functions-info.html),
[table statistics](https://www.postgresql.org/docs/current/monitoring-stats.html).

## SQL Server, Oracle and Db2

These providers use installed vendor ODBC drivers. FastDB bundles the unixODBC
manager on macOS/Linux, runs blocking client calls on worker threads, and keeps
one connection per session. The original PostgreSQL, MySQL/MariaDB and SQLite
providers keep their existing drivers.

## Client installation

Install the vendor's driver matching the architecture of FastDB (arm64 for an
Apple Silicon build, x86_64 for a Linux x86_64 build). A server alone is not enough.

| Provider | Client | Connection fields |
| --- | --- | --- |
| SQL Server | [Microsoft ODBC Driver 18](https://learn.microsoft.com/en-us/sql/connect/odbc/download-odbc-driver-for-sql-server) | Host, TCP port (1433), database, SQL login and password |
| Oracle | [Oracle Instant Client Basic and ODBC](https://www.oracle.com/database/technologies/instant-client/downloads.html), 19c or later | Host, listener port (1521), **service name**, user and password |
| Db2 for LUW | [IBM Data Server Driver for ODBC and CLI](https://www.ibm.com/docs/en/db2/11.5.x?topic=drivers-obtaining-data-server-driver-odbc-cli) | Host, TCP port (50000), database, user and password |

Register the installed driver in unixODBC's `odbcinst.ini`. FastDB looks for driver
names containing `SQL Server`, `Oracle`, or `DB2` / `IBM Data Server`, respectively.
Microsoft ODBC Driver 17 or later is required; the highest numbered installed version is preferred.
The bundled manager reads `/etc/odbcinst.ini` and user configuration; if the vendor
installer uses another directory, launch FastDB with `ODBCSYSINI` pointing to that
directory. `ODBCSYSINI` contains a directory, not a file name. Configuration must
be visible to the launched app, including when it is started from Finder.

Example registration (use the real installed library path):

```ini
[ODBC Driver 18 for SQL Server]
Driver=/absolute/path/to/libmsodbcsql.18.dylib

[Oracle Instant Client ODBC]
Driver=/absolute/path/to/libsqora.dylib

[IBM DB2 ODBC DRIVER]
Driver=/absolute/path/to/libdb2.dylib
```

On Linux, use the corresponding `.so` library paths. Follow vendor setup
instructions for dependencies and Oracle/Db2 runtime library search paths.
An absent driver produces a connection error with setup guidance; it never falls
back to a different database protocol.

Oracle and Db2 do not consistently unescape brace-quoted connection fields. FastDB sends
their connection fields without braces and rejects semicolons/control characters
rather than allowing connection-string options to be injected. Use credentials
without those delimiters with these providers.

## SQL and results

All seven providers have driver implementations; only PostgreSQL, MySQL and
SQLite are currently selectable in the desktop connection dialog. Deferred ODBC
providers retain the setup and future validation protocol below. For an enabled
provider use **Test Connection**, then **Connect**, open a SQL script and run the
statement at the caret. SQL for these three providers is sent without automatic SELECT wrapping;
use the database's own `TOP`, `FETCH`, or other limits when needed. Oracle SQL and Db2 statement
terminators are removed; Oracle PL/SQL block semicolons are retained.
In Run script, standalone `GO` separates SQL Server batches, standalone `/`
ends an Oracle PL/SQL block, and `--#SET TERMINATOR` sets the Db2 delimiter.
FastDB strips these client directives. SQL Server executes each batch once.

Result sets stream through the existing bounded result store. SQL NULL remains
NULL, strings are decoded through UTF-16, exact decimals remain text, and binary
values are displayed as hex. Statements without a result set complete with an
empty result. For a statement producing multiple result sets, only the first is
displayed; subsequent results are drained and late errors reported.

Table browsing uses `OFFSET … FETCH` for SQL Server 2012+, Oracle 12c+ and current
Db2 LUW releases. Columns are loaded in bulk for the navigator and completion.
Oracle initially lists objects owned by the connected user. Schema constraints,
indexes and primary-key enrichment are not implemented for these providers yet.
Cancellation calls ODBC `SQLCancel`; responsiveness depends on the vendor driver.

## SSH forwarding

PostgreSQL, MySQL, and MariaDB use password or private-key authentication
against a jump host pinned by its OpenSSH SHA256 fingerprint. The database
endpoint is resolved from the jump host. TLS verification uses the database
hostname, including for tunneled connections. The driver factory rejects SSH
settings for SQLite and vendor ODBC providers.

`cargo test --workspace --locked --lib ssh_` runs the SSH fixtures. Set
`FASTDB_TEST_PG_PORT` and `FASTDB_TEST_MYSQL_PORT` and the usual optional
credentials to exercise main and isolated script sessions with real databases.

## TLS

PostgreSQL, MySQL, and MariaDB drivers accept an optional PKCS#12 client
identity through `ConnectionConfig::tls_client_identity`. TLS must be enabled.
The archive contains the client certificate and private key. The database
server certificate and hostname are verified independently. The
`ci/test-tls.sh` fixture verifies both normal and isolated script connections
against PostgreSQL certificate authentication and MySQL `REQUIRE X509`.

- SQL Server presets enable TLS. Certificate and hostname verification stay on;
  install private CAs in the driver/system trust store. The Root CA field is not
  used for this provider.
- Oracle TLS uses Easy Connect Plus `tcps` with server identity matching. Configure
  the Oracle client's trust store/wallet according to the installed client. The
  PEM Root CA field is not used for this provider.
- Db2 TLS uses `Security=SSL` and `SSLClientHostnameValidation=Basic`. The optional
  Root CA path is passed as `SSLServerCertificate`. Use a recent IBM client that
  supports hostname validation.

## Live tests

Run `cargo test -p db-wire --test provider_test -- --nocapture` after setting
`FASTDB_TEST_<PROVIDER>_PORT`, `_HOST`, `_DATABASE`, `_USER`, and optionally
`_PASSWORD`, `_TLS=1`, `_CA` for `SQLSERVER`, `ORACLE`, or `DB2`.
`_DATABASE` means service name for Oracle. Without a provider's port, its test is
skipped. `FASTDB_REQUIRE_PROVIDER_TESTS=1` requires all three providers to be
configured. The basic tests only read data; set `FASTDB_TEST_PROVIDER_WRITES=1` to
also exercise DDL, DML and transactional row editing in uniquely named test tables.
The write opt-in also checks dedicated tab transactions, native savepoints and
rollback on session closure with read-back through a separate connection.
The configured accounts need permission to create/drop their own test tables for
that optional write check. These tests require actual vendor clients and servers;
unit tests do not establish vendor compatibility.

### Local validation (2026-11-01)

Oracle Free 23 with the ARM64 Oracle Instant Client 23.26 and SQL Server 2022
with Microsoft's x86_64 ODBC Driver 18.7 passed the provider integration tests
against local containers. Checks included connection reuse, streamed values and
NULLs, decimal precision, catalog reads, invalid SQL, DDL/DML and rollback of a
failed row-edit transaction. These runs used plaintext local connections; vendor
TLS and the macOS UI were not validated by these runs.

Db2 Community 12.1.5 started and created the test database, but its FastDB
integration test did not pass: IBM's client interpreted brace-quoted hostnames
literally. The connection-string correction has unit coverage; live Db2
validation is deferred. PostgreSQL remains the priority provider.

A separate PostgreSQL 18 container passed all four PostgreSQL integration tests:
binary decoding against server text, incremental streaming of 20,000 rows,
query cancellation and connection reuse across short-lived caller runtimes.

### Workflow acceptance (2026-11-09)

`FASTDB_TEST_PG_PORT=… FASTDB_TEST_MYSQL_PORT=… cargo test -p db-wire --test
workflow_test -- --nocapture` passed against PostgreSQL 15.19 (local cluster)
and MySQL 8.4.13 (`mysql:8.4` container). The tests cover the connect, browse,
run, edit/save/read-back, export, cancel and disconnect data paths; see
[workflow acceptance](WORKFLOW_ACCEPTANCE.md). Desktop acceptance is separate.

See [local server setup](../ci/provider-containers/README.md) for images, ports,
architecture requirements and the Db2 image's setup workaround.


## SQL planning boundaries

Table browsing and row edits share provider-aware identifier quoting and pagination in `db-wire::sql`. SQL Server identifiers use brackets and string literals preserve Unicode. Default-only row inserts are rejected for Oracle and Db2 until their provider-specific behavior has live coverage; enter at least one column instead. Generated row edits still run through the driver's exactly-one-row transactional contract.

When native relation DDL is unavailable, Properties falls back to a partial metadata preview. Incomplete native column types make that preview unavailable; index and constraint names without definitions appear only as comments. PostgreSQL uses catalog-derived structural DDL, MySQL uses SHOW CREATE TABLE for tables/views, and SQLite returns stored relation/index/trigger SQL. SQL planning has deterministic unit coverage for all presets. Unit coverage does not replace the configured live provider and TLS tests described above.

## Visual query plans

All native plan actions reuse the SQL tab's physical session, including TEMP
objects and active transactions. The original statement is neither paged nor
wrapped in a subquery. The app validates one statement before execution.

| Provider | Estimated | Actual | Limits |
| --- | --- | --- | --- |
| PostgreSQL | EXPLAIN FORMAT JSON | ANALYZE/BUFFERS FORMAT JSON | Native costs, rows, per-loop times/loops and raw JSON. |
| MySQL | EXPLAIN FORMAT=JSON | EXPLAIN ANALYZE FORMAT=TREE | Native nested/window/subquery operators, estimates and per-loop observations. |
| MariaDB | EXPLAIN FORMAT=JSON | ANALYZE FORMAT=JSON | Native row/cost fields when present; r_total_time_ms is divided by r_loops for per-loop display. Other engine timings stay in native details. Desktop preset remains disabled. |
| SQLite | EXPLAIN QUERY PLAN | Unavailable | Native operator hierarchy and raw rows; no invented costs or runtime statistics. |
| SQL Server | SHOWPLAN_ALL | Pending | Adapter and fixture tests are available. Live ODBC acceptance is pending. |
| Oracle and Db2 | Pending | Pending | Native plan adapters remain open. |

Analyze executes the statement, including supported writes/function side
effects. Error/cancel cleanup follows the existing session runner. Stateful
providers can override ScriptSession::execute_plan; ScriptControl::with_plan
marks only the last input statement, preserving preceding setup or implicit
transaction controls. PostgreSQL/MySQL/MariaDB output is limited to 16 MiB;
all adapters enforce a 4,096-operator bound.

Expensive-node navigation uses reported inclusive per-loop time multiplied by
loops, otherwise native cumulative cost. Operators may overlap; their values
are not summed. Save/Open JSON snapshots preserve provider/native output,
never replay SQL, and compare only the same provider. See the
[SQL editor guide](guide/sql-editor.md) for file/report bounds and GUI status.

The new query_plan_test suite passed against disposable PostgreSQL 16.15,
MySQL 8.4.13 and MariaDB 11.4.13 with FASTDB_REQUIRE_DB_TESTS=1. It verifies
estimated/actual metrics, TEMP-session identity, unchanged data, MySQL window
operators and SQLite setup/BEGIN preservation. mysql_session_test additionally
verifies that explaining DELETE does not execute it. Desktop acceptance remains
pending. The opt-in SQL Server plan test uses a session-local temporary table and checks that both successful and failed Explain restore normal execution. MariaDB uses FASTDB_REQUIRE_MARIADB_TESTS when its fixture is mandatory.

Syntax references: [MySQL EXPLAIN](https://dev.mysql.com/doc/refman/8.4/en/explain.html),
[MariaDB ANALYZE JSON](https://mariadb.com/docs/server/reference/sql-statements/administrative-sql-statements/analyze-and-explain-statements/analyze-format-json),
[SQLite PRAGMAs](https://www.sqlite.org/pragma.html).

### Native column metadata

MySQL's scoped and schema-wide readers preserve COLUMN_TYPE (including unsigned,
length and decimal precision), the server's COLUMN_DEFAULT value, collation,
comment and generated-column identity. Auto-increment columns accept explicit
NULL as a generated-key request. INFORMATION_SCHEMA cannot distinguish an absent
default from an explicit NULL default; SHOW CREATE remains authoritative for
that distinction. See the [native column catalog](https://dev.mysql.com/doc/refman/8.4/en/information-schema-columns-table.html).

SQLite's background column reader uses table_xinfo (including generated columns),
foreign_key_list and index_list. It preserves declared types and default SQL,
including SQL NULL, literal 'NULL', empty text and absent defaults. Hidden
virtual-table implementation columns are excluded. Rowid-alias PKs are not
nullable; ordinary nullable PKs and the inline INTEGER PRIMARY KEY DESC
exception retain SQLite semantics. STRICT/WITHOUT ROWID PKs use native NOT NULL
flags. Read-only authorization and quoted identifiers have real-engine tests.

A SQLite 3.46.0 fixture with 1,000 unrelated tables and 60 target columns used
native virtual metadata scans plus a small ORDER BY sort, not user-table data
scans. Query-plan/profile evidence is recorded in the completion plan. Native
SQLite relation DDL reads the stored table or view definition plus explicit indexes and triggers for the selected object. It caps source at 1 MiB and 256 objects. MySQL reads the selected table or view with [SHOW CREATE TABLE](https://dev.mysql.com/doc/refman/8.4/en/show-create-table.html) and caps displayed source at 1 MiB. Remaining relation metadata and PostgreSQL relation kinds are tracked in the roadmap.

PostgreSQL integration tests also accept `FASTDB_TEST_PG_PASSWORD` when the local
test server requires password authentication. Leave it unset for trust/peer setups.


## PostgreSQL script sessions

Execute and Run script share one dedicated physical PostgreSQL connection per
SQL tab. TLS/root CA/hostname validation and socket policy apply, with a separate
cancellation token. Explain/Analyze use the same tab session, so plans can access temporary tables,
session settings and uncommitted writes. Analyze executes SQL in the current
transaction; SQL errors allow savepoint/rollback recovery. Cancel uses that
session's cancellation token, then rolls back and closes it. Metadata, browsing
and row-edit Save use the original driver connection. A lost tab session never reconnects or retries SQL.

Temporary tables, settings and transactions survive successive runs. Begin,
Commit and Rollback toolbar controls execute on the tab session. SQL supports
BEGIN / START TRANSACTION, COMMIT / END, ROLLBACK / ABORT, optional WORK /
TRANSACTION, SAVEPOINT, ROLLBACK TO [SAVEPOINT] and RELEASE [SAVEPOINT].
BEGIN options, chaining and prepared transactions remain unsupported.

SQL errors stop remaining statements and retain the session for recovery.
Cancel drops the current query future before its session-local cancellation,
waits for acknowledgement, then rolls back and closes the connection. Cancellation
and cleanup each have a five-second bound. Closing the tab or disconnecting also
closes idle sessions, causing server rollback. Earlier committed/autocommit work
remains applied; losing a COMMIT acknowledgement can mean an uncertain outcome.
Run ownership remains active through cleanup so late cancellation cannot reach
a later run. Quit waits for bounded active-run cleanup after the autosave barrier.

Other providers retain their ordinary-script policy and default to no dedicated
session capability. No provider is selected through workspace-specific SQL cases.

```bash
FASTDB_TEST_PG_PORT=5432 cargo test -p db-wire --test script_session_test
# TLS settings are normally supplied by ci/test-tls.sh:
FASTDB_TLS_PG_PORT=5433 FASTDB_TLS_CA=/path/to/ca.pem \
  cargo test -p db-wire --test tls_test postgres_pinned_script_session
```

Live tests cover backend identity, temporary tables/settings, commit/rollback,
error/cancellation cleanup, no leakage to row-edit/normal sessions, and terminal
connection loss without replay. TLS tests cover private CA/hostname validation,
encrypted sessions and session-local cancellation. Environment-based skipping
and the existing CI requirement flags remain unchanged.


## ER diagram relationships

PostgreSQL loads ordered FK column pairs from pg_constraint/pg_attribute, including
self references and cross-schema targets. MySQL/MariaDB uses
information_schema.key_column_usage with ordinal_position. SQLite uses the
table-valued pragma_foreign_key_list and resolves omitted target columns against
the ordered primary key; its diagram covers the main database. SQLite's generated
fk_<id> labels identify constraints within their source table, not stored names.

Relationship loading runs during connection/schema refresh. Catalog errors and
unsupported ODBC providers are shown explicitly rather than interpreted as no
relationships. Target tables outside the loaded catalog appear with unavailable
columns. Diagrams retain qualified identities and never infer FKs from names or
parse DDL. SQL-tab temporary tables are outside this connection metadata view.


## PostgreSQL column drafts

Column definitions use dedicated connections with a pg_catalog search path and
stable database/table-OID/attribute identity. Mutations require an owned ordinary
local column outside inheritance/partition trees; identity/generated columns are
read-only. Name, native type, nullable, default and comment changes generate a
reviewable DDL sequence. Defaults are dropped/reapplied around type changes;
renames run last. No conversion or CASCADE is invented. Type candidates include
catalog base/domain/enum types; explicit native declarations retain typmods and
arrays. Edited fragments reject statement separators, comments, dollar quoting
and session-dependent backslash escapes; unchanged catalog expressions survive.

Persist uses a new physical connection, 3-second lock timeout, 15-second statement
timeout, ACCESS EXCLUSIVE table lock and an original-snapshot comparison inside a
transaction. A pre-commit error rolls back all changes. Commit acknowledgement or
read-back failures require explicit Refresh before another attempt. Refresh uses
the stable catalog identity, including after a successful rename. SQL-tab
transactions are independent. Reconnect/configuration changes require reloading
the column snapshot. Schema-cache refreshes address the owning connection even
if the user switches tabs. Existing row edits are retained but blocked from Save
until the table result is reloaded.

Automated PostgreSQL fixtures cover rename/Unicode comments, NULL vs removed
defaults, NOT NULL/type-conversion rollback, concurrent changes, dropped/recreated
columns, specialized read-only columns and session isolation. GUI acceptance is
pending the user's manual test. See the implementation plan for further scope.


## PostgreSQL read-only catalog sections

Dependencies/References use direct pg_depend links, including constraints/defaults
and view rewrite dependencies. Relation targets come from catalog OIDs; the UI
does not parse object names/DDL. Lists include internal objects and do not claim
transitive dependency closure. Partitions exposes direct children and parent
relations with native bounds; navigation supports drilling through nested trees.
Triggers excludes internal constraint triggers; Rules includes view return rules.
Policies shows roles/commands/USING/WITH CHECK plus enabled/forced RLS flags.
Permissions uses aclexplode for stored/default owner table ACLs and explicit
column ACLs. Effective role membership and RLS access are not computed.

Each section loads on a dedicated read-only repeatable-read connection, uses a
10-second statement timeout and preserves TLS/hostname verification. Per-tab
caches are invalidated by metadata generations; stale replies after
refresh/reconnect/disconnect/close are ignored. Unsupported providers return None,
distinct from a successful empty list. Source copy is read-only; relation
navigation is offered only for real relation targets.

Live PostgreSQL fixtures validate all seven sections, FK/view targets,
partition parent/bounds, user triggers/rules, RLS roles/flags, table/column grants
and empty/missing relations. Other-provider defaults are covered by SQLite tests.
GUI acceptance is pending at the user's request.


## Structured filter values

The criteria builder emits provider-quoted identifiers and data literals with
explicit comparison/LIKE/NULL operators. PostgreSQL uses E literals with doubled
backslashes/quotes, preserving values even when standard_conforming_strings is
off. MySQL/MariaDB uses CONVERT of UTF-8 hex literals so NO_BACKSLASH_ESCAPES and
connection string escaping cannot change data semantics. Other providers use the
existing dialect literal rules; live ODBC criteria validation remains pending.

SQLite fixtures verify exact returned rows, numeric comparisons, ordering, empty
strings, NULL text versus NULL predicates and attempted quote injection. Native
PostgreSQL and MySQL fixtures verify Unicode/quote/backslash preservation under
server string modes; the MySQL fixture executes generated predicates and numeric
comparisons against bound input rows on one leased test session. These tests do
not modify global server settings. Paging recognizes only outer clauses rather
than keywords inside quoted values or subqueries; ambiguous legacy lexical forms
retain conservative fallback behavior.


## Metadata capability declarations

`DatabaseDriver::metadata_capabilities()` describes implemented readers, not user
privileges, completeness for every relation kind, or live support certification.
Unlisted readers default to not implemented; unsupported is a separate state.

| Provider | Available readers | Current limits |
| :--- | :--- | :--- |
| PostgreSQL | Columns, constraints, indexes, FK mappings, native table details, all seven optional catalog sections | Native detail/DDL relation-kind limits above still apply. |
| MySQL/MariaDB | Columns, constraint names/types, index names, FK mappings | Native details and extended sections are not implemented; PostgreSQL Rules/Policies are unsupported. |
| SQLite | Columns, index names, FK mappings | Constraint/detail readers are not implemented; partitions, Rules/Policies and database ACLs are unsupported. |
| ODBC presets | Columns | Constraint/index/FK/detail and extended catalog readers are not implemented. No live validation is implied. |

Optional schema-reader outcomes are retained independently. Supported empty data
is a success; failed reads do not become empty metadata. Native PostgreSQL 42501
and MySQL access-denial codes become permission-denied states, preserving context;
other native/ODBC errors remain explicit failures. Each optional batch has a
five-second client deadline within the existing connection/refresh deadline.
Optional batches load concurrently; there are no new catalog SQL queries.
Catalog sections retain their existing dedicated read-only PG connection,
server/client deadlines and connection-generation guards. Refresh retries failed
metadata; unavailable readers are not called. UI acceptance is pending.

## PostgreSQL definition search

The exact schema scope covers views and materialized views, functions and
procedures, non-internal triggers, rules, indexes, table and domain constraints,
row-level security policies and extended statistics. Policy source includes
command, mode, roles and any explicit predicates. Statistics are scoped by the
statistics object’s schema. Definitions are read in a dedicated read-only
transaction with a shared match limit and a bounded source preview.

## SQLite definition search

The navigator searches stored CREATE statements for tables, views, indexes
and triggers in the saved database's `main` schema. It uses a dedicated
read-only handle for files and the owning handle for in-memory databases.
Search uses ASCII case folding, a bounded result list and cancellation.

SQLite data search uses the same catalog worker for ordinary tables in
`main`. Table metadata and matches come from one read transaction.
Selecting a result displays the bounded preview captured during the scan.

## MySQL definition and table data search

The navigator reads visible view SELECT text, stored function and procedure
bodies, and trigger bodies from the selected MySQL database. Visibility follows
the connection user's catalog privileges. The read-only scan uses bound values,
a shared result limit and server-side cancellation.

Table data search resolves and scans one base table at a time in a read-only
transaction. Results include bounded previews captured by the search query,
including for tables without a primary key. Cancelling an active request uses
a separate control connection with the same credentials and transport.

## Extended catalog explorer

PostgreSQL object discovery uses native OID keyset pages of at most 100 entries.
Sequences, functions/procedures, user triggers and user-defined enum/domain/
composite/range/multirange types are exposed outside system catalogs. Native
identity and schema/name/signature are rechecked before source reads. Routine
and trigger definitions come from PostgreSQL; sequences/types show catalog
metadata. This is read-only inspection. Source exceeds 256 KiB is rejected.
Requests use dedicated read-only repeatable-read connections, a 10-second
statement deadline and a 15-second UI request deadline. Other providers report
unsupported discovery. Favorites are stored separately in favorites.json under
the application config directory, preserving existing connections.json files.
Missing favorites and malformed/save-failed files have explicit feedback.
Desktop acceptance remains pending.

## Execution feedback (P1-04)

SQLite SQL tabs now use dedicated connections sharing the owning driver's main
database, including an isolated named in-memory database when applicable. TEMP
objects and transactions remain tab-local. Cancellation uses both SQLite's
interrupt handle and a progress callback to cover cancellation before execution
starts; cleanup waits for the blocking worker and closes the connection to roll
back uncommitted work. `cargo test -p db-wire --test sqlite_session_test --locked`
exercises real SQLite sessions without external provider configuration. Full
P1-05 provider parity and desktop acceptance remain pending.

MySQL/MariaDB SQL tabs use dedicated nonpooled connections. A bounded worker
queue keeps each tab's TEMP objects, user variables and transactions together.
Native OK-packet flags report transaction state; after SQL errors a bounded
`DO 0` refresh obtains current state without replaying the failed statement.
Connection loss discards the session. Cancel terminates an active server query
through a separate same-user connection, with bounded connection/command/
disconnect deadlines; idle closure rolls back and disconnects. Cleanup errors
remain visible. DDL may implicitly commit earlier work.

`FASTDB_TEST_MYSQL_PORT=55440 cargo test -p db-wire --test mysql_session_test --locked`
was exercised against the local MySQL fixture. Tests cover TEMP/settings/session
identity, savepoints, runtime lifetime, actual server cancellation and rollback,
error recovery, implicit DDL commit, connection loss without replay, and tab-owner
closure. This is MySQL evidence; separate MariaDB acceptance remains pending.

MySQL/MariaDB parameter detection and modal substitution preserve native `@name`
variables and `@@` system variables; `:name` and `?` remain client parameters.
The scanner consumes full native variable names so a name such as `@counter$1`
does not accidentally prompt for `$1`. Other providers keep their existing
`@name` parameter behavior. See [MySQL user-variable syntax](https://dev.mysql.com/doc/refman/8.4/en/user-variables.html).

The SQL toolbar isolation selector uses PostgreSQL session transaction
characteristics, MySQL/MariaDB session transaction isolation and SQLite's
`read_uncommitted = 0` for Serializable. SQLite does not advertise weaker levels
that depend on shared-cache behavior. Selection is blocked for active/unknown
transactions; settings and native read-back use the pinned tab session with a
15-second per-statement deadline. Live PostgreSQL/MySQL and real SQLite tests
check exact read-back values and unchanged browsing-session settings. Separate
MariaDB runtime and desktop-dialog acceptance remain pending.

The MySQL/MariaDB Autocommit control applies native `SET SESSION autocommit`
and reads `@@SESSION.autocommit` back on the pinned session. Enabling is blocked
while a transaction is active/unknown; the user must Commit or Rollback first.
Disabling does not end existing work. Live MySQL tests verify Off/On, explicit
and implicit manual transactions, blocked enabling, rollback/read-back and an
unchanged browsing session. Separate MariaDB and desktop acceptance remain open.

PostgreSQL/SQLite client-managed autocommit lazily opens a transaction before
ordinary SQL and preserves Off across explicit Commit/Rollback and session
closure. SQLite checks native state before each ordinary statement, including
after an outer SAVEPOINT is released. Enabling is blocked until pending work is
explicitly resolved. Real SQLite/PostgreSQL tests cover cross-run writes,
savepoint error recovery, explicit boundaries and independent browsing
visibility; PostgreSQL Analyze is verified in the same transaction. Desktop
acceptance is deferred by the user.

PostgreSQL forwards bounded per-statement NOTICE/WARNING messages, native command
counts and one-based character error positions in SQL tab execution logs.
MySQL/MariaDB show native counts and up to 20 warning details from the same connection;
failure/timeout loading warnings is explicit and does not retry the original SQL.
SQLite uses native change counts for commands without result columns; ODBC uses
SQLRowCount when available and labels unknown counts unavailable. ODBC informational
diagnostics and structured positions are not yet exposed. See the
[SQL editor guide](guide/sql-editor.md) for deadlines, retention and cancellation limits.

`cargo test -p db-wire --test execution_feedback_test -- --nocapture` always tests real
in-memory SQLite and optionally PostgreSQL/MySQL using `FASTDB_TEST_PG_*` and
`FASTDB_TEST_MYSQL_*` (PORT, HOST, USER, PASSWORD, DATABASE). Missing ports print a skip;
`FASTDB_REQUIRE_DB_TESTS=1` makes missing live providers fail. The PostgreSQL fixture
uses temporary tables and leaves no persistent objects.


## Editable query results

PostgreSQL supports the explicit Load editable result workflow for a direct
schema-qualified SELECT * against a verified permanent ordinary table with its
complete primary key. Non-key Boolean, integer, text, varchar and UUID fields
can be updated. Unsupported relation forms and query projections remain
read-only. Save uses an isolated connection, bound values, original-value
checks and transaction-level source verification. Stored values are reloaded
after commit. Tests cover composite keys, concurrent writers, conflict rollback,
views, row security, user triggers, missing keys and schema drift.

Run the native integration test with FASTDB_TEST_PG_PORT and the optional
FASTDB_TEST_PG_HOST, DATABASE, USER and PASSWORD settings.

```bash
cargo test -p db-wire --test query_edit_test --locked -- --nocapture
```

MySQL/MariaDB, SQLite and ODBC query results remain read-only.

## Typed value edits (P1-07)

Table Save plans binary edits as native binary literals, preserving raw SQLite/
MySQL bytes and decoded PostgreSQL/ODBC hex values. PostgreSQL text literals use
explicit escape strings; MySQL/MariaDB use UTF-8 hex conversion so session
backslash modes cannot change the intended value. This shares the validated
structured-filter literal encoder. Query-result editing retains its narrower
scalar type whitelist.

SQLite tests cover binary INSERT/UPDATE/DELETE, duplicate/Save read-back, NULL,
empty bytes and literal `\x` prefix bytes. PostgreSQL live tests cover exact
binary, JSON number/duplicate-key text, XML mixed content, empty/NULL and text
under both `standard_conforming_strings` modes. MySQL 8.4 tests cover the table
Save path and literals on pinned sessions with/without `NO_BACKSLASH_ESCAPES`.
Oracle empty binary edits are explicitly unavailable until native RAW/BLOB
metadata can distinguish empty BLOB from Oracle NULL semantics. Other ODBC binary literal variants have unit coverage;
live verification remains deferred to Linux by user request.

```bash
FASTDB_TEST_PG_PORT=5432 cargo test -p db-wire --test postgres_test value_edits_roundtrip
```
