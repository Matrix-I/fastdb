//! Native visible columns, including generated values omitted by table_info.
use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};

// Rowid aliases cannot store NULL. The legacy inline PRIMARY KEY DESC case
// has a native primary-key index, so it must retain ordinary nullable semantics.
const COLUMNS: &str = r#"
SELECT c.name, c.type, c."notnull", c.dflt_value, c.pk,
       EXISTS(SELECT 1 FROM pragma_foreign_key_list(?1, ?2) f WHERE f."from" = c.name),
       c.hidden, (c.pk > 0 AND c.type = 'INTEGER' COLLATE NOCASE
           AND NOT EXISTS(SELECT 1 FROM pragma_index_list(?1, ?2) p WHERE p.origin = 'pk'))
FROM pragma_table_xinfo(?1, ?2) c
WHERE c.hidden <> 1
ORDER BY c.cid
"#;

// sqlite_schema omits WITHOUT ROWID primary-key entries. index_list also
// includes the implicit UNIQUE/PK indexes whose names must not be filtered out.
const INDEX_NAMES: &str = r#"
SELECT 'main',
       CASE WHEN length(CAST(m.name AS BLOB)) <= ?2 THEN m.name END,
       CASE WHEN length(CAST(i.name AS BLOB)) <= ?2 THEN i.name END
FROM main.sqlite_schema m JOIN pragma_index_list(m.name, 'main') i
WHERE m.type = 'table' AND m.name NOT GLOB 'sqlite_*'
ORDER BY m.name, i.name
LIMIT ?1
"#;

const MAX_SCHEMA_INDEXES: usize = 100_000;
const MAX_INDEX_NAME_BYTES: usize = 128 * 1024;
const MAX_INDEX_METADATA_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn index_names(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    bounded_index_names(conn, MAX_SCHEMA_INDEXES)
}

fn bounded_index_names(conn: &Connection, limit: usize) -> Result<Vec<(String, String, String)>> {
    crate::sql_observer::observe!(
        "SQLite",
        INDEX_NAMES,
        |query| {
            let mut statement = conn.prepare(query)?;
            let mut rows = statement.query(rusqlite::params![limit + 1, MAX_INDEX_NAME_BYTES])?;
            let mut result = Vec::new();
            let mut bytes = 0;
            while let Some(row) = rows.next()? {
                anyhow::ensure!(
                    result.len() < limit,
                    "SQLite index metadata exceeds {limit} objects"
                );
                let table: String = row
                    .get::<_, Option<String>>(1)?
                    .context("SQLite index metadata name exceeds 128 KiB")?;
                let index: String = row
                    .get::<_, Option<String>>(2)?
                    .context("SQLite index metadata name exceeds 128 KiB")?;
                bytes += table.len() + index.len();
                anyhow::ensure!(
                    bytes <= MAX_INDEX_METADATA_BYTES,
                    "SQLite index metadata exceeds 16 MiB"
                );
                result.push((row.get(0)?, table, index));
            }
            Ok(result)
        },
        |rows| Some(rows.len() as u64)
    )
}

// The pragmas expose constraint identities even when the declared name is
// available only in CREATE TABLE. FK seq and PK ordinal select one row per key.
const CONSTRAINT_NAMES: &str = r#"
WITH tables AS NOT MATERIALIZED (
    SELECT name FROM main.sqlite_schema
    WHERE type = 'table' AND name NOT GLOB 'sqlite_*'
), constraints(table_name, name, kind) AS (
    SELECT t.name, 'PRIMARY KEY', 'PRIMARY KEY'
    FROM tables t CROSS JOIN pragma_table_xinfo(t.name, 'main') c WHERE c.pk = 1
    UNION ALL
    SELECT t.name, i.name, 'UNIQUE'
    FROM tables t CROSS JOIN pragma_index_list(t.name, 'main') i WHERE i.origin = 'u'
    UNION ALL
    SELECT t.name, 'fk_' || f.id, 'FOREIGN KEY'
    FROM tables t CROSS JOIN pragma_foreign_key_list(t.name, 'main') f WHERE f.seq = 0
)
SELECT CASE WHEN length(CAST(table_name AS BLOB)) <= ?2 THEN table_name END,
       CASE WHEN length(CAST(name AS BLOB)) <= ?2 THEN name END,
       kind
