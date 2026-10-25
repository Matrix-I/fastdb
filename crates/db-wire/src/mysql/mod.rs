use std::sync::Arc;
use std::time::Instant;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use mem_engine::{ColumnMeta, DataType, QueryResultPager};
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Params, Pool, Value};
use parking_lot::Mutex;
use platform_hal::socket::{detect_local_unix_socket, DbKind};

use crate::traits::{ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary};

pub struct MySqlDriver {
    pub config: ConnectionConfig,
    pool: Arc<Mutex<Option<Pool>>>,
    /// Server connection id of the connection running the current query, for KILL QUERY
    active_conn_id: Arc<Mutex<Option<u32>>>,
}

impl MySqlDriver {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            config,
            pool: Arc::new(Mutex::new(None)),
            active_conn_id: Arc::new(Mutex::new(None)),
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

    fn get_pool(&self) -> Result<Pool> {
        let mut guard = self.pool.lock();
        if let Some(ref p) = *guard {
            return Ok(p.clone());
        }

        let opts = self.get_opts();
        let pool = Pool::new(opts);
        *guard = Some(pool.clone());
        Ok(pool)
    }

    /// Runs a metadata query with bound parameters on the shared runtime
    async fn query_rows<T, P>(&self, sql: String, params: P) -> Result<Vec<T>>
    where
        T: FromRow + Send + 'static,
        P: Into<Params> + Send + 'static,
    {
        let pool = self.get_pool()?;
        crate::run_on_shared_runtime(async move {
            let mut conn = pool.get_conn().await?;
            Ok(conn.exec(sql, params).await?)
        })
        .await
    }

    /// Streams a query's rows into a new pager; runs on the shared runtime via the caller
    async fn stream_query(
        pool: Pool,
        sql: String,
        event_tx: flume::Sender<StreamEvent>,
        active_conn_id: Arc<Mutex<Option<u32>>>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let fail = |msg: String| {
            let _ = event_tx.send(StreamEvent::Error(msg.clone()));
            anyhow::anyhow!(msg)
        };

        let mut conn = pool
            .get_conn()
            .await
            .map_err(|e| fail(format!("MySQL connection error: {e}")))?;
        *active_conn_id.lock() = Some(conn.id());

        let result = async {
            let mut query_result = conn
                .query_iter(sql)
                .await
                .map_err(|e| fail(format!("MySQL query execution error: {e}")))?;

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
            let pager = Arc::new(QueryResultPager::new(columns).map_err(|e| fail(format!("{e:#}")))?);
            let _ = event_tx.send(StreamEvent::SchemaReady(Arc::clone(&pager)));

            let mut total_rows = 0u64;
            let mut last_event_instant = Instant::now();
            let mut cell_buffers: Vec<Option<Vec<u8>>> = vec![None; num_cols];

            let stream = query_result
                .stream::<mysql_async::Row>()
                .await
                .map_err(|e| fail(format!("MySQL row stream error: {e}")))?;
            if let Some(mut stream) = stream {
                while let Some(row_res) = stream.next().await {
                    let row = row_res.map_err(|e| fail(format!("MySQL row stream error: {e}")))?;

                    for (i, buf) in cell_buffers.iter_mut().enumerate() {
                        *buf = mysql_value_to_bytes(row.get(i));
                    }

                    let cell_slices: Vec<Option<&[u8]>> = cell_buffers.iter().map(|c| c.as_deref()).collect();
                    let (rows, rollover) = pager.append_row(cell_slices).map_err(|e| fail(format!("{e:#}")))?;
                    total_rows = rows;

                    if rollover || last_event_instant.elapsed().as_millis() >= 16 {
                        let _ = event_tx.send(StreamEvent::PageAppended { total_rows });
                        last_event_instant = Instant::now();
                    }
                }
            }
            Ok::<u64, anyhow::Error>(total_rows)
        }
        .await;
        *active_conn_id.lock() = None;

        let total_rows = result?;
        let elapsed_ms = start_time.elapsed().as_millis() as u64;
        let _ = event_tx.send(StreamEvent::Completed {
            total_rows,
            elapsed_ms,
        });
        Ok(())
    }
}

