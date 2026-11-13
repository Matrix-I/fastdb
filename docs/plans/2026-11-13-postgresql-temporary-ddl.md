# PostgreSQL Temporary DDL Implementation Plan

> For agentic workers, use executing-plans-strict for this plan.

Goal / Architecture / Tech Stack / Branch

- Display temporary structural source with an explicit unknown-ON-COMMIT note.
- Extend the existing catalog-snapshot source query and owning metadata tests.
- Rust, tokio-postgres, PostgreSQL 16.15 isolated fixture.
- Branch `feature/p2-05-plans`.

## Confirmed Requirements

- The user selected “Hiện cấu trúc + chú thích thiếu ON COMMIT (đề xuất)”.
- Source includes CREATE TEMPORARY TABLE. No ON COMMIT policy is inferred.
- PostgreSQL/MySQL/SQLite remain the P2-05 scope. GUI acceptance is manual.
- Incremental local commits are authorized. No push for this checkpoint.

## Global Constraints

- Preserve existing permanent/unlogged source, inherited-source guards and ACL
  visibility. The query remains read-only and one statement/snapshot.
- No public API, config, dependency, application database schema or session
  routing changes. Temporary visibility follows the existing metadata connection.
- Referenced dependencies must exist, as for the current structural source.
- No raw textual rewriting of deparsed SQL or user string/comment contents.
- Use pg_temp for current-backend namespace references that this query formats.
- Run full affected suites, workspace formatter/Clippy/tests/build, native
  PostgreSQL tests and query-plan checks. Independently review the complete diff.

## Technical Defaults

- PostgreSQL already emits pg_temp in native index deparsing. The fixture
  confirmed explicit pg_temp is accepted for temporary tables/views/sequences.
- Keep the output row's real schema name for matching TableDetails. Normalize
  only emitted SQL identifiers and the current temporary search-path entry.
- Views/sequences use TEMPORARY, while the unknown-ON-COMMIT warning applies to
  temporary tables/partitioned tables. The body omits an ON COMMIT clause.
- Existing source labels/Copy DDL behavior consume this catalog source directly.

## Risks & Rollback

- A backend-specific namespace could break cross-session replay. Test two
  simultaneously open sessions with distinct temp namespaces.
- Serial/identity sequences, index comments and inherited/partition references
  must use session-local names. Apply identifier-aware formatting, never a
  replace over native SQL strings.
- Omitted ON COMMIT defaults to PostgreSQL PRESERVE ROWS on replay. The source
  warning must state that limitation and default before executable source.
- Existing inherited reconstruction guards still apply to temporary relations.
- No persisted API/schema/format migration. Revert the scoped source commit to
  restore the old unavailable output if needed.

## Task 1 — Native regression and structural source

Files

- crates/db-wire/src/postgres/metadata.rs, add a temporary-source test module
  after the existing permission tests. Use super::load as the production entry.
- crates/db-wire/src/postgres/ddl.sql, formatted relation/sequence/namespace
  targets, supported-kind guard, structural header and CREATE branches.

Interfaces

- Consume the existing `Client` and `load(&Client)` result.
- Produce unchanged `(schema, table, TableDetails)` data with temporary DDL.

- [ ] Add the following native regression before the query edit:

