pub mod binary_decoder;

use std::sync::Arc;
use std::time::Instant;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use mem_engine::{ColumnMeta, QueryResultPager};
use parking_lot::Mutex;
use platform_hal::socket::{detect_local_unix_socket, DbKind};
use tokio_postgres::{CancelToken, Client, NoTls};
use tracing::{debug, info, warn};

use crate::traits::{ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary};
use binary_decoder::{decode_pg_cell, pg_type_to_data_type};

pub struct PostgresDriver {
    pub config: ConnectionConfig,
    client: tokio::sync::Mutex<Option<Arc<Client>>>,
    cancel_token: Arc<Mutex<Option<CancelToken>>>,
}

impl PostgresDriver {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            config,
            client: tokio::sync::Mutex::new(None),
            cancel_token: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_or_connect(&self) -> Result<Arc<Client>> {
        let mut guard = self.client.lock().await;
        if let Some(ref client) = *guard {
            if !client.is_closed() {
                return Ok(Arc::clone(client));
            }
        }

        // Connect on the shared runtime so the socket outlives the caller's runtime
        let client = Arc::new(crate::run_on_shared_runtime(Self::connect_raw(self.config.clone())).await?);
        *self.cancel_token.lock() = Some(client.cancel_token());
        *guard = Some(Arc::clone(&client));
        Ok(client)
    }

    async fn connect_raw(config: ConnectionConfig) -> Result<Client> {
        let mut pg_config = tokio_postgres::Config::new();
        pg_config.user(&config.user);
        if let Some(ref pw) = config.password {
            pg_config.password(pw);
        }
        pg_config.dbname(&config.database);
        pg_config.connect_timeout(config.connect_timeout);

        // Check for local Unix Domain Socket if enabled
        let unix_socket = if config.use_unix_socket {
            detect_local_unix_socket(DbKind::Postgres, &config.host, config.port)
        } else {
            None
        };

        if let Some(socket_path) = unix_socket {
            info!("Connecting to PostgreSQL via Unix Domain Socket: {}", socket_path.display());
            pg_config.host_path(socket_path);
        } else {
            info!("Connecting to PostgreSQL via TCP {}:{}", config.host, config.port);
            pg_config.host(&config.host);
            pg_config.port(config.port);
        }

        let (client, connection) = pg_config
            .connect(NoTls)
            .await
            .with_context(|| format!("Failed to connect to PostgreSQL ({}:{})", config.host, config.port))?;

        crate::get_runtime().spawn(async move {
            if let Err(e) = connection.await {
                warn!("PostgreSQL connection error: {}", e);
            }
        });

        Ok(client)
    }
}

#[async_trait]
impl DatabaseDriver for PostgresDriver {
    async fn ping(&self) -> Result<()> {
        let client = self.get_or_connect().await?;
        client.simple_query("SELECT 1;").await?;
        Ok(())
    }

