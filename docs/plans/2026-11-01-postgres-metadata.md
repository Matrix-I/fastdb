# PostgreSQL table metadata

## Scope

Load PostgreSQL catalog metadata into the connection-owned Properties cache:
owner, OID, effective tablespace, comment, persistence, row security, partition
key, estimated rows, storage sizes and vacuum/analyze timestamps. Show real
constraint definitions (including foreign keys) and index methods, expressions,
uniqueness, validity and definitions. Other drivers retain unavailable fallbacks.

## Implementation

1. Add optional provider-neutral table details to the database driver trait.
2. Fetch PostgreSQL details in bounded catalog batches, without scanning table
   data or running COUNT, VACUUM or ANALYZE. Preserve unknown statistics as NULL.
3. Populate immutable Properties models during background schema refresh, using
   the existing connection ownership checks. Surface metadata failure in Properties.
4. Render cached values in the header, statistics, index, constraint and foreign
   key sections; retain the existing DDL preview rather than claim a schema dump.
5. Add live PostgreSQL tests for quoted identifiers, composite foreign keys,
   expression/partial indexes, unanalyzed tables, views and partitions. Add UI
   model tests for optional metadata and unavailable values.
6. Update README and provider documentation. Run formatting, workspace clippy,
   workspace tests, live PostgreSQL tests where available, and launch the app.

## Validation semantics

Row counts are explicitly estimates; storage sizes describe the relation itself,
not the sum of partitions. Missing timestamps mean never recorded/unknown.
Refresh reloads metadata. No database credentials are logged.

The user authorized committing this plan and implementation after validation.

## Implementation and validation record

Implemented the shared optional detail API, PostgreSQL batch catalog loader,
native column precision/length/comments and PK/FK membership, connection-owned
Properties caching, real header/statistics/index/constraint/foreign-key values,
and catalog definitions as comments in the partial DDL preview. Metadata errors
are visible and retain the PostgreSQL server's diagnostic message.

Validation on 2026-11-01:

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `FASTDB_TEST_PG_PORT=55439 cargo test --workspace --offline`: passed (212 tests).
  PostgreSQL tests ran against a disposable local PostgreSQL 16 instance;
  MySQL/ODBC/TLS integration tests skipped without their environment variables.
- `cargo build -p fastdb-app --offline`: passed.
- App launch smoke test used `FASTDB_CONFIG_DIR=/tmp/fastdb-metadata-ui`.
  Interactive visual validation could not run: Computer Use reported
  `Computer Use permissions are not granted`. No UI acceptance is claimed.
- Workspace tests initially hit sandbox restrictions on local sockets and macOS
  system services; the complete suite passed with the required execution access.
- The temporary PostgreSQL instance was stopped after validation.