```rust
#[cfg(test)]
mod temporary_source_tests {
    #[test]
    fn temporary_structure_replays_across_sessions_with_unknown_commit_note() {
        let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
            assert!(std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(), "FASTDB_TEST_PG_PORT required");
            return;
        };
        crate::get_runtime().block_on(async {
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                let mut config = tokio_postgres::Config::new();
                config.host(std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()))
                    .port(port.parse().unwrap()).dbname("postgres")
                    .user(std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()));
                if let Ok(password) = std::env::var("FASTDB_TEST_PG_PASSWORD") { config.password(password); }
                config.connect_timeout(std::time::Duration::from_secs(5));
                let (source, source_connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
                let _source_task = crate::AbortOnDrop(tokio::spawn(source_connection));
                let (destination, destination_connection) = config.connect(tokio_postgres::NoTls).await.unwrap();
                let _destination_task = crate::AbortOnDrop(tokio::spawn(destination_connection));
                source.batch_execute(r#"
                    SET statement_timeout='5s'; BEGIN;
                    SET LOCAL search_path=pg_temp,public;
                    CREATE TEMP TABLE "temp preview" (
                        id serial PRIMARY KEY, label text DEFAULT 'pg_temp_3',
                        calculated text GENERATED ALWAYS AS (upper(label)) STORED,
                        CHECK(label <> 'bad')
                    ) ON COMMIT DELETE ROWS;
                    CREATE INDEX preview_label ON "temp preview"(label);
                    COMMENT ON TABLE "temp preview" IS 'Temporary source';
                    COMMENT ON INDEX preview_label IS 'Temporary index';
                    CREATE TEMP TABLE drop_preview(id integer) ON COMMIT DROP;
                    CREATE TEMP TABLE preserve_preview(id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY) ON COMMIT PRESERVE ROWS;
                    CREATE TEMP VIEW preview_view AS SELECT id,calculated FROM "temp preview";
                    INSERT INTO "temp preview"(label) VALUES('source row');
                "#).await.unwrap();
                destination.batch_execute("SET statement_timeout='5s'; CREATE TEMP TABLE anchor(id integer); BEGIN").await.unwrap();
                let source_schema: String = source.query_one("SELECT nspname FROM pg_namespace WHERE oid=pg_my_temp_schema()", &[]).await.unwrap().get(0);
                let destination_schema: String = destination.query_one("SELECT nspname FROM pg_namespace WHERE oid=pg_my_temp_schema()", &[]).await.unwrap().get(0);
                assert_ne!(source_schema, destination_schema);
                let details = super::load(&source).await.unwrap();
                for name in ["temp preview", "drop_preview", "preserve_preview", "preview_view"] {
                    let ddl = details.iter().find(|(schema, table, _)| schema == &source_schema && table == name).unwrap().2.ddl.as_ref().unwrap();
                    assert!(ddl.contains("CREATE TEMPORARY"), "{ddl}");
                    if name != "preview_view" {
                        assert!(ddl.contains("ON COMMIT is unknown"), "{ddl}");
                        assert!(ddl.contains("PRESERVE ROWS"), "{ddl}");
                    }
                    assert!(!ddl.contains(&format!("{source_schema}.")), "{ddl}");
                    destination.batch_execute(ddl).await.unwrap();
                }
                assert_eq!(destination.query_one(r#"SELECT count(*) FROM "temp preview""#, &[]).await.unwrap().get::<_,i64>(0), 0);
                destination.batch_execute(r#"INSERT INTO "temp preview" DEFAULT VALUES; INSERT INTO preserve_preview DEFAULT VALUES; INSERT INTO drop_preview VALUES(7); COMMIT"#).await.unwrap();
                let row = destination.query_one("SELECT id,calculated FROM preview_view", &[]).await.unwrap();
                assert_eq!(row.get::<_,i32>(0), 1);
                assert_eq!(row.get::<_,String>(1), "PG_TEMP_3");
                assert_eq!(destination.query_one("SELECT count(*) FROM drop_preview", &[]).await.unwrap().get::<_,i64>(0), 1);
                assert_eq!(destination.query_one("SELECT id FROM preserve_preview", &[]).await.unwrap().get::<_,i32>(0), 1);
                assert!(destination.batch_execute(r#"INSERT INTO "temp preview"(label) VALUES('bad')"#).await.is_err());
                source.batch_execute("COMMIT").await.unwrap();
                assert!(source.query_one("SELECT to_regclass('pg_temp.drop_preview') IS NULL", &[]).await.unwrap().get::<_,bool>(0));
                assert_eq!(source.query_one(r#"SELECT count(*) FROM "temp preview""#, &[]).await.unwrap().get::<_,i64>(0), 0);
            }).await.expect("temporary-source fixture timed out");
        });
    }
}
```

