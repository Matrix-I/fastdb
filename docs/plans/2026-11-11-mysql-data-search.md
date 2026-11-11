# MySQL table data search implementation plan

## Status audit — 2026-11-12

Source baseline: `76e8815`. The provider driver and native regression tests
are implemented. Implementation boxes confirmed from source are checked;
unchecked reproduction, review, verification and commit steps are not inferred
from source alone. Existing measurement/acceptance evidence below remains
historical, not a fresh test run. Navigator definition/multi-table search
entries have since been removed; current data search opens from a table footer.
See the [current P2 checklist](2026-11-10-p2-search-plans-transfer.md).

> **For agentic workers:** REQUIRED SUB-SKILL: development-superpowers:subagent-driven-development or executing-plans-strict. Steps use `- [ ]`.

**Goal / Architecture / Tech Stack / Branch:** Add MySQL data search to the existing GPUI scope picker using Rust and mysql_async. The worktree is on main and no commit is authorized.

## Confirmed Requirements

- P2-04 covers PostgreSQL, MySQL and SQLite. Search selected table data with bounded results, progress, cancel and navigation.
- The user approved additive search APIs and result kinds.

## Global Constraints

- Preserve existing providers. Bind user search terms, quote identifiers with the provider dialect and use read-only transactions.
- Run fmt, workspace Clippy, full tests, build and app startup. GUI verification belongs to the user.
- No commits or pushes without authorization.

## Technical Defaults

- Scan supported scalar columns in a base table in the exact selected database. Compare folded UTF-8 with binary collation.
- MySQL selection opens the row preview captured by the same SELECT. This covers tables without stable row locators without confusing a later row with the match.
- Reuse the 51-hit backend limit and cap previews at 32 columns and 512 characters per value.

## Risks & Rollback

- Hold a metadata lock through the read-only transaction so DDL cannot replace the table during column discovery and scanning. Cancellation kills the checked-out search connection.
- Revert the MySQL override, captured locator variant and UI capability wiring. No migration or persisted format change is needed.

## Task 1 — Native data search and UI entry

Files: `crates/db-wire/src/mysql/{definition_search.rs,data_search.rs,mod.rs}`, `crates/db-wire/src/objects.rs`, `crates/db-wire/tests/mysql_test.rs`, `crates/app/src/workspace/{definition_search/data.rs,navigator.rs,table_view.rs}`.

- [ ] Add this regression and run `FASTDB_TEST_MYSQL_PORT=55448 cargo test -p db-wire --test mysql_test --locked mysql_data_search` in the disposable fixture. Expect the unsupported result to fail.

```rust

#[test]
fn mysql_data_search_matches_literals_and_opens_captured_rows() {
    use futures::FutureExt;
    let Some(mut config) = test_config() else { return; };
    db_wire::get_runtime().block_on(async {
        let schema = format!("fastdb_data_search_{}", std::process::id());
        let admin = MySqlDriver::new(config.clone());
        admin.execute_query(&format!("CREATE DATABASE {schema}")).await.unwrap();
        config.database = schema.clone();
        let driver = MySqlDriver::new(config);
        let result = std::panic::AssertUnwindSafe(async {
            for sql in [
                "CREATE TABLE `odd``name` (value VARCHAR(80), blank TEXT, payload BLOB)",
                "INSERT INTO `odd``name` VALUES ('Needle_% literal', NULL, X'00'), ('needleAB', '', X'6e6565646c655f25')",
                "CREATE VIEW scope_view AS SELECT value FROM `odd``name`",
            ] { driver.execute_query(sql).await.unwrap(); }
            let (cancel, signal) = flume::bounded(1);
            let hits = driver.search_table_data(&schema, "odd`name", "needle_%", signal.clone()).await.unwrap().unwrap();
            assert_eq!(hits.len(), 1);
            assert_eq!(hits[0].column, "value");
            driver.execute_query("DELETE FROM `odd``name`").await.unwrap();
            let row = driver.read_data_search_row(&hits[0].locator, signal.clone()).await.unwrap().unwrap();
            assert_eq!(row.columns, [("value".into(), Some("Needle_% literal".into())), ("blank".into(), None)]);
            assert_eq!(row.omitted_columns, 1);
            assert!(driver.search_table_data(&schema, "scope_view", "needle", signal.clone()).await.unwrap_err().to_string().contains("base table"));
            cancel.send(()).unwrap();
            assert!(driver.search_table_data(&schema, "odd`name", "needle", signal).await.unwrap_err().to_string().contains("cancelled"));
        }).catch_unwind().await;
        admin.execute_query(&format!("DROP DATABASE {schema}")).await.unwrap();
        if let Err(panic) = result { std::panic::resume_unwind(panic); }
    });
}
```

- [x] Extract the existing MySQL definition search timeout/cancel runner as `run_catalog_search`, accepting a static label and a boxed async operation over the checked-out connection. Preserve existing behavior for definition search.
- [x] Add the SQLite-equivalent captured preview variant `DataRowIdentity::MySql(DataRowPreview)` and the following driver module.

```rust
use super::*;
use anyhow::Context as _;
use crate::objects::{
    DATA_PREVIEW_CHARS, DATA_SEARCH_LIMIT, DEFINITION_INPUT_LIMIT, DataRowIdentity, DataRowLocator,
    DataRowPreview, DataSearchHit,
};
use crate::{DbDriverKind, sql::SqlDialect};

