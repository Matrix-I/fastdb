# MySQL definition search implementation plan

## Status audit — 2026-11-12

Source baseline: `76e8815`. The provider driver and native regression tests
are implemented. Implementation boxes confirmed from source are checked;
unchecked reproduction, review, verification and commit steps are not inferred
from source alone. Existing measurement/acceptance evidence below remains
historical, not a fresh test run. Navigator definition/multi-table search
entries have since been removed; current data search opens from a table footer.
See the [current P2 checklist](2026-11-10-p2-search-plans-transfer.md).

> **For agentic workers:** REQUIRED SUB-SKILL: development-superpowers:subagent-driven-development or executing-plans-strict. Steps use `- [ ]`.

**Goal / Architecture / Tech Stack / Branch:** Add native MySQL definition search through the existing trait and GPUI navigator. Rust, mysql_async and MySQL 8. Existing worktree on main, with no commit authorized.

## Confirmed Requirements

- P2-04 covers PostgreSQL, MySQL and SQLite.
- Search object definitions and table data within a selected scope with bounded results, progress/cancel and navigation.
- The user approved additive read-only search API changes.

## Global Constraints

- Keep SQL bound, time-limited and read-only. Keep PostgreSQL and SQLite behavior.
- Run formatter, workspace Clippy, full workspace tests, and build and launch the app. GUI acceptance belongs to the user.
- No commits or pushes without authorization.

## Technical Defaults

- Read visible view SELECT text, routine bodies and trigger bodies from INFORMATION_SCHEMA in the exact database.
- Compare folded UTF-8 bytes for literal matches. Return 51 hits to support the existing 50-result display.
- Use the same pool and a separate connection for server cancellation. Retain the running handle until cancellation finishes.

## Risks & Rollback

- Metadata visibility depends on MySQL privileges. Missing source is excluded by the predicate.
- Revert the driver override and navigator capability check. Search changes no persisted data.

## Task 1 — Driver and navigator

Files: `crates/db-wire/src/mysql/{mod.rs,definition_search.rs}`, `crates/db-wire/tests/mysql_test.rs`, `crates/app/src/workspace/{navigator.rs,definition_search.rs}`.

Interface: implement the existing `DatabaseDriver::search_definitions` for MySQL.

- [ ] Add the regression below and run with `FASTDB_TEST_MYSQL_PORT` set. Expect the default unsupported result to fail.

```rust

#[test]
fn mysql_search_definitions_matches_literal_native_source() {
    use db_wire::objects::DefinitionKind;
    use futures::FutureExt;
    let Some(mut config) = test_config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let schema = format!("fastdb_definitions_{}", std::process::id());
        let admin = MySqlDriver::new(config.clone());
        admin.execute_query(&format!("CREATE DATABASE {schema}")).await.unwrap();
        config.database = schema.clone();
        let driver = MySqlDriver::new(config);
        let result = std::panic::AssertUnwindSafe(async {
            for sql in [
                "CREATE TABLE sample (label VARCHAR(40))",
                "CREATE VIEW marker AS SELECT 'Needle_%' AS label",
                "CREATE VIEW decoy AS SELECT 'needleAB' AS label",
                "CREATE PROCEDURE source_proc() SELECT 'Needle_%' AS label",
                "CREATE TRIGGER source_trig BEFORE INSERT ON sample FOR EACH ROW SET NEW.label = 'Needle_%'",
            ] {
                driver.execute_query(sql).await.unwrap();
            }
            let (cancel, signal) = flume::bounded(1);
            let hits = driver.search_definitions(&schema, "needle_%", signal.clone()).await.unwrap().unwrap();
            assert_eq!(hits.iter().map(|hit| hit.kind).collect::<Vec<_>>(),
                       [DefinitionKind::Routine, DefinitionKind::Trigger, DefinitionKind::View]);
            assert_eq!(hits[1].object.qualifier, "sample");
            assert_eq!(hits[2].object.name, "marker");
            assert!(hits.iter().all(|hit| hit.source.as_deref().unwrap().contains("Needle_%")));
            for index in 0..52 {
                driver.execute_query(&format!("CREATE VIEW bulk_{index} AS SELECT 'bulk marker'")).await.unwrap();
            }
            let capped = driver.search_definitions(&schema, "bulk marker", signal.clone()).await.unwrap().unwrap();
            assert_eq!(capped.len(), db_wire::objects::DEFINITION_SEARCH_LIMIT + 1);
            let wide = "x".repeat(db_wire::objects::DEFINITION_SOURCE_LIMIT as usize);
            driver.execute_query(&format!("CREATE PROCEDURE wide_proc() SELECT 'wide marker {wide}'")).await.unwrap();
            let wide = driver.search_definitions(&schema, "wide marker", signal.clone()).await.unwrap().unwrap();
            assert_eq!(wide.len(), 1);
            assert!(wide[0].source.is_none());
            cancel.send(()).unwrap();
            assert!(driver.search_definitions(&schema, "needle", signal).await.unwrap_err().to_string().contains("cancelled"));
        }).catch_unwind().await;
        admin.execute_query(&format!("DROP DATABASE {schema}")).await.unwrap();
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    });
}
```

