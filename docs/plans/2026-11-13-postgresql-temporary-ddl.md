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

- [x] Add the following native regression before the query edit:

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

- [x] Run `FASTDB_TEST_PG_PORT=55449 cargo test -p db-wire --lib temporary_structure --locked`.
  Expect failure because the existing output says temporary tables unsupported.
- [x] Apply these exact query transformations:

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

- [x] Rerun the regression and full PostgreSQL provider tests. Verify ordinary
  permanent/unlogged source remains unchanged by the existing replay fixtures.

## Task 2 — Query plans, docs, verification and review

- [x] Measure the existing and changed catalog statement using EXPLAIN
  (ANALYZE, BUFFERS) on a disposable 300-table fixture with indexed keys and
  100,000 user rows. Include temporary relations in the measured connection.
  Record metadata rows, time, buffers and plan shape. Do not claim a speedup.
- [x] Update docs/PROVIDERS.md, docs/guide/navigator-and-tables.md and
  docs/guide/not-built-yet.md with the approved warning/default and session
  visibility. Record the confirmed choice in the main completion plan.
- [x] Read full tracked/untracked diff, check scope and secrets.
- [x] Run cargo fmt --all, cargo clippy --workspace --all-targets --locked,
  cargo test --workspace --locked, cargo build --workspace --locked and live
  isolated PostgreSQL/MySQL suites with real SQLite. Report any failure/skip.
- [x] Use code-review-strict with a full review package. The external Opus
  invocation was previously blocked by automatic approval review for external
  code transmission. Use the internal reviewer as the safer alternative and
  disclose that model-specific exception, without bypassing the rejection.
- [x] Apply receiving-code-review, reverify any fix, then commit the feature and
  matching guides. Save final checkpoint/checklist state separately as needed.
- [x] Stop only the owned database fixtures. Keep runnable binary/dependencies.
  GUI acceptance remains with the user. P2-05 requires its final audit.


## Plan review

The internal independent reviewer approved this plan with no Critical/Important
finding. Its non-blocking source-row suggestion was incorporated so DELETE ROWS
proves a data transition. The literal pg_temp_3 value remains unchanged to guard
against unsafe namespace text replacement. The exact Opus-model requirement
cannot be fulfilled through the external CLI because automatic approval review
previously rejected transmitting private source to that external destination.
The internal reviewer is the safer substitute. This exception will be reported.


## SQL strict projection refinement

The edited query's three c.*/r.* CTE projections are expanded to the named
columns already consumed downstream. This honors sql-strict without changing
the final schema/table/DDL interface. Existing native replay tests and the new
fixture verify the expanded fields. No unrelated query or schema is changed.
The exact pg_class fields are oid, relname, relkind, relpersistence, relowner,
relacl, relam, relforcerowsecurity, relispartition, reloftype, reloptions,
relpartbound, relreplident, relrowsecurity and reltablespace. Subsequent CTEs
also carry nspname, target, object_type and unsupported_reason as applicable.

The native fixture also covers temporary inheritance, partition attachment and
extended statistics/comments because those existing source branches format
namespace references. Its replay assertions insert through the inherited and
partitioned parents and read the resulting child rows. This extends the same
cross-session acceptance rather than introducing a new feature surface.

## Measured aggregate refinement

The initial 300-table plan estimated 429,766 join rows for 2,463 statements,
which crossed the server's JIT optimization threshold. Source generation spent
4,650.034 ms in JIT and took 4,704.057 ms overall. Aggregating statement source
by OID before joining relation names bounds the join to one source per relation:

```sql
SELECT r.nspname::text, r.relname::text, s.ddl
FROM relations r JOIN (
    SELECT oid, string_agg(sql, E'\n' ORDER BY phase, sort_key) AS ddl
    FROM statements GROUP BY oid
) s ON s.oid = r.oid
ORDER BY r.nspname, r.relname
```

The same-fixture candidate took 377.587 ms, with 338.389 ms in JIT. Server JIT
settings remain unchanged. Both returned 303 relations. A separate same-snapshot
EXCEPT ALL comparison in both directions returned zero source differences on
300 tables, 100,000 user rows and temporary serial/index source. Apply this
small final aggregation refinement, rerun native replay suites and review the
whole query. Measurements describe this PostgreSQL 16.15 fixture only.