FROM constraints
LIMIT ?1
"#;

pub(super) fn constraint_names(conn: &Connection) -> Result<Vec<(String, String, String, String)>> {
    const MAX_CONSTRAINTS: usize = 100_000;
    const MAX_NAME_BYTES: usize = 128 * 1024;
    const MAX_METADATA_BYTES: usize = 16 * 1024 * 1024;
    crate::sql_observer::observe!(
        "SQLite",
        CONSTRAINT_NAMES,
        |query| {
            let mut statement = conn.prepare(query)?;
            let mut rows =
                statement.query(rusqlite::params![MAX_CONSTRAINTS + 1, MAX_NAME_BYTES])?;
            let mut result = Vec::new();
            let mut bytes = 0;
            while let Some(row) = rows.next()? {
                anyhow::ensure!(
                    result.len() < MAX_CONSTRAINTS,
                    "SQLite constraint metadata exceeds {MAX_CONSTRAINTS} objects"
                );
                let table = row
                    .get::<_, Option<String>>(0)?
                    .context("SQLite constraint table name exceeds 128 KiB")?;
                let name = row
                    .get::<_, Option<String>>(1)?
                    .context("SQLite constraint name exceeds 128 KiB")?;
                bytes += table.len() + name.len();
                anyhow::ensure!(
                    bytes <= MAX_METADATA_BYTES,
                    "SQLite constraint metadata names exceed 16 MiB"
                );
                result.push(("main".to_owned(), table, name, row.get(2)?));
            }
            result.sort_unstable();
            Ok(result)
        },
        |rows| Some(rows.len() as u64)
    )
}

// WITHOUT ROWID primary keys are reported by index_list even when they have
// no sqlite_schema index row. index_xinfo excludes auxiliary storage columns.
const INDEX_DETAILS: &str = r#"
SELECT CASE WHEN length(CAST(m.name AS BLOB)) <= ?2 THEN m.name END AS table_name,
       CASE WHEN length(CAST(i.name AS BLOB)) <= ?2 THEN i.name END AS index_name,
       i."unique", i.origin, x.cid,
       CASE WHEN length(CAST(x.name AS BLOB)) <= ?2 THEN x.name END,
       x."desc",
       CASE WHEN length(CAST(x.coll AS BLOB)) <= ?2 THEN x.coll END,
       CASE WHEN x.seqno = 0 AND length(CAST(d.sql AS BLOB)) <= ?2 THEN d.sql END,
       x.seqno = 0 AND coalesce(length(CAST(d.sql AS BLOB)) > ?2, 0), i.partial
FROM main.sqlite_schema m
JOIN pragma_index_list(m.name, 'main') i
JOIN pragma_index_xinfo(i.name, 'main') x
LEFT JOIN main.sqlite_schema d ON d.type = 'index' AND d.name = i.name
WHERE m.type = 'table' AND m.name NOT GLOB 'sqlite_*' AND x."key" = 1
ORDER BY table_name, index_name, x.seqno
LIMIT ?1
"#;

#[derive(Default)]
struct IndexCatalog {
    tables: BTreeMap<String, crate::TableDetails>,
    full_indexes: HashMap<String, HashSet<String>>,
}