- [x] Add `mod definition_search;` to MySQL and override:

```rust
async fn search_definitions(
    &self, schema: &str, needle: &str, cancel: flume::Receiver<()>,
) -> Result<Option<Vec<crate::objects::DefinitionHit>>> {
    self.find_definitions(schema, needle, cancel).await
}
```

- [x] Add `crates/db-wire/src/mysql/definition_search.rs`.

```rust
use super::*;
use crate::objects::{
    DEFINITION_INPUT_LIMIT, DEFINITION_SEARCH_LIMIT, DEFINITION_SOURCE_LIMIT, DefinitionHit,
    DefinitionKind, ObjectSummary,
};

const QUERY_TIMEOUT: Duration = Duration::from_secs(15);
const CANCEL_TIMEOUT: Duration = Duration::from_secs(2);

// These identifiers are fixed catalog fields, never user input.
type DefinitionFamily = (
    DefinitionKind,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);
const FAMILIES: [DefinitionFamily; 3] = [
    (
        DefinitionKind::Routine,
        "ROUTINES",
        "ROUTINE_SCHEMA",
        "ROUTINE_NAME",
        "ROUTINE_TYPE",
        "ROUTINE_DEFINITION",
    ),
    (
        DefinitionKind::Trigger,
        "TRIGGERS",
        "TRIGGER_SCHEMA",
        "TRIGGER_NAME",
        "EVENT_OBJECT_TABLE",
        "ACTION_STATEMENT",
    ),
    (
        DefinitionKind::View,
        "VIEWS",
        "TABLE_SCHEMA",
        "TABLE_NAME",
        "''",
        "VIEW_DEFINITION",
    ),
];

impl MySqlDriver {
    pub(super) async fn find_definitions(
        &self,
        schema: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<Vec<DefinitionHit>>> {
        anyhow::ensure!(
            !schema.is_empty()
                && !needle.trim().is_empty()
                && [schema, needle]
                    .iter()
                    .all(|value| value.len() <= DEFINITION_INPUT_LIMIT && !value.contains('\0')),
            "Enter an exact database name and search text up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes"
        );
        let deadline = tokio::time::sleep(QUERY_TIMEOUT);
        tokio::pin!(deadline);
        let pool = tokio::select! {
            biased;
            _ = cancel.recv_async() => anyhow::bail!("Definition search cancelled"),
            _ = &mut deadline => anyhow::bail!("Definition search timed out"),
            result = self.get_pool() => result?,
        };
        let schema = schema.to_owned();
        let needle = needle.to_owned();
        let deadline_at = deadline.deadline();
        crate::run_on_shared_runtime(async move {
            let timeout = tokio::time::sleep_until(deadline_at);
            tokio::pin!(timeout);
            let mut conn = tokio::select! {
                biased;
                _ = cancel.recv_async() => anyhow::bail!("Definition search cancelled"),
                _ = &mut timeout => anyhow::bail!("Definition search timed out"),
                result = pool.get_conn() => result?,
            };
            let reason = tokio::select! {
                biased;
                _ = cancel.recv_async() => "cancelled",
                _ = &mut timeout => "timed out",
                result = query_definitions(&mut conn, &schema, &needle) => return result.map(Some),
            };
            // Hold the running connection until cancellation finishes so the pool
            // cannot hand its server ID to another operation.
            let id = conn.id();
            let kill = tokio::time::timeout(CANCEL_TIMEOUT, async {
                let mut controller = pool.get_conn().await?;
                controller
                    .query_drop(format!("KILL CONNECTION {id}"))
                    .await?;
                anyhow::Ok(())
            })
            .await;
            match kill {
                Ok(Ok(())) => anyhow::bail!("Definition search {reason}"),
                Ok(Err(error)) => {
                    anyhow::bail!("Definition search {reason}. Server cancellation failed: {error}")
                }
                Err(_) => {
                    anyhow::bail!("Definition search {reason}. Server cancellation timed out")
                }
            }
        })
        .await
    }
}

async fn query_definitions(
    conn: &mut mysql_async::Conn,
    schema: &str,
    needle: &str,
) -> Result<Vec<DefinitionHit>> {
    let mut hits = Vec::new();
    for (kind, catalog, schema_field, name, qualifier, source) in FAMILIES {
        let remaining = DEFINITION_SEARCH_LIMIT + 1 - hits.len();
        if remaining == 0 {
            break;
        }
        let sql = format!(
            "SELECT /*+ MAX_EXECUTION_TIME(10000) */ {name}, {qualifier},
                    CASE WHEN OCTET_LENGTH({source}) <= ? THEN {source} END
             FROM information_schema.{catalog}
             WHERE {schema_field} = ?
               AND CAST({schema_field} AS BINARY) = CAST(? AS BINARY)
               AND LOCATE(
                   CAST(LOWER(CONVERT(? USING utf8mb4)) AS BINARY),
                   CAST(LOWER(CONVERT({source} USING utf8mb4)) AS BINARY)
               ) > 0
             ORDER BY CAST({name} AS BINARY), 2 LIMIT ?"
        );
        let rows: Vec<(String, String, Option<String>)> = conn
            .exec(
                sql,
                (
                    DEFINITION_SOURCE_LIMIT,
                    schema,
                    schema,
                    needle,
                    remaining as u64,
                ),
            )
            .await?;
        for (name, qualifier, source) in rows {
            hits.push(DefinitionHit {
                kind,
                object: ObjectSummary {
                    id: (hits.len() + 1) as u32,
                    schema: schema.into(),
                    name,
                    qualifier,
                },
                source,
            });
        }
    }
    Ok(hits)
}
```

