# SQLite table data search implementation plan

## Status audit — 2026-11-12

Source baseline: `76e8815`. The provider driver and native regression tests
are implemented. Implementation boxes confirmed from source are checked;
unchecked reproduction, review, verification and commit steps are not inferred
from source alone. Existing measurement/acceptance evidence below remains
historical, not a fresh test run. Navigator definition/multi-table search
entries have since been removed; current data search opens from a table footer.
See the [current P2 checklist](2026-11-10-p2-search-plans-transfer.md).

> **For agentic workers:** REQUIRED SUB-SKILL: development-superpowers:subagent-driven-development or executing-plans-strict. Steps use `- [ ]`.

**Goal / Architecture / Tech Stack / Branch:** Extend P2-04 to SQLite table values through the shared GPUI scope picker and native rusqlite driver. Worktree is on main. Commit policy is explicit authorization only.

## Confirmed Requirements

- P2-04 covers PostgreSQL, MySQL and SQLite. Search selected tables with bounded results, progress, cancel and navigation to the matching row.
- The user approved additive search APIs and result kinds.

## Global Constraints

- Preserve PostgreSQL physical row reopening. No new dependencies or persistent state.
- Use bound search terms and dialect-quoted catalog identifiers. Keep GUI acceptance manual.
- Verify with fmt, workspace Clippy, workspace tests, app build and startup.

## Technical Defaults

- SQLite uses the saved `main` catalog. Match ASCII-folded text, integer and real values. Skip SQL NULL, BLOB and text containing NUL as search terms.
- Capture a bounded preview in the same statement as the match. SQLite has no row revision token that can prove a later read is the same version, so navigation presents the observed row, including keyless and WITHOUT ROWID tables.
- Use the same 50-hit display limit and 51st overflow hit. Preview up to 32 columns and 512 characters per value.

## Risks & Rollback

- Bound lock waits and scan time through the existing SQLite catalog worker. Native database reads may contend with writes.
- Revert the SQLite overrides and UI capability changes. No file or database migration is needed.

## Task 1 — Bounded SQLite search and result selection

Files: `crates/db-wire/src/sqlite/{data_search.rs,definition_search.rs,mod.rs}`, `crates/db-wire/src/{objects.rs,postgres/data_search.rs}`, `crates/db-wire/tests/catalog_test.rs`, `crates/app/src/workspace/{definition_search/data.rs,navigator.rs,table_view.rs}`.

Interface: existing `DatabaseDriver::search_table_data` and `read_data_search_row`. Keep the opaque `DataRowLocator` API and store SQLite previews in a private enum.

- [ ] Add the test below and run `cargo test -p db-wire --test catalog_test --locked sqlite_data_search`. Expect the unsupported result to fail.

```rust

#[tokio::test]
async fn sqlite_data_search_returns_captured_keyless_and_without_rowid_matches() {
    let driver = SqliteDriver::new_memory();
    for sql in [
        r#"CREATE TABLE "odd""table" (label, blank, bytes)"#,
        r#"INSERT INTO "odd""table" VALUES ('Needle_% literal', NULL, X'6e6565646c655f25'), ('needleAB', '', X'6e6565646c655f25')"#,
        "CREATE TABLE keyed (a TEXT, b INTEGER, PRIMARY KEY (a, b)) WITHOUT ROWID",
        "INSERT INTO keyed VALUES ('Needle_% keyed', 1)",
    ] {
        driver.execute_query(sql).await.unwrap();
    }
    let (cancel, signal) = flume::bounded(1);
    let keyless = driver.search_table_data("main", r#"odd"table"#, "needle_%", signal.clone()).await.unwrap().unwrap();
    assert_eq!(keyless.len(), 1);
    assert_eq!(keyless[0].column, "label");
    let keyed = driver.search_table_data("main", "keyed", "needle_%", signal.clone()).await.unwrap().unwrap();
    assert_eq!(keyed.len(), 1);
    driver.execute_query(r#"DELETE FROM "odd""table""#).await.unwrap();
    let preview = driver.read_data_search_row(&keyless[0].locator, signal.clone()).await.unwrap().unwrap();
    assert_eq!(preview.columns, [
        ("label".into(), Some("Needle_% literal".into())),
        ("blank".into(), None),
        ("bytes".into(), Some("[BLOB]".into())),
    ]);
    assert!(driver.search_table_data("temp", "keyed", "needle", signal.clone()).await.is_err());
    cancel.send(()).unwrap();
    assert!(driver.search_table_data("main", "keyed", "needle", signal).await.unwrap_err().to_string().contains("cancelled"));
}
```