fn index_details(conn: &Connection) -> Result<IndexCatalog> {
    use crate::{IndexDetails, TableDetails, sql::SqlDialect};
    const MAX_KEY_ROWS: usize = 200_000;
    let dialect = SqlDialect(crate::DbDriverKind::Sqlite);
    crate::sql_observer::observe!(
        "SQLite",
        INDEX_DETAILS,
        |query| {
            let mut statement = conn.prepare(query)?;
            let mut rows =
                statement.query(rusqlite::params![MAX_KEY_ROWS + 1, MAX_INDEX_NAME_BYTES])?;
            let mut catalog = IndexCatalog::default();
            let mut current_table: Option<(String, TableDetails)> = None;
            let mut key_rows = 0;
            let mut bytes = 0;
            while let Some(row) = rows.next()? {
                key_rows += 1;
                anyhow::ensure!(
                    key_rows <= MAX_KEY_ROWS,
                    "SQLite index details exceed {MAX_KEY_ROWS} keys"
                );
                let mut field = |column| -> Result<Option<&str>> {
                    match row.get_ref(column)? {
                        rusqlite::types::ValueRef::Null => Ok(None),
                        value => {
                            let text = value.as_str()?;
                            bytes += text.len();
                            anyhow::ensure!(
                                bytes <= MAX_INDEX_METADATA_BYTES,
                                "SQLite index details exceed 16 MiB"
                            );
                            Ok(Some(text))
                        }
                    }
                };
                let table = field(0)?.context("SQLite index table name exceeds 128 KiB")?;
                let name = field(1)?.context("SQLite index name exceeds 128 KiB")?;
                let column = field(5)?;
                let collation =
                    field(7)?.context("SQLite index collation is missing or exceeds 128 KiB")?;
                anyhow::ensure!(!row.get::<_, bool>(9)?, "SQLite index SQL exceeds 128 KiB");
                let source = field(8)?;
                if current_table.as_ref().is_none_or(|(name, _)| name != table)
                    && let Some((name, details)) =
                        current_table.replace((table.into(), TableDetails::default()))
                {
                    catalog.tables.insert(name, details);
                }
                let indexes = &mut current_table.as_mut().unwrap().1.indexes;
                if indexes.last().is_none_or(|index| index.name != name) {
                    let definition = match (source, row.get_ref(3)?.as_str()?) {
                        (Some(sql), _) => sql.into(),
                        (None, "pk") => "Implicit primary-key index".into(),
                        (None, "u") => "Implicit unique-constraint index".into(),
                        _ => "Definition unavailable".into(),
                    };
                    if !row.get::<_, bool>(10)? {
                        catalog
                            .full_indexes
                            .entry(table.into())
                            .or_default()
                            .insert(name.into());
                    }
                    indexes.push(IndexDetails {
                        name: name.into(),
                        method: "B-tree".into(),
                        columns: Vec::new(),
                        unique: row.get(2)?,
                        valid: true,
                        definition,
                    });
                }
                let key = match (column, row.get::<_, i32>(4)?) {
                    (Some(column), _) => dialect.quote_ident(column),
                    (None, -2) => "(expression)".into(),
                    (None, -1) => "rowid".into(),
                    _ => anyhow::bail!("Missing SQLite index key name"),
                };
                let direction = if row.get::<_, bool>(6)? {
                    "DESC"
                } else {
                    "ASC"
                };
                indexes
                    .last_mut()
                    .unwrap()
                    .columns
                    .push(format!("{key} COLLATE {collation} {direction}"));
            }
            if let Some((name, details)) = current_table {
                catalog.tables.insert(name, details);
            }
            Ok(catalog)
        },
        |catalog| Some(catalog.tables.len() as u64)
    )
}

const HAS_STATS: &str = "SELECT EXISTS (SELECT 1 FROM main.sqlite_schema WHERE type = 'table' AND name = 'sqlite_stat1')";

// Check full-index membership against the already bounded index catalog.
const TABLE_ESTIMATES: &str = r#"
SELECT CASE WHEN length(CAST(s.tbl AS BLOB)) <= ?2 THEN s.tbl END,
       s.idx IS NULL OR s.idx = s.tbl,
       CASE WHEN length(CAST(s.stat AS BLOB)) <= ?2 THEN s.stat END,
       CASE WHEN length(CAST(s.idx AS BLOB)) <= ?2 THEN s.idx END
FROM main.sqlite_stat1 s
JOIN main.sqlite_schema m ON m.type = 'table' AND m.name = s.tbl
WHERE m.name NOT GLOB 'sqlite_*'
LIMIT ?1
"#;