- [ ] Include `DbDriverKind::MySql` in the navigator's definition-button provider match. Keep the table-data gate specific to PostgreSQL. Initialize the modal from the saved database.

```rust
    pub(super) fn open_definition_search(&mut self, cx: &mut Context<Self>) {
        let session = self.connections.active();
        let (Some(owner), Some(driver)) = (&self.connections.active_connection_id, &session.driver)
        else {
            return;
        };
        if !matches!(
            session.kind,
            DbDriverKind::Postgres | DbDriverKind::Sqlite | DbDriverKind::MySql
        ) {
            return;
        }
        let mut search =
            DefinitionSearch::new(owner.clone(), driver.clone(), session.metadata_generation);
        match session.kind {
            DbDriverKind::Sqlite => search.schema = Input::new("main"),
            DbDriverKind::MySql => {
                let database = self
                    .connections
                    .saved_connections
                    .iter()
                    .find(|connection| &connection.id == owner)
                    .map_or("", |connection| connection.database.as_str());
                search.schema = Input::new(database);
            }
            _ => {}
        }
        self.definition_search = Some(search);
        cx.notify();
    }

```

- [ ] Format MySQL routines as Procedure or Function with provider-quoted identifiers in `hit_title`, and assert the Procedure label in the modal model test.
- [ ] Rerun the native test and verify the view's stored SELECT and exclusion of the wildcard decoy.
- [ ] Ready to commit: driver, test and UI files — `feat(db-wire): search MySQL object definitions`.

## Task 2 — Verification and documentation

- [x] Update the navigator guide, provider guide, gaps and roadmap with MySQL source scope and GUI status.
- [ ] Inspect real MySQL query plans, run the full test gate and smoke the app process. Report any missing live environment or manual GUI evidence.
- [ ] Obtain independent implementation review and repeat the final gate after edits.
- [ ] Ready to commit: docs — `docs: describe MySQL definition search`.

## Query plan evidence

MySQL 8.4, isolated catalog with 1,000 views, 250 matching. The initial union
materialized 250 matches before applying its 51-row limit. Its matching and
no-match plans took 3.57 ms and 3.63 ms. The final three statements use the
remaining hit budget at each boundary and avoid the union materialization.
The sum of their server execution times was 3.52 ms for 51 matches and 2.19 ms
for no matches. The view branch uses the data dictionary schema lookup and
a top-N name sort. These are fixture measurements of server execution and
exclude client round trips.