    async fn execute_streaming(
        &self,
        sql: &str,
        pager: Arc<QueryResultPager>,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let client = self.get_or_connect().await?;

        // Update cancel token for this connection
        *self.cancel_token.lock() = Some(client.cancel_token());

        let statement = match client.prepare(sql).await {
            Ok(stmt) => stmt,
            Err(e) => {
                let err_msg = format!("SQL Preparation Error: {e}");
                let _ = event_tx.send(StreamEvent::Error(err_msg.clone()));
                bail!(err_msg);
            }
        };

        // Extract column metadata from statement
        let columns: Vec<ColumnMeta> = statement
            .columns()
            .iter()
            .map(|c| ColumnMeta::new(c.name(), pg_type_to_data_type(c.type_())))
            .collect();

        let num_cols = columns.len();
        let _ = event_tx.send(StreamEvent::SchemaReady(columns));

        let row_stream = match client.query_raw(&statement, std::iter::empty::<i32>()).await {
            Ok(s) => s,
            Err(e) => {
                let err_msg = format!("Query execution error: {e}");
                let _ = event_tx.send(StreamEvent::Error(err_msg.clone()));
                bail!(err_msg);
            }
        };

        futures::pin_mut!(row_stream);

        let mut total_rows = 0u64;
        let mut last_event_instant = Instant::now();
        let mut cell_buffers: Vec<Option<Vec<u8>>> = vec![None; num_cols];

        while let Some(row_res) = row_stream.next().await {
            let row = match row_res {
                Ok(r) => r,
                Err(e) => {
                    let err_msg = format!("Row stream error: {e}");
                    let _ = event_tx.send(StreamEvent::Error(err_msg.clone()));
                    bail!(err_msg);
                }
            };

            for col_idx in 0..num_cols {
                cell_buffers[col_idx] = decode_pg_cell(&row, col_idx);
            }

            let cell_slices: Vec<Option<&[u8]>> = cell_buffers.iter().map(|c| c.as_deref()).collect();
            let (rows, rollover) = pager.append_row(cell_slices)?;
            total_rows = rows;

            // Throttled notification: notify every 16ms (60 FPS) or on page rollover
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

        debug!("Postgres query streaming completed: {} rows in {}ms", total_rows, elapsed_ms);
        Ok(())
    }

    async fn execute_query(&self, sql: &str) -> Result<(Arc<QueryResultPager>, u64, u64)> {
        let start_time = Instant::now();
        let client = self.get_or_connect().await?;
        *self.cancel_token.lock() = Some(client.cancel_token());

        let clean_sql = sql.trim().trim_end_matches(';').trim();
        let statement = client.prepare(clean_sql).await
            .with_context(|| format!("SQL Preparation Error: {clean_sql}"))?;

        let columns: Vec<ColumnMeta> = statement
            .columns()
            .iter()
            .map(|c| ColumnMeta::new(c.name(), pg_type_to_data_type(c.type_())))
            .collect();

        let num_cols = columns.len();
        let pager = Arc::new(QueryResultPager::new(columns)?);

        let rows = client.query(&statement, &[]).await
            .with_context(|| format!("Query execution error: {clean_sql}"))?;

        let mut total_rows = 0u64;
        let mut cell_buffers: Vec<Option<Vec<u8>>> = vec![None; num_cols];

        for row in &rows {
            for col_idx in 0..num_cols {
                cell_buffers[col_idx] = decode_pg_cell(row, col_idx);
            }
            let cell_slices: Vec<Option<&[u8]>> = cell_buffers.iter().map(|c| c.as_deref()).collect();
            let (r, _) = pager.append_row(cell_slices)?;
            total_rows = r;
        }

        let elapsed_ms = start_time.elapsed().as_millis() as u64;
        info!("Postgres execute_query completed: {} rows in {}ms", total_rows, elapsed_ms);
        Ok((pager, total_rows, elapsed_ms))
    }

    async fn cancel_active_query(&self) -> Result<()> {
        let maybe_token = self.cancel_token.lock().clone();
        if let Some(token) = maybe_token {
            token.cancel_query(NoTls).await?;
            info!("Cancelled active PostgreSQL query");
        }
        Ok(())
    }

    async fn list_tables(&self) -> Result<Vec<TableSummary>> {
        let client = self.get_or_connect().await?;
        let sql = r#"
            SELECT t.table_schema, t.table_name, t.table_type,
                   CASE WHEN c.reltuples >= 0 THEN c.reltuples::bigint ELSE NULL END as est_rows
            FROM information_schema.tables t
            LEFT JOIN pg_namespace n ON n.nspname = t.table_schema
            LEFT JOIN pg_class c ON c.relname = t.table_name AND c.relnamespace = n.oid
            WHERE t.table_schema NOT IN ('information_schema', 'pg_catalog')
            ORDER BY t.table_schema, t.table_name;
        "#;

        let rows = client.query(sql, &[]).await?;
        let tables = rows
            .into_iter()
            .map(|r| {
                let est_rows: Option<i64> = r.get(3);
                TableSummary {
                    schema: r.get(0),
                    name: r.get(1),
                    table_type: r.get(2),
                    estimated_rows: est_rows.and_then(|x| if x >= 0 { Some(x as u64) } else { None }),
                }
            })
            .collect();

        Ok(tables)
    }

    async fn list_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnMeta>> {
        let client = self.get_or_connect().await?;
        let sql = r#"
            SELECT column_name, data_type, is_nullable, column_default, collation_name
            FROM information_schema.columns
            WHERE table_schema = $1 AND table_name = $2
            ORDER BY ordinal_position;
        "#;

        let rows = client.query(sql, &[&schema, &table]).await?;
        let cols = rows
            .into_iter()
            .map(|r| {
                let name: String = r.get(0);
                let dtype_str: String = r.get(1);
                let is_null_str: String = r.get(2);
                let default_val: Option<String> = r.get(3);
                let collation: Option<String> = r.get(4);
                let data_type = match dtype_str.to_lowercase().as_str() {
                    "integer" | "int" => mem_engine::DataType::Int4,
                    "bigint" => mem_engine::DataType::Int8,
                    "smallint" => mem_engine::DataType::Int2,
                    "boolean" => mem_engine::DataType::Bool,
                    "numeric" | "decimal" => mem_engine::DataType::Numeric,
                    "real" => mem_engine::DataType::Float4,
                    "double precision" => mem_engine::DataType::Float8,
                    "json" | "jsonb" => mem_engine::DataType::Json,
                    "uuid" => mem_engine::DataType::Uuid,
                    "character varying" | "varchar" => mem_engine::DataType::Varchar,
                    _ => mem_engine::DataType::Text,
                };
                let raw_type = if dtype_str.to_lowercase() == "character varying" {
                    "varchar".to_string()
                } else {
                    dtype_str
                };
                let mut col = ColumnMeta::new(name, data_type)
                    .with_nullable(is_null_str == "YES")
                    .with_raw_type(raw_type);
                if let Some(def) = default_val {
                    col = col.with_default(def);
                }
                if let Some(coll) = collation {
                    col = col.with_collation(coll);
                }
                col
            })
            .collect();

        Ok(cols)
    }

    async fn list_all_columns(&self) -> Result<Vec<(String, String, Vec<ColumnMeta>)>> {
        let client = self.get_or_connect().await?;
        let sql = r#"
            SELECT c.table_schema, c.table_name, c.column_name, c.data_type, c.is_nullable,
                   COALESCE((
                       SELECT 1 FROM information_schema.table_constraints tc
                       JOIN information_schema.key_column_usage kcu
                         ON tc.constraint_name = kcu.constraint_name
                        AND tc.table_schema = kcu.table_schema
                       WHERE tc.constraint_type = 'PRIMARY KEY'
                         AND tc.table_schema = c.table_schema
                         AND tc.table_name = c.table_name
                         AND kcu.column_name = c.column_name
                       LIMIT 1
                   ), 0) as is_pk,
                   c.column_default,
                   c.collation_name
            FROM information_schema.columns c
            WHERE c.table_schema NOT IN ('information_schema', 'pg_catalog')
            ORDER BY c.table_schema, c.table_name, c.ordinal_position;
        "#;

        let rows = client.query(sql, &[]).await?;
        let mut map: std::collections::BTreeMap<(String, String), Vec<ColumnMeta>> = std::collections::BTreeMap::new();
        for r in rows {
            let schema: String = r.get(0);
            let table: String = r.get(1);
            let name: String = r.get(2);
            let dtype_str: String = r.get(3);
            let is_null_str: String = r.get(4);
            let is_pk: i32 = r.get(5);
            let default_val: Option<String> = r.get(6);
            let collation: Option<String> = r.get(7);
            let data_type = match dtype_str.to_lowercase().as_str() {
                "integer" | "int" => mem_engine::DataType::Int4,
                "bigint" => mem_engine::DataType::Int8,
                "smallint" => mem_engine::DataType::Int2,
                "boolean" => mem_engine::DataType::Bool,
                "numeric" | "decimal" => mem_engine::DataType::Numeric,
                "real" => mem_engine::DataType::Float4,
                "double precision" => mem_engine::DataType::Float8,
                "json" | "jsonb" => mem_engine::DataType::Json,
                "uuid" => mem_engine::DataType::Uuid,
                "character varying" | "varchar" => mem_engine::DataType::Varchar,
                _ => mem_engine::DataType::Text,
            };
            let raw_type = if dtype_str.to_lowercase() == "character varying" {
                "varchar".to_string()
            } else {
                dtype_str
            };
            let mut col = ColumnMeta::new(name, data_type)
                .with_nullable(is_null_str == "YES")
                .with_pk(is_pk > 0)
                .with_raw_type(raw_type);
            if let Some(def) = default_val {
                col = col.with_default(def);
            }
            if let Some(coll) = collation {
                col = col.with_collation(coll);
            }
            map.entry((schema, table)).or_default().push(col);
        }

        Ok(map.into_iter().map(|((s, t), cols)| (s, t, cols)).collect())
    }

    async fn list_all_constraints(&self) -> Result<Vec<(String, String, String, String)>> {
        let client = self.get_or_connect().await?;
        let sql = r#"
            SELECT table_schema, table_name, constraint_name, constraint_type
            FROM information_schema.table_constraints
            WHERE table_schema NOT IN ('information_schema', 'pg_catalog')
            ORDER BY table_schema, table_name, constraint_name;
        "#;
        let rows = client.query(sql, &[]).await?;
        Ok(rows.into_iter().map(|r| (r.get(0), r.get(1), r.get(2), r.get(3))).collect())
    }

    async fn list_all_indexes(&self) -> Result<Vec<(String, String, String)>> {
        let client = self.get_or_connect().await?;
        let sql = r#"
            SELECT schemaname, tablename, indexname
            FROM pg_indexes
            WHERE schemaname NOT IN ('information_schema', 'pg_catalog')
            ORDER BY schemaname, tablename, indexname;
        "#;
        let rows = client.query(sql, &[]).await?;
        Ok(rows.into_iter().map(|r| (r.get(0), r.get(1), r.get(2))).collect())
    }
}