## Public driver entry-point verification

Also add this fixture to crates/db-wire/tests/postgres_test.rs, using its
existing test_config and CATALOG_TEST_LOCK. This proves source reaches the
normal DatabaseDriver API for temporary objects visible to that connection.
SQL-tab temporary objects retain their existing separate-session visibility.

```rust
#[test]
fn postgres_driver_reports_visible_temporary_structure_with_commit_warning() {
    let Some(config) = test_config() else { return };
    let _catalog_guard = CATALOG_TEST_LOCK.lock().unwrap();
    db_wire::get_runtime().block_on(async {
        let driver = PostgresDriver::new(config);
        let table = format!("fastdb_temp_{}", uuid::Uuid::new_v4().simple());
        driver.execute_query(&format!("CREATE TEMP TABLE {table}(id integer PRIMARY KEY) ON COMMIT DELETE ROWS")).await.unwrap();
        let outcome = driver.list_table_details().await;
        driver.execute_query(&format!("DROP TABLE {table}")).await.unwrap();
        let details = outcome.unwrap();
        let (_, _, details) = details.iter().find(|(_, name, _)| name == &table).unwrap();
        assert_eq!(details.persistence.as_deref(), Some("Temporary"));
        let source = details.ddl.as_deref().unwrap();
        assert!(source.contains(&format!("CREATE TEMPORARY TABLE pg_temp.{table}")), "{source}");
        assert!(source.contains("ON COMMIT is unknown"), "{source}");
    });
}
```


## Paused checkpoint: final verification is not green

This is an incomplete local checkpoint requested by the user, not acceptance.
The initial full native workspace test, clippy, build and startup smoke passed.
After adding the public fixture's overall timeout, final clippy passed but
`FASTDB_TEST_PG_PORT=55449 FASTDB_TEST_MYSQL_PORT=55448 FASTDB_REQUIRE_DB_TESTS=1 cargo test --workspace --locked -- --test-threads=1`
failed: db-wire library tests had 128 passes and two failures. The temporary
source fixture hit its unchanged five-second statement timeout; the SSH/MySQL
fixture received connection EOF. The owned MySQL container was subsequently
observed stopped with exit 137 (OOM flag false); its exact termination cause
remains unknown. The final build did not run after the failed test command.
Logs: /tmp/fastdb-temp-ddl-clippy-final.log and
/tmp/fastdb-temp-ddl-workspace-native-final.log.

The aggregation refinement alone is not reliably sufficient: a later
300-table profile estimated cost 1,148,874 and took 4,709 ms, mostly JIT.
A candidate replacing the information_schema.tables LEFT JOIN visibility
check with EXISTS took 450 ms at estimated cost 363,116 on that same snapshot.
It is only an experiment in /tmp/fastdb-temp-ddl-exists.sql, not applied source.
The profiler and evidence are /tmp/fastdb-temp-ddl-profile.py and
/tmp/fastdb-temp-ddl-exists-profile.log. Preserve the information_schema
visibility rules; do not weaken deadlines or change shared session JIT settings.

Resume with candidate equivalence/permissions verification and a durable query
performance fix, restore and check the isolated MySQL fixture, then rerun fmt,
workspace clippy, native workspace tests and build. Complete independent final
code review only after gates pass. Final code review remains pending; earlier
internal reviews cover the plan/refinements only. The exact Opus review model
was unavailable through the approved route: automatic approval rejected the
external CLI because it could transmit private source externally. Do not retry
that route. Manual GUI acceptance remains pending with the user. No push.


## Resume: visibility semijoin refinement

The existing five-second regression and prior final-gate log remain the
performance reproduction. Replace only the visible_relations LEFT JOIN plus
nonnull check with the equivalent information_schema EXISTS predicate:

```sql
FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
WHERE n.nspname NOT IN ('information_schema', 'pg_catalog')
  AND (EXISTS (SELECT FROM information_schema.tables t
               WHERE t.table_schema = n.nspname AND t.table_name = c.relname)
       OR (c.relkind = 'm' AND (pg_has_role(c.relowner, 'USAGE')
            OR has_any_column_privilege(c.oid, 'SELECT'))))
```

