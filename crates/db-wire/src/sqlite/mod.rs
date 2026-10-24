use std::sync::Arc;
use std::time::Instant;
use anyhow::{Context, Result};
use async_trait::async_trait;
use mem_engine::{ColumnMeta, DataType, QueryResultPager};
use parking_lot::Mutex;
use rusqlite::types::ValueRef;
use rusqlite::Connection;

use crate::traits::{ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary};

pub struct SqliteDriver {
    pub config: ConnectionConfig,
    conn: Arc<Mutex<Connection>>,
    interrupt_handle: Arc<Mutex<rusqlite::InterruptHandle>>,
}

impl SqliteDriver {
    pub fn new(config: ConnectionConfig) -> Result<Self> {
        let conn = if config.database == ":memory:" || config.database.is_empty() {
            Connection::open_in_memory()?
        } else {
            Connection::open(&config.database)
                .with_context(|| format!("Failed to open SQLite database: {}", config.database))?
        };

        let interrupt_handle = conn.get_interrupt_handle();

        Ok(Self {
            config,
            conn: Arc::new(Mutex::new(conn)),
            interrupt_handle: Arc::new(Mutex::new(interrupt_handle)),
        })
    }

    pub fn new_memory() -> Self {
        let mut config = ConnectionConfig::default();
        config.database = ":memory:".to_string();
        Self::new(config).expect("Failed to initialize in-memory SQLite database")
    }
}

#[async_trait]
impl DatabaseDriver for SqliteDriver {
    async fn ping(&self) -> Result<()> {
        let conn = self.conn.lock();
        conn.execute("SELECT 1;", [])?;
        Ok(())
    }

    async fn execute_streaming(
        &self,
        sql: &str,
        pager: Arc<QueryResultPager>,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let conn = self.conn.lock();

        let mut stmt = match conn.prepare(sql) {
            Ok(s) => s,
            Err(e) => {
                let err_msg = format!("SQLite prepare error: {e}");
                let _ = event_tx.send(StreamEvent::Error(err_msg.clone()));
                anyhow::bail!(err_msg);
            }
        };

        let col_count = stmt.column_count();
        let columns: Vec<ColumnMeta> = stmt
            .column_names()
            .iter()
            .map(|name| ColumnMeta::new(*name, DataType::Text))
            .collect();

        let _ = event_tx.send(StreamEvent::SchemaReady(columns));

        let mut rows = match stmt.query([]) {
            Ok(r) => r,
            Err(e) => {
                let err_msg = format!("SQLite query error: {e}");
                let _ = event_tx.send(StreamEvent::Error(err_msg.clone()));
                anyhow::bail!(err_msg);
            }
        };

        let mut total_rows = 0u64;
        let mut last_event_instant = Instant::now();
        let mut cell_buffers: Vec<Option<Vec<u8>>> = vec![None; col_count];

        while let Some(row) = rows.next()? {
            for i in 0..col_count {
                cell_buffers[i] = match row.get_ref(i)? {
                    ValueRef::Null => None,
                    ValueRef::Integer(v) => Some(v.to_string().into_bytes()),
                    ValueRef::Real(v) => Some(v.to_string().into_bytes()),
                    ValueRef::Text(v) => Some(v.to_vec()),
                    ValueRef::Blob(v) => Some(v.to_vec()),
                };
            }

            let cell_slices: Vec<Option<&[u8]>> = cell_buffers.iter().map(|c| c.as_deref()).collect();
            let (count, rollover) = pager.append_row(cell_slices)?;
            total_rows = count;

            if rollover || last_event_instant.elapsed().as_millis() >= 16 {
                let _ = event_tx.send(StreamEvent::PageAppended { total_rows });
                last_event_instant = Instant::now();
            }
        }

        let elapsed_ms = start_time.elapsed().as_millis() as u64;
        let _ = event_tx.send(StreamEvent::Completed {
            total_rows,
            elapsed_ms,
        });

        Ok(())
    }

    async fn cancel_active_query(&self) -> Result<()> {
        self.interrupt_handle.lock().interrupt();
        Ok(())
    }

    async fn list_tables(&self) -> Result<Vec<TableSummary>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT 'main', name, type FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE 'sqlite_%' ORDER BY name;"
        )?;

        let rows = stmt.query_map([], |row| {
            Ok(TableSummary {
                schema: row.get(0)?,
                name: row.get(1)?,
                table_type: row.get(2)?,
            })
        })?;

        let mut tables = Vec::new();
        for r in rows {
            tables.push(r?);
        }
        Ok(tables)
    }

    async fn list_columns(&self, _schema: &str, table: &str) -> Result<Vec<ColumnMeta>> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&format!("PRAGMA table_info(\"{table}\");"))?;

        let rows = stmt.query_map([], |row| {
            let name: String = row.get(1)?;
            let dtype_str: String = row.get(2)?;
            let notnull: i32 = row.get(3)?;
            let pk: i32 = row.get(5)?;

            let data_type = match dtype_str.to_uppercase().as_str() {
                "INTEGER" | "INT" => DataType::Int4,
                "BIGINT" => DataType::Int8,
                "REAL" | "FLOAT" | "DOUBLE" => DataType::Float8,
                "BLOB" => DataType::Bytea,
                _ => DataType::Text,
            };

            Ok(ColumnMeta::new(name, data_type)
                .with_nullable(notnull == 0)
                .with_pk(pk > 0))
        })?;

        let mut cols = Vec::new();
        for r in rows {
            cols.push(r?);
        }
        Ok(cols)
    }
}