- [ ] Run `FASTDB_TEST_PG_PORT=55449 cargo test -p db-wire --lib temporary_structure --locked`.
  Expect failure because the existing output says temporary tables unsupported.
- [ ] Apply these exact query transformations:

  1. In formatted `%I.%I` targets, map a namespace equal to pg_my_temp_schema()
     to pg_temp. The real schema output column remains unchanged.

```sql
format('%I.%I', CASE WHEN n.oid = pg_my_temp_schema()
    THEN 'pg_temp' ELSE n.nspname END, c.relname) AS target
```

  Use the same expression with sn/s for sequence targets, n/p for inheritance
  parents, pn/p for partition parents, n/ic for index comments, and n/st for
  explicit ALTER/COMMENT statistics targets.

  2. Split the supported-kind guard so temporary persistence alone no longer
     makes a supported relation unavailable.

```sql
WHEN r.relkind NOT IN ('r', 'p', 'v', 'm', 'f')
    THEN 'this relation kind is not supported.'
```

  3. Append this header expression before the existing SET statements:

```sql
CASE WHEN r.relpersistence = 't' AND r.relkind IN ('r','p') THEN
    E'-- WARNING: ON COMMIT is unknown from PostgreSQL catalogs and is omitted. Replay defaults to PRESERVE ROWS.\n'
ELSE '' END
```

  4. In header search-path aggregation, map only the current temporary namespace:

```sql
CASE WHEN s = (SELECT nspname FROM pg_namespace WHERE oid=pg_my_temp_schema())
    THEN 'pg_temp' ELSE quote_ident(s) END
```

  5. Extend the existing persistence expressions for table/sequence creation:

```sql
CASE WHEN r.relpersistence = 'u' THEN 'UNLOGGED '
     WHEN r.relpersistence = 't' THEN 'TEMPORARY ' ELSE '' END
```

  Use s.relpersistence for the sequence branch. In CREATE VIEW prepend the
  temporary modifier when r.relpersistence='t'. Existing materialized-view,
  foreign-table, inherited-source and permanent branches remain applicable.

- [ ] Rerun the regression and full PostgreSQL provider tests. Verify ordinary
  permanent/unlogged source remains unchanged by the existing replay fixtures.

## Task 2 — Query plans, docs, verification and review

- [ ] Measure the existing and changed catalog statement using EXPLAIN
  (ANALYZE, BUFFERS) on a disposable 300-table fixture with indexed keys and
  100,000 user rows. Include temporary relations in the measured connection.
  Record metadata rows, time, buffers and plan shape. Do not claim a speedup.
- [ ] Update docs/PROVIDERS.md, docs/guide/navigator-and-tables.md and
  docs/guide/not-built-yet.md with the approved warning/default and session
  visibility. Record the confirmed choice in the main completion plan.
- [ ] Read full tracked/untracked diff, check scope and secrets.
- [ ] Run cargo fmt --all, cargo clippy --workspace --all-targets --locked,
  cargo test --workspace --locked, cargo build --workspace --locked and live
  isolated PostgreSQL/MySQL suites with real SQLite. Report any failure/skip.
- [ ] Use code-review-strict with a full review package. The external Opus
  invocation was previously blocked by automatic approval review for external
  code transmission. Use the internal reviewer as the safer alternative and
  disclose that model-specific exception, without bypassing the rejection.
- [ ] Apply receiving-code-review, reverify any fix, then commit the feature and
  matching guides. Save final checkpoint/checklist state separately as needed.
- [ ] Stop only the owned database fixtures. Keep runnable binary/dependencies.
  GUI acceptance remains with the user. P2-05 requires its final audit.


## Plan review

The internal independent reviewer approved this plan with no Critical/Important
finding. Its non-blocking source-row suggestion was incorporated so DELETE ROWS
proves a data transition. The literal pg_temp_3 value remains unchanged to guard
against unsafe namespace text replacement. The exact Opus-model requirement
cannot be fulfilled through the external CLI because automatic approval review
previously rejected transmitting private source to that external destination.
The internal reviewer is the safer substitute. This exception will be reported.
