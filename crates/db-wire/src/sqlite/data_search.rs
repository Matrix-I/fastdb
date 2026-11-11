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
                 ELSE CAST(substr(CAST(substr(CAST({value} AS TEXT), 1, ?2) AS BLOB), 1, ?5) AS TEXT) END"
            ));
        }
    }
    format!(
        "SELECT matched_index,
                CAST(substr(CAST(substr(matched_value, max(1, instr(lower(matched_value), lower(?1)) - ?3), ?2) AS BLOB), 1, ?5) AS TEXT),
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
        (DATA_SEARCH_LIMIT + 1) as i64,
        DATA_PREVIEW_CHARS * 4,
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
        ValueRef::Text(bytes) => Ok(Some(
            String::from_utf8_lossy(bytes)
                .chars()
                .take(DATA_PREVIEW_CHARS as usize)
                .collect(),
        )),
        _ => anyhow::bail!("Unexpected SQLite preview type"),
    }
}
