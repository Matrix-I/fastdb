# Provider setup and validation

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
VACUUM to populate Properties. Catalog visibility follows `information_schema`;
metadata failures appear in the Properties header and Statistics section without
preventing table browsing.

### PostgreSQL DDL

Properties → DDL / Copy DDL exports executable structural SQL from one catalog
query snapshot during metadata refresh. It supports ordinary and partitioned
tables, partition attachment, zero-column tables and views. Output preserves:

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
security labels and extension membership are outside its scope. Inheritance,
typed, temporary and foreign tables return an explicit unavailable message.
Other providers keep their partial metadata preview. Catalog DDL is tested on
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

All seven providers are selectable in the connection dialog. Use **Test
Connection**, then **Connect**, open a SQL script and run the statement at the
caret. SQL for these three providers is sent without automatic SELECT wrapping;
use the database's own `TOP`, `FETCH`, or other limits when needed. Oracle SQL and Db2 statement
terminators are removed; Oracle PL/SQL block semicolons are retained.
SQL*Plus commands and batch delimiters such as `GO` are not SQL statements.

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

## TLS

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

See [local server setup](../ci/provider-containers/README.md) for images, ports,
architecture requirements and the Db2 image's setup workaround.


## SQL planning boundaries

Table browsing and row edits share provider-aware identifier quoting and pagination in `db-wire::sql`. SQL Server identifiers use brackets and string literals preserve Unicode. Default-only row inserts are rejected for Oracle and Db2 until their provider-specific behavior has live coverage; enter at least one column instead. Generated row edits still run through the driver's exactly-one-row transactional contract.

For providers other than PostgreSQL, DDL in Properties remains a partial metadata preview, not an exact reconstruction. Incomplete native column types make the preview unavailable; index and constraint names without definitions appear only as comments. SQL planning has deterministic unit coverage for all presets. Unit coverage does not replace the configured live provider and TLS tests described above.

## Visual query plans

PostgreSQL script tabs offer Explain (estimated JSON plan) and Analyze (executes
the statement with ANALYZE and BUFFERS). The original statement is not paged or
wrapped in a subquery. Analyze can change data and run functions with side
effects, just like executing the statement normally. Results use the standard
streaming/cancellation path. The tree displays inclusive estimated costs relative
to the root, row estimates, actual per-loop times/rows, loops, conditions and
buffer counts; raw JSON remains available for all other fields. Other providers
currently expose plans only through manually entered provider-specific SQL.

PostgreSQL integration tests also accept `FASTDB_TEST_PG_PASSWORD` when the local
test server requires password authentication. Leave it unset for trust/peer setups.
