//! Native visible columns, including generated values omitted by table_info.
use super::*;

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
    use super::{COLUMNS, SqliteDriver};

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