pub(super) fn table_details(
    conn: &Connection,
) -> Result<Vec<(String, String, crate::TableDetails)>> {
    let IndexCatalog {
        mut tables,
        full_indexes,
    } = index_details(conn)?;
    for (name, rows) in table_estimates(conn, &full_indexes)? {
        tables.entry(name).or_default().estimated_rows = Some(rows);
    }
    Ok(tables
        .into_iter()
        .map(|(name, details)| ("main".into(), name, details))
        .collect())
}

fn table_estimates(
    conn: &Connection,
    full_indexes: &HashMap<String, HashSet<String>>,
) -> Result<Vec<(String, i64)>> {
    let exists: bool = crate::sql_observer::observe!("SQLite", HAS_STATS, |query| conn.query_row(
        query,
        [],
        |row| row.get(0)
    ))?;
    if !exists {
        return Ok(Vec::new());
    }
    crate::sql_observer::observe!(
        "SQLite",
        TABLE_ESTIMATES,
        |query| {
            let mut statement = conn.prepare(query)?;
            let mut rows = statement.query(rusqlite::params![
                MAX_SCHEMA_INDEXES + 1,
                MAX_INDEX_NAME_BYTES
            ])?;
            let mut estimates = std::collections::BTreeMap::<String, (bool, i64)>::new();
            let mut count = 0;
            let mut bytes = 0;
            while let Some(row) = rows.next()? {
                count += 1;
                anyhow::ensure!(
                    count <= MAX_SCHEMA_INDEXES,
                    "SQLite statistics exceed {MAX_SCHEMA_INDEXES} entries"
                );
                let table = row
                    .get_ref(0)?
                    .as_str()
                    .context("SQLite statistics table name is invalid or exceeds 128 KiB")?;
                let direct: bool = row.get(1)?;
                if !direct {
                    let Ok(index) = row.get_ref(3)?.as_str() else {
                        continue;
                    };
                    if !full_indexes
                        .get(table)
                        .is_some_and(|indexes| indexes.contains(index))
                    {
                        continue;
                    }
                }
                let Ok(stat) = row.get_ref(2)?.as_str() else {
                    continue;
                };
                bytes += table.len() + stat.len();
                anyhow::ensure!(
                    bytes <= MAX_INDEX_METADATA_BYTES,
                    "SQLite statistics exceed 16 MiB"
                );
                let Some(rows) = stat
                    .split_ascii_whitespace()
                    .next()
                    .filter(|token| token.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|token| token.parse::<i64>().ok())
                else {
                    continue;
                };
                let candidate = (direct, rows);
                if let Some(current) = estimates.get_mut(table) {
                    *current = (*current).max(candidate);
                } else {
                    estimates.insert(table.into(), candidate);
                }
            }
            Ok(estimates
                .into_iter()
                .map(|(name, (_, rows))| (name, rows))
                .collect::<Vec<_>>())
        },
        |rows| Some(rows.len() as u64)
    )
}