This retains the native visibility view and materialized-view grant branch,
with no copied authorization predicates or session settings. Each catalog
relation has at most one matching schema/name row. First compare exact source
multisets under owner and restricted column grants using sequential temporary
result tables in one rolled-back transaction; avoid inlining four copies of
the query into EXCEPT ALL. Run existing permissions and replay tests, retain
the unchanged timeout, profile both forms, then full gates and code review.
Rollback is to restore the scoped visibility join.

The first resume regression failed before useful query evidence: PostgreSQL
was recovering after the combined equivalence query's backend was killed.
Container cgroup memory.events reported oom_kill=1; the Podman VM has about
940 MiB usable memory and the MySQL fixture again exited 137. No user service
was modified. Sequential comparison bounds JIT memory and runs while the
owned MySQL fixture is stopped. Restore MySQL for the native full gate only
after this profiling transaction completes.


### Semijoin evidence

Internal plan-refinement review approved with no Critical/Important issue.
Corrected sequential same-snapshot comparisons returned zero multiset
differences for owner and restricted column-granted role. The initial one-row
difference was the comparison's own old_result table, excluded symmetrically
from both result sets in the corrected script. All setup rolled back.

At checkpoint 94357ed, the isolated regression reproduced native statement
timeout (7.91 s overall). After the visibility semijoin, the unchanged test
passed in 0.25 s. On 300 tables, 100,000 data rows and temp serial/index source,
EXPLAIN (ANALYZE, BUFFERS) returned 303 rows for both forms. Execution was
6,496.609 ms before and 722.456 ms after; JIT was 6,267.432 / 635.256 ms; shared
hits 31,681 / 29,618, physical reads and temp blocks zero, disk sorts zero,
maximum hash batches one. Only catalog relations were scanned; user data was
not read. Estimated cost fell from 1,149,568 to 362,924, avoiding expensive
JIT optimization on this fixture. Session JIT settings stayed unchanged. These
are fixture measurements, not a production latency guarantee.

Logs: /tmp/fastdb-temp-ddl-stable-red.log,
/tmp/fastdb-temp-ddl-exists-green.log,
/tmp/fastdb-temp-ddl-exists-equivalence.log and
/tmp/fastdb-temp-ddl-exists-final-profile.log. Formatter check and workspace
Clippy passed; full native workspace tests/build/review follow.

### Final verification and review

- `cargo fmt --all -- --check`: passed (exit 0).
- `cargo clippy --workspace --all-targets --locked`: passed (exit 0).
- `FASTDB_TEST_PG_PORT=55449 FASTDB_TEST_MYSQL_PORT=55448 FASTDB_REQUIRE_DB_TESTS=1 cargo test --workspace --locked -- --test-threads=1`: passed across every workspace crate and integration suite (0 failed, 0 ignored), including live PostgreSQL 16.15 and MySQL 8.4.13 tests and real in-process SQLite coverage.
- `cargo build --workspace --locked`: passed (exit 0).
- Isolated `./target/debug/fastdb` 5-second startup check with temporary `FASTDB_CONFIG_DIR`: stayed running 5 seconds and initialized cleanly.
- Independent code review (`code-review-strict` via internal reviewer subagent; external Opus route unavailable due to automatic approval policy): Round 1 found zero code/SQL/test defects and one Important checklist-reconciliation finding (`docs/plans/2026-11-12-p2-05-completion.md`). After reconciling the checklists, Round 2 returned **Approved** (zero Critical, zero Important, zero Minor findings).
- Owned database fixtures (`fastdb-p205-postgres`, `fastdb-p205-mysql`) are stopped, keeping the runnable binary and reusable build dependencies. Manual GUI acceptance remains pending with the user.


## Acceptance closure — 2026-11-13

The user confirmed the P2-05 GUI checklist passed ("test ổn"). This closes the
manual acceptance pending above for PostgreSQL/MySQL/SQLite; see the main
completion plan's dated user acceptance record. No source changed afterward.