const PREVIEW_COLUMNS: usize = 32;
const SNIPPET_LEAD_CHARS: i32 = 96;

impl MySqlDriver {
    pub(super) async fn find_table_data(
        &self,
        schema: &str,
        table: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<Vec<DataSearchHit>>> {
        anyhow::ensure!(
            !needle.trim().is_empty() && [schema, table, needle].iter().all(|value|
                !value.is_empty() && value.len() <= DEFINITION_INPUT_LIMIT && !value.contains('\0')),
            "Enter an exact database and table name and search text up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes"
        );
        let (schema, table, needle) = (schema.to_owned(), table.to_owned(), needle.to_owned());
        self.run_catalog_search("Data search", cancel, move |conn| Box::pin(async move {
            let mut options = TxOpts::default();
            options.with_readonly(Some(true));
            let mut tx = conn.start_transaction(options).await?;
            let result = query_table(&mut tx, &schema, &table, &needle).await;
            tx.rollback().await?;
            result.map(Some)
        })).await
    }
}

async fn query_table(tx: &mut mysql_async::Transaction<'_>, schema: &str, table: &str, needle: &str) -> Result<Vec<DataSearchHit>> {
    let qualified = SqlDialect(DbDriverKind::MySql).qualified_table(schema, table);
    // Resolve the relation before inspecting columns so DDL cannot replace it
    // during the scan. LIMIT 0 reads no user rows.
    tx.query_drop(format!("SELECT 1 FROM {qualified} LIMIT 0")).await?;
    let metadata: Vec<(String, String)> = tx.exec(
        "SELECT c.COLUMN_NAME, c.DATA_TYPE
         FROM information_schema.COLUMNS c
         JOIN information_schema.TABLES t ON c.TABLE_SCHEMA = t.TABLE_SCHEMA AND c.TABLE_NAME = t.TABLE_NAME
           AND CAST(c.TABLE_SCHEMA AS BINARY) = CAST(t.TABLE_SCHEMA AS BINARY)
           AND CAST(c.TABLE_NAME AS BINARY) = CAST(t.TABLE_NAME AS BINARY)
         WHERE c.TABLE_SCHEMA = ? AND c.TABLE_NAME = ?
           AND CAST(c.TABLE_SCHEMA AS BINARY) = CAST(? AS BINARY)
           AND CAST(c.TABLE_NAME AS BINARY) = CAST(? AS BINARY)
           AND t.TABLE_TYPE = 'BASE TABLE'
         ORDER BY c.ORDINAL_POSITION",
        (schema, table, schema, table),
    ).await?;
    anyhow::ensure!(!metadata.is_empty(), "Search requires an existing MySQL base table");
    let total = metadata.len();
    let columns: Vec<String> = metadata.into_iter().filter_map(|(name, kind)| matches!(
        kind.as_str(),
        "char" | "varchar" | "tinytext" | "text" | "mediumtext" | "longtext"
        | "tinyint" | "smallint" | "mediumint" | "int" | "bigint"
        | "decimal" | "float" | "double" | "date" | "datetime" | "timestamp"
        | "time" | "year" | "enum" | "set" | "json"
    ).then_some(name)).collect();
    anyhow::ensure!(!columns.is_empty(), "No supported scalar columns in this table");
    let sql = search_sql(&qualified, &columns);
    let mut rows = tx.exec_iter(sql, (needle, SNIPPET_LEAD_CHARS, DATA_PREVIEW_CHARS, (DATA_SEARCH_LIMIT + 1) as u64)).await?;
    let mut hits = Vec::new();
    while let Some(mut row) = rows.next().await? {
        let index = row.take::<u64, _>(0).context("MySQL match column missing")? as usize;
        let preview = row.take::<String, _>(1).context("MySQL match preview missing")?;
        let mut snapshot = columns.iter().take(PREVIEW_COLUMNS).enumerate()
            .map(|(i, name)| Ok((name.clone(), row.take::<Option<String>, _>(i + 2).context("MySQL preview column missing")?)))
            .collect::<Result<Vec<_>>>()?;
        let column = columns.get(index).context("MySQL match column is invalid")?.clone();
        if index >= PREVIEW_COLUMNS {
            snapshot[PREVIEW_COLUMNS - 1] = (column.clone(), Some(preview.clone()));
        }
        hits.push(DataSearchHit {
            column,
            preview,
            locator: DataRowLocator(DataRowIdentity::MySql(DataRowPreview {
                omitted_columns: total - snapshot.len(),
                columns: snapshot,
            })),
        });
    }
    rows.drop_result().await?;
    Ok(hits)
}

fn search_sql(qualified: &str, columns: &[String]) -> String {
    let dialect = SqlDialect(DbDriverKind::MySql);
    let mut match_arms = Vec::new();
    let mut value_arms = Vec::new();
    let mut previews = Vec::new();
    for (index, name) in columns.iter().enumerate() {
        let value = format!("CONVERT(t.{} USING utf8mb4)", dialect.quote_ident(name));
        match_arms.push(format!(
            "WHEN LOCATE(needle, LOWER({value}) COLLATE utf8mb4_bin) > 0 THEN {index}"
        ));
        value_arms.push(format!("WHEN {index} THEN {value}"));
        if index < PREVIEW_COLUMNS {
            previews.push(format!("LEFT({value}, {DATA_PREVIEW_CHARS}) AS preview_{index}"));
        }
    }
    let projection = (0..previews.len()).map(|index| format!("preview_{index}")).collect::<Vec<_>>().join(", ");
    format!(
        "WITH term AS (SELECT LOWER(CONVERT(? USING utf8mb4)) COLLATE utf8mb4_bin AS needle)
         SELECT /*+ MAX_EXECUTION_TIME(10000) */ matched_index,
             SUBSTRING(matched_value, GREATEST(1, LOCATE(needle, LOWER(matched_value) COLLATE utf8mb4_bin) - ?), ?),
             {projection}
         FROM (
             SELECT needle, CASE {} END AS matched_index,
                 CASE (CASE {} END) {} END AS matched_value, {}
             FROM {qualified} AS t CROSS JOIN term
         ) AS matched WHERE matched_index IS NOT NULL LIMIT ?",
        match_arms.join(" "), match_arms.join(" "), value_arms.join(" "), previews.join(", "),
    )
}
```

- [x] Wire the existing trait methods, enable MySQL in the data picker and footer, and initialize the picker from the saved database. Label MySQL results as captured previews.
- [x] Run the live regression.

## Task 2 — Verification and documentation

- [x] Update the roadmap, provider guide and navigator guide.
- [x] Inspect the native query plan on a large fixture. Review the diff, run formatter, Clippy, workspace tests and app build, and smoke-test startup.
- [x] Obtain independent review.
- [x] Complete manual desktop acceptance. The user confirmed testing passed.

## Verification evidence

- Formatting, workspace Clippy, workspace tests, workspace build and a
  startup smoke check passed. The default workspace test invocation skips
  provider tests that require environment variables.
- The live PostgreSQL 16 and SQLite catalog suite passed all 12 tests.
- Independent review identified a cancellation deadlock under pool saturation.
  The search runner now reuses the dedicated MySQL control connection and
  discards the interrupted connection. Follow-up review found no remaining
  issue on that path. Pool saturation was reviewed by inspection.
- MySQL Community Server 8.4.11 ran from the macOS ARM64 archive in a
  disposable local fixture. All seven tests in
  `cargo test -p db-wire --test mysql_test --locked` passed with
  `FASTDB_TEST_MYSQL_PORT` set. This includes bounded captured previews,
  literal terms, active metadata-lock cancellation and a successful search
  after cancellation.
- On a 100,000-row fixture with three searchable columns, MySQL
  `EXPLAIN ANALYZE` showed the matching scan stopping at 51,000 rows
  after returning 51 hits in 29.2 ms. The no-match search scanned 100,000
  rows in 57.7 ms. Both were single scans without a result sort. The
  no-match cardinality estimate remained high because the literal infix
  predicate has no selectivity estimate. Metadata used dictionary indexes
  for the exact schema and table and sorted three column rows in 0.097 ms.