fn mysql_value_to_bytes(val: Option<Value>) -> Option<Vec<u8>> {
    match val {
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
    }
}

#[async_trait]
impl DatabaseDriver for MySqlDriver {
    async fn ping(&self) -> Result<()> {
        let pool = self.get_pool()?;
        crate::run_on_shared_runtime(async move {
            let mut conn = pool.get_conn().await?;
            conn.query_drop("SELECT 1;").await?;
            Ok(())
        })
        .await
    }

    async fn execute_streaming(&self, sql: &str, event_tx: flume::Sender<StreamEvent>) -> Result<()> {
        let pool = self.get_pool()?;
        let sql = sql.to_string();
        let active_conn_id = Arc::clone(&self.active_conn_id);
        crate::run_on_shared_runtime(Self::stream_query(pool, sql, event_tx, active_conn_id)).await
    }

    async fn cancel_active_query(&self) -> Result<()> {
        let Some(conn_id) = *self.active_conn_id.lock() else {
            return Ok(());
        };
        let pool = self.get_pool()?;
        crate::run_on_shared_runtime(async move {
            // KILL must run on a different connection than the one executing the query
            let mut conn = pool.get_conn().await?;
            conn.query_drop(format!("KILL QUERY {conn_id}")).await?;
            Ok(())
        })
        .await
    }

    async fn list_tables(&self) -> Result<Vec<TableSummary>> {
        let sql = r#"
            SELECT table_schema, table_name, table_type, table_rows
            FROM information_schema.tables
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            ORDER BY table_schema, table_name;
        "#;

        let rows: Vec<(String, String, String, Option<u64>)> = self.query_rows(sql.to_string(), ()).await?;
        let tables = rows
            .into_iter()
            .map(|(schema, name, table_type, estimated_rows)| TableSummary {
                schema,
                name,
                table_type,
                estimated_rows,
            })
            .collect();

        Ok(tables)
    }

    async fn list_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnMeta>> {
        let sql = "SELECT column_name, data_type, is_nullable, column_key FROM information_schema.columns WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position;";

        let rows: Vec<(String, String, String, String)> =
            self.query_rows(sql.to_string(), (schema.to_string(), table.to_string())).await?;
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

    async fn list_all_columns(&self) -> Result<Vec<(String, String, Vec<ColumnMeta>)>> {
        let sql = r#"
            SELECT table_schema, table_name, column_name, data_type, is_nullable, column_key
            FROM information_schema.columns
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            ORDER BY table_schema, table_name, ordinal_position;
        "#;

        let rows: Vec<(String, String, String, String, String, String)> = self.query_rows(sql.to_string(), ()).await?;
        let mut map: std::collections::BTreeMap<(String, String), Vec<ColumnMeta>> = std::collections::BTreeMap::new();
        for (schema, table, name, dtype, is_null, col_key) in rows {
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
            let col = ColumnMeta::new(name, data_type)
                .with_nullable(is_null == "YES")
                .with_pk(col_key == "PRI");
            map.entry((schema, table)).or_default().push(col);
        }

        Ok(map.into_iter().map(|((s, t), cols)| (s, t, cols)).collect())
    }

    async fn list_all_constraints(&self) -> Result<Vec<(String, String, String, String)>> {
        let sql = r#"
            SELECT table_schema, table_name, constraint_name, constraint_type
            FROM information_schema.table_constraints
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            ORDER BY table_schema, table_name, constraint_name;
        "#;
        let rows: Vec<(String, String, String, String)> = self.query_rows(sql.to_string(), ()).await?;
        Ok(rows)
    }

    async fn list_all_indexes(&self) -> Result<Vec<(String, String, String)>> {
        let sql = r#"
            SELECT DISTINCT table_schema, table_name, index_name
            FROM information_schema.statistics
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            ORDER BY table_schema, table_name, index_name;
        "#;
        let rows: Vec<(String, String, String)> = self.query_rows(sql.to_string(), ()).await?;
        Ok(rows)
    }
}

