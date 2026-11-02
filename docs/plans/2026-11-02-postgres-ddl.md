# PostgreSQL catalog DDL

## Scope

Replace PostgreSQL's column-only preview with executable, catalog-derived table
DDL. Include native types/collations, defaults, identity/generated columns,
owned serial sequences and identity sequence options, constraints, standalone
indexes, comments, table persistence/access method/storage/tablespace, owner,
row security and policies, user triggers, rules and explicit grants. Support
partition keys and attachment to existing parents; dependencies (schemas, roles,
types, functions, referenced tables and partition parents) must already exist.
This is single-object structural DDL, not a database dump or sequence-data backup.
Unsupported relation kinds/inheritance and invalid indexes must be identified
rather than silently presented as complete.

## Steps

1. Add an optional catalog DDL result to TableDetails; fetch in a bounded catalog
   query during background metadata refresh, never during render.
2. Assemble ordered PostgreSQL statements using server quoting/deparser functions.
   Keep provider-neutral previews for other providers and metadata failures.
3. Display PostgreSQL DDL in Properties and retain Copy DDL. Add model tests and
   live round-trip tests that recreate catalog structures, including quoted names,
   generated/identity/serial columns, constraints/indexes, partitions and views.
4. Update README/PROVIDERS. Run fmt, workspace clippy/tests, live PostgreSQL tests
   with a disposable local cluster, and launch an isolated UI profile.

## Commit policy

The plan was saved before implementation and held uncommitted for user review,
following the repository policy against unsolicited commits. The user subsequently
requested commits; record this plan separately from the implementation.

## Implementation and validation

Implemented catalog SQL and cached Properties integration. PostgreSQL 16 live
round-trip coverage recreates ordinary/unlogged/zero-column tables, views and
partitions; quoted identifiers, defaults/collations, identity/serial/generated
columns, check/PK/unique/FK constraints, expression/partial/INCLUDE indexes,
comments, storage/statistics, clustering, RLS/policies, triggers/rules and explicit
ACLs are exercised. Invalid indexes are identified and omitted. Unsupported
inheritance/typed relations return an explicit unavailable message. Catalog DDL
failure preserves the other independently loaded metadata.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `FASTDB_TEST_PG_PORT=55438 cargo test --workspace --offline`: passed, 247 tests.
  All seven PostgreSQL integration tests ran against a disposable PostgreSQL 16
  cluster. MySQL/ODBC/TLS/native-keychain tests retained environment-based skips.
- `cargo build -p fastdb-app --offline`: passed.
- Interactive GUI acceptance is pending the user's manual smoke test. Computer
  Use returned `Computer Use permissions are not granted`. An isolated profile
  at `/tmp/fastdb-ddl-ui-profile` and table `public.fastdb_ddl_smoke` on the
  disposable localhost PostgreSQL port 55438 are prepared for that test.

Validation completed before the user requested commits. No release was created.