- [x] Extract a private SQLite `run_catalog_read` worker from the definition adapter. It takes the database, owned handle, cancellation receiver, operation label and synchronous query callback. Keep the existing read-only flags, bounded waits and progress cleanup.
- [x] Store existing PostgreSQL fields as `DataRowIdentity::Postgres(PostgresRowLocator)`. Add `DataRowIdentity::Sqlite(DataRowPreview)` and return its captured preview on selection.
- [x] Add the SQLite search module below and wire both existing driver methods.

```rust
use super::*;
use crate::objects::{
    DATA_PREVIEW_CHARS, DATA_SEARCH_LIMIT, DEFINITION_INPUT_LIMIT, DataRowIdentity, DataRowLocator,
    DataRowPreview, DataSearchHit,
};
use crate::{DbDriverKind, sql::SqlDialect};

const PREVIEW_COLUMNS: usize = 32;
const SNIPPET_LEAD_CHARS: i32 = 96;

pub(super) async fn search(
    database: PathBuf,
    owned: Arc<Mutex<Connection>>,
    schema: &str,
    table: &str,
    needle: &str,
    cancel: flume::Receiver<()>,
) -> Result<Option<Vec<DataSearchHit>>> {
    anyhow::ensure!(
        schema == "main",
        "SQLite data search requires the main schema"
    );
    anyhow::ensure!(
        !table.is_empty()
            && !needle.trim().is_empty()
            && [table, needle]
                .iter()
                .all(|value| value.len() <= DEFINITION_INPUT_LIMIT && !value.contains('\0')),
        "Enter an exact table name and search text up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes"
    );
    let (table, needle) = (table.to_owned(), needle.to_owned());
    definition_search::run_catalog_read(database, owned, cancel, "Data search", move |conn| {
        find(conn, &table, &needle).map(Some)
    })
    .await
}

fn find(conn: &Connection, table: &str, needle: &str) -> Result<Vec<DataSearchHit>> {
    // Resolve metadata and read matches from one SQLite snapshot.
    anyhow::ensure!(
        conn.is_autocommit(),
        "Finish the current SQLite transaction before searching"
    );
    let snapshot = conn.unchecked_transaction()?;
    let conn = &*snapshot;
    let dialect = SqlDialect(DbDriverKind::Sqlite);
    let table_list = format!("PRAGMA main.table_list({})", dialect.quote_ident(table));
    let exists = conn
        .prepare(&table_list)?
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .iter()
        .any(|(name, kind)| name == table && kind == "table");
    anyhow::ensure!(exists, "Search requires an existing ordinary SQLite table");
    let pragma = format!("PRAGMA main.table_xinfo({})", dialect.quote_ident(table));
    let mut metadata = conn.prepare(&pragma)?;
    let columns = metadata
        .query_map([], |row| {
            Ok((row.get::<_, String>(1)?, row.get::<_, i32>(6)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let columns: Vec<String> = columns
        .into_iter()
        .filter_map(|(name, hidden)| (hidden != 1).then_some(name))
        .collect();
    anyhow::ensure!(!columns.is_empty(), "No searchable columns in this table");
    query_hits(conn, table, needle, &columns)
}

fn search_sql(table: &str, columns: &[String]) -> String {
    let dialect = SqlDialect(DbDriverKind::Sqlite);
    let mut match_arms = Vec::new();
    let mut value_arms = Vec::new();
    let mut previews = Vec::new();
    for (index, name) in columns.iter().enumerate() {
        let name = dialect.quote_ident(name);
        let value = format!("t.{name}");
        match_arms.push(format!(
            "WHEN typeof({value}) IN ('integer','real','text')
             AND instr(CAST({value} AS TEXT), char(0)) = 0
             AND instr(lower(CAST({value} AS TEXT)), lower(?1)) > 0 THEN {index}"
        ));
        value_arms.push(format!("WHEN {index} THEN CAST({value} AS TEXT)"));
        if index < PREVIEW_COLUMNS {
            previews.push(format!(
                "CASE WHEN typeof({value}) = 'blob' THEN '[BLOB]'
                 WHEN instr(CAST({value} AS TEXT), char(0)) > 0 THEN '[TEXT contains NUL]'
                 ELSE substr(CAST({value} AS TEXT), 1, ?2) END"
            ));
        }
    }
    format!(
        "SELECT matched_index,
                substr(matched_value, max(1, instr(lower(matched_value), lower(?1)) - ?3), ?2),
                {}
         FROM (
             SELECT CASE {} END AS matched_index,
                    CASE (CASE {} END) {} END AS matched_value,
                    {}
             FROM main.{} AS t
         ) WHERE matched_index IS NOT NULL LIMIT ?4",
        (0..previews.len())
            .map(|i| format!("preview_{i}"))
            .collect::<Vec<_>>()
            .join(", "),
        match_arms.join(" "),
        match_arms.join(" "),
        value_arms.join(" "),
        previews
            .into_iter()
            .enumerate()
            .map(|(i, preview)| format!("{preview} AS preview_{i}"))
            .collect::<Vec<_>>()
            .join(", "),
        dialect.quote_ident(table),
    )
}

fn query_hits(
    conn: &Connection,
    table: &str,
    needle: &str,
    columns: &[String],
) -> Result<Vec<DataSearchHit>> {
    let sql = search_sql(table, columns);
    let mut statement = conn.prepare(&sql)?;
    let mut rows = statement.query(rusqlite::params![
        needle,
        DATA_PREVIEW_CHARS,
        SNIPPET_LEAD_CHARS,
        (DATA_SEARCH_LIMIT + 1) as i64
    ])?;
    let mut hits = Vec::new();
    while let Some(row) = rows.next()? {
        let index: usize = row.get(0)?;
        let preview = display_text(row, 1)?.context("SQLite match has no preview")?;
        let mut snapshot = columns
            .iter()
            .take(PREVIEW_COLUMNS)
            .enumerate()
            .map(|(i, name)| Ok((name.clone(), display_text(row, i + 2)?)))
            .collect::<Result<Vec<_>>>()?;
        let column = columns[index].clone();
        if index >= PREVIEW_COLUMNS {
            snapshot[PREVIEW_COLUMNS - 1] = (column.clone(), Some(preview.clone()));
        }
        hits.push(DataSearchHit {
            column,
            preview,
            locator: DataRowLocator(DataRowIdentity::Sqlite(DataRowPreview {
                omitted_columns: columns.len() - snapshot.len(),
                columns: snapshot,
            })),
        });
    }
    Ok(hits)
}


fn display_text(row: &rusqlite::Row<'_>, index: usize) -> Result<Option<String>> {
    match row.get_ref(index)? {
        ValueRef::Null => Ok(None),
        // SQLite can store arbitrary bytes as TEXT. Replace malformed sequences
        // for display so another cell cannot invalidate the entire match.
        ValueRef::Text(bytes) => Ok(Some(String::from_utf8_lossy(bytes).into_owned())),
        _ => anyhow::bail!("Unexpected SQLite preview type"),
    }
}
```

- [ ] Enable the existing data picker and table action for SQLite. Default navigator scope to `main` and recognize SQLite table metadata as `table`. Identify selected SQLite results as captured previews.
- [ ] Rerun the native regression.
- [ ] Ready to commit: driver, app and test files — `feat(db-wire): search SQLite table data`.

## Task 2 — Verification and docs

- [x] Update navigator guide, provider guide, remaining features and roadmap.
- [ ] Review full diff and untracked files. Run formatter, workspace Clippy, workspace tests and app build. Inspect EXPLAIN QUERY PLAN on a large SQLite fixture and smoke the app process.
- [ ] Obtain independent review and rerun final gate after changes. Report manual GUI acceptance pending.

## Query plan evidence

SQLite 3.54, in-memory fixture with 100,000 rows and two searchable columns.
`EXPLAIN QUERY PLAN` reports one `SCAN t` and no temporary sort. A literal
infix match returned 51 rows in 22.318 ms. A no-match scan returned zero rows
in 43.166 ms. The search computes the match against cell text, so the
primary-key index does not satisfy the predicate. Result navigation reads
the preview captured by the same statement.
