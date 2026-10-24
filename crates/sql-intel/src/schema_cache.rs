use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::sync::Mutex;

pub struct SchemaCache {
    conn: Mutex<Connection>,
}

impl SchemaCache {
    pub fn new() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .context("Failed to open SQLite in-memory database for SchemaCache")?;

        conn.execute_batch(
            r#"
            CREATE TABLE tables (
                schema_name TEXT NOT NULL,
                table_name TEXT NOT NULL,
                table_type TEXT NOT NULL,
                PRIMARY KEY (schema_name, table_name)
            );

            CREATE TABLE columns (
                schema_name TEXT NOT NULL,
                table_name TEXT NOT NULL,
                column_name TEXT NOT NULL,
                data_type TEXT NOT NULL,
                is_pk INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (schema_name, table_name, column_name)
            );

            CREATE INDEX idx_tables_name ON tables (table_name);
            CREATE INDEX idx_columns_table ON columns (table_name, column_name);
            "#,
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn add_table(&self, schema: &str, table: &str, table_type: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO tables (schema_name, table_name, table_type) VALUES (?1, ?2, ?3);",
            params![schema, table, table_type],
        )?;
        Ok(())
    }

    pub fn add_column(
        &self,
        schema: &str,
        table: &str,
        column: &str,
        data_type: &str,
        is_pk: bool,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO columns (schema_name, table_name, column_name, data_type, is_pk) VALUES (?1, ?2, ?3, ?4, ?5);",
            params![schema, table, column, data_type, if is_pk { 1 } else { 0 }],
        )?;
        Ok(())
    }

    pub fn search_tables(&self, prefix: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("{}%", prefix);
        let mut stmt = conn.prepare(
            "SELECT table_name FROM tables WHERE table_name LIKE ?1 ORDER BY table_name LIMIT 50;",
        )?;

        let rows = stmt.query_map(params![pattern], |row| row.get(0))?;
        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn search_columns(&self, table: &str, prefix: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().unwrap();
        let pattern = format!("{}%", prefix);
        let mut results = Vec::new();
        if table.is_empty() {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT column_name FROM columns WHERE column_name LIKE ?1 ORDER BY column_name LIMIT 50;",
            )?;
            let rows = stmt.query_map(params![pattern], |row| row.get(0))?;
            for r in rows {
                results.push(r?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT column_name FROM columns WHERE table_name = ?1 AND column_name LIKE ?2 ORDER BY column_name LIMIT 50;",
            )?;
            let rows = stmt.query_map(params![table, pattern], |row| row.get(0))?;
            for r in rows {
                results.push(r?);
            }
        }
        Ok(results)
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch("DELETE FROM tables; DELETE FROM columns;")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_cache_queries() {
        let cache = SchemaCache::new().unwrap();
        cache.add_table("public", "users", "BASE TABLE").unwrap();
        cache.add_table("public", "orders", "BASE TABLE").unwrap();

        cache.add_column("public", "users", "id", "int", true).unwrap();
        cache.add_column("public", "users", "email", "varchar", false).unwrap();
        cache.add_column("public", "orders", "total", "numeric", false).unwrap();

        let tables = cache.search_tables("us").unwrap();
        assert_eq!(tables, vec!["users"]);

        let cols = cache.search_columns("users", "em").unwrap();
        assert_eq!(cols, vec!["email"]);

        let all_cols = cache.search_columns("users", "").unwrap();
        assert_eq!(all_cols.len(), 2);
    }
}
