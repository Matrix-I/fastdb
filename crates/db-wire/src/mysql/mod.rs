use std::sync::Arc;
use std::time::Instant;
use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::StreamExt;
use mem_engine::{ColumnMeta, DataType, QueryResultPager};
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Pool, Value};
use parking_lot::Mutex;
use platform_hal::socket::{detect_local_unix_socket, DbKind};

use crate::traits::{ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary};

pub struct MySqlDriver {
    pub config: ConnectionConfig,
    pool: Arc<Mutex<Option<Pool>>>,
}

impl MySqlDriver {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            config,
            pool: Arc::new(Mutex::new(None)),
        }
    }

    fn get_opts(&self) -> Opts {
        let mut builder = OptsBuilder::default();
        builder = builder.user(Some(&self.config.user));
        if let Some(ref pw) = self.config.password {
            builder = builder.pass(Some(pw));
        }
        builder = builder.db_name(Some(&self.config.database));

        let socket = if self.config.use_unix_socket {
            detect_local_unix_socket(DbKind::MySql, &self.config.host, self.config.port)
        } else {
            None
        };

        if let Some(sock_path) = socket {
            builder = builder.socket(Some(sock_path.to_string_lossy().to_string()));
        } else {
            builder = builder.ip_or_hostname(self.config.host.clone()).tcp_port(self.config.port);
        }

        builder.into()
    }

    async fn get_pool(&self) -> Result<Pool> {
        let mut guard = self.pool.lock();
        if let Some(ref p) = *guard {
            return Ok(p.clone());
        }

        let opts = self.get_opts();
        let pool = Pool::new(opts);
        *guard = Some(pool.clone());
        Ok(pool)
    }
}

#[async_trait]
impl DatabaseDriver for MySqlDriver {
    async fn ping(&self) -> Result<()> {
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        conn.query_drop("SELECT 1;").await?;
        Ok(())
    }

    async fn execute_streaming(
        &self,
        sql: &str,
        pager: Arc<QueryResultPager>,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;

        let mut query_result = match conn.query_iter(sql).await {
            Ok(qr) => qr,
            Err(e) => {
                let msg = format!("MySQL query execution error: {e}");
                let _ = event_tx.send(StreamEvent::Error(msg.clone()));
                bail!(msg);
            }
        };

        let columns: Vec<ColumnMeta> = query_result
            .columns()
            .as_ref()
            .map(|cols| {
                cols.iter()
                    .map(|c| ColumnMeta::new(c.name_str().to_string(), DataType::Text))
                    .collect()
            })
            .unwrap_or_default();

        let num_cols = columns.len();
        let _ = event_tx.send(StreamEvent::SchemaReady(columns));

        let mut total_rows = 0u64;
        let mut last_event_instant = Instant::now();
        let mut cell_buffers: Vec<Option<Vec<u8>>> = vec![None; num_cols];

        let stream_opt = query_result.stream::<mysql_async::Row>().await?;
        if let Some(mut stream) = stream_opt {
            while let Some(row_res) = stream.next().await {
                let row = match row_res {
                    Ok(r) => r,
                    Err(e) => {
                        let msg = format!("MySQL row stream error: {e}");
                        let _ = event_tx.send(StreamEvent::Error(msg.clone()));
                        bail!(msg);
                    }
                };

                for i in 0..num_cols {
                    let val: Option<Value> = row.get(i);
                    cell_buffers[i] = match val {
                        None | Some(Value::NULL) => None,
                        Some(Value::Bytes(b)) => Some(b),
                        Some(Value::Int(n)) => Some(n.to_string().into_bytes()),
                        Some(Value::UInt(u)) => Some(u.to_string().into_bytes()),
                        Some(Value::Float(f)) => Some(f.to_string().into_bytes()),
                        Some(Value::Double(d)) => Some(d.to_string().into_bytes()),
                        Some(Value::Date(y, m, d, h, min, s, _)) => {
                            Some(format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}").into_bytes())
                        }
                        Some(Value::Time(neg, d, h, min, s, _)) => {
                            let sign = if neg { "-" } else { "" };
                            Some(format!("{sign}{d}d {h:02}:{min:02}:{s:02}").into_bytes())
                        }
                    };
                }

                let cell_slices: Vec<Option<&[u8]>> = cell_buffers.iter().map(|c| c.as_deref()).collect();
                let (rows, rollover) = pager.append_row(cell_slices)?;
                total_rows = rows;

                if rollover || last_event_instant.elapsed().as_millis() >= 16 {
                    let _ = event_tx.send(StreamEvent::PageAppended { total_rows });
                    last_event_instant = Instant::now();
                }
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
        // In MySQL, canceling is done via KILL QUERY <conn_id>
        Ok(())
    }

    async fn list_tables(&self) -> Result<Vec<TableSummary>> {
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        let sql = r#"
            SELECT table_schema, table_name, table_type
            FROM information_schema.tables
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            ORDER BY table_schema, table_name;
        "#;

        let rows: Vec<(String, String, String)> = conn.query(sql).await?;
        let tables = rows
            .into_iter()
            .map(|(schema, name, table_type)| TableSummary {
                schema,
                name,
                table_type,
            })
            .collect();

        Ok(tables)
    }

    async fn list_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnMeta>> {
        let pool = self.get_pool().await?;
        let mut conn = pool.get_conn().await?;
        let sql = format!(
            "SELECT column_name, data_type, is_nullable, column_key FROM information_schema.columns WHERE table_schema = '{schema}' AND table_name = '{table}' ORDER BY ordinal_position;"
        );

        let rows: Vec<(String, String, String, String)> = conn.query(sql).await?;
        let cols = rows
            .into_iter()
            .map(|(name, dtype, is_null, col_key)| {
                let data_type = match dtype.to_lowercase().as_str() {
                    "tinyint" | "smallint" => DataType::Int2,
                    "int" | "integer" | "mediumint" => DataType::Int4,
                    "bigint" => DataType::Int8,
                    "float" => DataType::Float4,
                    "double" => DataType::Float8,
                    "decimal" => DataType::Numeric,
                    "json" => DataType::Json,
                    _ => DataType::Text,
                };
                ColumnMeta::new(name, data_type)
                    .with_nullable(is_null == "YES")
                    .with_pk(col_key == "PRI")
            })
            .collect();

        Ok(cols)
    }
}