pub(super) fn columns(
    conn: &Connection,
    schema: &str,
    table: &str,
) -> Result<Vec<(ColumnMeta, bool)>> {
    Ok(crate::sql_observer::observe!(
        "SQLite",
        COLUMNS,
        |query| {
            let mut statement = conn.prepare(query)?;
            let rows = statement.query_map(rusqlite::params![table, schema], |row| {
                let declared: String = row.get(1)?;
                let declared_nullable = row.get::<_, i32>(2)? == 0;
                let rowid_alias: bool = row.get(7)?;
                let mut column =
                    ColumnMeta::new(row.get::<_, String>(0)?, sqlite_decl_data_type(&declared))
                        .with_raw_type(declared)
                        .with_nullable(declared_nullable && !rowid_alias)
                        .with_pk(row.get::<_, i32>(4)? > 0)
                        .with_fk(row.get(5)?);
                column.default_val = row.get(3)?;
                column.is_generated = matches!(row.get::<_, i32>(6)?, 2 | 3);
                column.generates_key_on_null = rowid_alias;
                Ok((column, declared_nullable))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        },
        |rows| Some(rows.len() as u64)
    )?)
}

const RELATION_DDL: &str = r#"
SELECT type, CASE WHEN length(CAST(sql AS BLOB)) <= ?2 THEN sql END
FROM main.sqlite_schema
WHERE tbl_name = ?1 AND sql IS NOT NULL AND type IN ('table', 'view', 'index', 'trigger')
ORDER BY CASE type WHEN 'table' THEN 0 WHEN 'view' THEN 0 WHEN 'index' THEN 1 ELSE 2 END, name
LIMIT ?3
"#;

pub(super) fn relation_ddl(conn: &Connection, table: &str) -> Result<Option<String>> {
    const MAX_OBJECTS: usize = 256;
    const MAX_BYTES: usize = 1024 * 1024;
    crate::sql_observer::observe!("SQLite", RELATION_DDL, |query| {
        let mut statement = conn.prepare(query)?;
        let mut rows = statement.query(rusqlite::params![table, MAX_BYTES, MAX_OBJECTS + 1])?;
        let mut source = String::new();
        let mut count = 0;
        while let Some(row) = rows.next()? {
            let kind: String = row.get(0)?;
            if count == 0 && !matches!(kind.as_str(), "table" | "view") {
                return Ok(None);
            }
            count += 1;
            anyhow::ensure!(
                count <= MAX_OBJECTS,
                "Relation DDL exceeds {MAX_OBJECTS} objects"
            );
            let sql: String = row
                .get::<_, Option<String>>(1)?
                .context("Relation DDL exceeds 1 MiB")?;
            anyhow::ensure!(
                source.len() + sql.len() + 2 <= MAX_BYTES,
                "Relation DDL exceeds 1 MiB"
            );
            source.push_str(sql.trim_end().trim_end_matches(';'));
            source.push_str(";\n");
        }
        Ok((count > 0).then_some(source))
    })
}

impl SqliteDriver {
    pub(super) async fn native_columns(
        &self,
        schema: &str,
        table: &str,
    ) -> Result<Vec<ColumnMeta>> {
        let schema = if schema.is_empty() { "main" } else { schema }.to_owned();
        let table = table.to_owned();
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            Ok(columns(&conn.lock(), &schema, &table)?
                .into_iter()
                .map(|(column, _)| column)
                .collect())
        })
        .await
        .context("SQLite column metadata worker failed")?
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COLUMNS, INDEX_NAMES, MAX_INDEX_NAME_BYTES, MAX_SCHEMA_INDEXES, SqliteDriver,
        bounded_index_names, index_names,
    };

    #[test]
    fn index_reader_handles_quoted_names_and_reports_object_and_name_limits() {
        let driver = SqliteDriver::new_memory();
        let conn = driver.conn.lock();
        conn.execute_batch("CREATE TABLE \"quoted\"\" 名\" (id INTEGER PRIMARY KEY, label TEXT UNIQUE); CREATE INDEX \"idx\"\"quoted\" ON \"quoted\"\" 名\"(label)").unwrap();
        let names = index_names(&conn).unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(
            names[0],
            ("main".into(), "quoted\" 名".into(), "idx\"quoted".into())
        );
        assert!(
            bounded_index_names(&conn, 1)
                .unwrap_err()
                .to_string()
                .contains("1 objects")
        );
        let oversized = "n".repeat(MAX_INDEX_NAME_BYTES + 1);
        conn.execute_batch(&format!(
            "CREATE INDEX {oversized} ON \"quoted\"\" 名\"(label)"
        ))
        .unwrap();
        assert!(
            index_names(&conn)
                .unwrap_err()
                .to_string()
                .contains("128 KiB")
        );
    }

    #[test]
    fn index_reader_enforces_the_aggregate_name_budget_without_partial_results() {
        // Each row contains two 128 KiB names, so 64 rows exactly fill 16 MiB.
        const ROWS_AT_BUDGET: usize = 64;
        let driver = SqliteDriver::new_memory();
        let conn = driver.conn.lock();
        let table = "t".repeat(MAX_INDEX_NAME_BYTES);
        conn.execute_batch(&format!("CREATE TABLE {table} (id INTEGER)"))
            .unwrap();
        for n in 0..ROWS_AT_BUDGET {
            let index = format!("i{n:03}{}", "x".repeat(MAX_INDEX_NAME_BYTES - 4));
            conn.execute_batch(&format!("CREATE INDEX {index} ON {table}(id)"))
                .unwrap();
        }
        assert_eq!(index_names(&conn).unwrap().len(), ROWS_AT_BUDGET);
        let index = "z".repeat(MAX_INDEX_NAME_BYTES);
        conn.execute_batch(&format!("CREATE INDEX {index} ON {table}(id)"))
            .unwrap();
        assert!(
            index_names(&conn)
                .unwrap_err()
                .to_string()
                .contains("16 MiB")
        );
    }

    #[test]
    fn schema_index_reader_uses_native_metadata_at_large_catalog_volume() {
        let driver = SqliteDriver::new_memory();
        let conn = driver.conn.lock();
        for n in 0..1000 {
            conn.execute_batch(&format!(
                "CREATE TABLE fixture_{n} (id TEXT PRIMARY KEY, label TEXT UNIQUE) WITHOUT ROWID"
            ))
            .unwrap();
        }
        let plan = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {INDEX_NAMES}"))
            .unwrap()
            .query_map(
                rusqlite::params![MAX_SCHEMA_INDEXES + 1, MAX_INDEX_NAME_BYTES],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(plan.iter().any(|line| line.contains("VIRTUAL TABLE")));
        let start = std::time::Instant::now();
        for _ in 0..20 {
            let indexes = index_names(&conn).unwrap();
            assert_eq!(indexes.len(), 2000);
            assert!(
                indexes
                    .iter()
                    .all(|(_, _, name)| name.starts_with("sqlite_autoindex_"))
            );
        }
        let version: String = conn
            .query_row("SELECT sqlite_version()", [], |row| row.get(0))
            .unwrap();
        eprintln!(
            "SQLite {version}; 1000 WITHOUT ROWID tables, 2000 native implicit indexes; 20 warm schema reads: {:?}; plan: {plan:?}",
            start.elapsed()
        );
    }

    #[test]
    fn scoped_column_reader_uses_native_metadata_at_large_schema_volume() {
        let driver = SqliteDriver::new_memory();
        let conn = driver.conn.lock();
        conn.execute_batch("CREATE TABLE parent (id INTEGER PRIMARY KEY)")
            .unwrap();
        for n in 0..1000 {
            conn.execute_batch(&format!(
                "CREATE TABLE fixture_{n} (id INTEGER, label TEXT)"
            ))
            .unwrap();
        }
        let columns = (0..60)
            .map(|n| format!("c{n} TEXT"))
            .collect::<Vec<_>>()
            .join(",");
        conn.execute_batch(&format!(
            "CREATE TABLE target ({columns}, FOREIGN KEY(c0) REFERENCES parent(id))"
        ))
        .unwrap();
        let plan: Vec<String> = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {COLUMNS}"))
            .unwrap()
            .query_map(["target", "main"], |row| row.get(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(plan.iter().any(|line| line.contains("VIRTUAL TABLE")));
        let mut statement = conn.prepare(COLUMNS).unwrap();
        let start = std::time::Instant::now();
        for _ in 0..100 {
            let columns: Vec<(String, bool)> = statement
                .query_map(["target", "main"], |row| Ok((row.get(0)?, row.get(5)?)))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap();
            assert_eq!(columns.len(), 60);
            assert_eq!(columns[0], ("c0".into(), true));
            assert!(columns[1..].iter().all(|(_, foreign)| !foreign));
        }
        let elapsed = start.elapsed();
        let version: String = conn
            .query_row("SELECT sqlite_version()", [], |row| row.get(0))
            .unwrap();
        eprintln!(
            "SQLite {version}; 1000 unrelated tables, 60 target columns; 100 warm reads: {elapsed:?}; plan: {plan:?}"
        );
    }
}
