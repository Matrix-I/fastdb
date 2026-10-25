pub mod binary_decoder;

use std::sync::Arc;
use std::time::Instant;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use mem_engine::ColumnMeta;
use parking_lot::Mutex;
use platform_hal::socket::{detect_local_unix_socket, DbKind};
use tokio_postgres::{CancelToken, Client, NoTls};
use tracing::{debug, info, warn};

use crate::stream::{report_error, RowSink};
use crate::traits::{row_count_error, ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary};
use binary_decoder::{decode_pg_cell, pg_type_to_data_type};

pub struct PostgresDriver {
    pub config: ConnectionConfig,
    client: tokio::sync::Mutex<Option<Arc<Client>>>,
    cancel_token: Arc<Mutex<Option<CancelToken>>>,
}

/// Server errors carry the useful text in their DbError; the Display impl only says "db error"
fn pg_error_message(e: &tokio_postgres::Error) -> String {
    match e.as_db_error() {
        Some(db) => format!("{}: {}", db.severity(), db.message()),
        None => e.to_string(),
    }
}

/// Maps an information_schema.columns data_type to the engine's DataType
fn info_schema_data_type(data_type: &str) -> mem_engine::DataType {
    use mem_engine::DataType;
    match data_type.to_lowercase().as_str() {
        "integer" | "int" => DataType::Int4,
        "bigint" => DataType::Int8,
        "smallint" => DataType::Int2,
        "boolean" => DataType::Bool,
        "numeric" | "decimal" => DataType::Numeric,
        "real" => DataType::Float4,
        "double precision" => DataType::Float8,
        "json" | "jsonb" => DataType::Json,
        "uuid" => DataType::Uuid,
        "character varying" | "varchar" => DataType::Varchar,
        "date" => DataType::Date,
        "time without time zone" => DataType::Time,
        "timestamp without time zone" => DataType::Timestamp,
        "timestamp with time zone" => DataType::TimestampTz,
        "bytea" => DataType::Bytea,
        _ => DataType::Text,
    }
}

/// Builds column metadata from an information_schema.columns row
fn info_schema_column(
    name: String,
    data_type: &str,
    is_nullable: &str,
    default_val: Option<String>,
    collation: Option<String>,
) -> ColumnMeta {
    let raw_type = if data_type.eq_ignore_ascii_case("character varying") {
        "varchar"
    } else {
        data_type
    };
    let mut col = ColumnMeta::new(name, info_schema_data_type(data_type))
        .with_nullable(is_nullable == "YES")
        .with_raw_type(raw_type);
    if let Some(def) = default_val {
        col = col.with_default(def);
    }
    if let Some(coll) = collation {
        col = col.with_collation(coll);
    }
    col
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

    async fn execute_streaming(&self, sql: &str, event_tx: flume::Sender<StreamEvent>) -> Result<()> {
        let start_time = Instant::now();
        let client = self
            .get_or_connect()
            .await
            .map_err(|e| report_error(&event_tx, format!("{e:#}")))?;

        // Update cancel token for this connection
        *self.cancel_token.lock() = Some(client.cancel_token());

        let statement = client
            .prepare(sql)
            .await
            .map_err(|e| report_error(&event_tx, format!("SQL Preparation Error: {}", pg_error_message(&e))))?;

        let columns: Vec<ColumnMeta> = statement
            .columns()
            .iter()
            .map(|c| ColumnMeta::new(c.name(), pg_type_to_data_type(c.type_())))
            .collect();
        let mut sink = RowSink::start(columns, event_tx, start_time)?;

        let row_stream = client
            .query_raw(&statement, std::iter::empty::<i32>())
            .await
            .map_err(|e| sink.fail(format!("Query execution error: {}", pg_error_message(&e))))?;
        futures::pin_mut!(row_stream);

        while let Some(row_res) = row_stream.next().await {
            let row = row_res.map_err(|e| sink.fail(format!("Row stream error: {}", pg_error_message(&e))))?;
            for (col_idx, cell) in sink.cells_mut().iter_mut().enumerate() {
                *cell = decode_pg_cell(&row, col_idx);
            }
            sink.commit_row()?;
        }

        let total_rows = sink.finish();
        debug!("Postgres query streaming completed: {} rows in {}ms", total_rows, start_time.elapsed().as_millis());
        Ok(())
    }

    async fn apply_row_changes(&self, statements: &[String]) -> Result<u64> {
        let client = self.get_or_connect().await?;
        client.batch_execute("BEGIN").await.map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
        for sql in statements {
            let outcome = match client.execute(sql.as_str(), &[]).await {
                Ok(1) => Ok(()),
                Ok(n) => Err(row_count_error(sql, n)),
                Err(e) => Err(anyhow::anyhow!(pg_error_message(&e))),
            };
            if let Err(e) = outcome {
                if let Err(rb) = client.batch_execute("ROLLBACK").await {
                    warn!("PostgreSQL rollback failed: {}", rb);
                }
                return Err(e);
            }
        }
        client.batch_execute("COMMIT").await.map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
        Ok(statements.len() as u64)
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
                info_schema_column(name, &dtype_str, &is_null_str, default_val, collation)
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
            let col = info_schema_column(name, &dtype_str, &is_null_str, default_val, collation).with_pk(is_pk > 0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use mem_engine::DataType;

    #[test]
    fn test_info_schema_column_mapping() {
        let col = info_schema_column(
            "email".to_string(),
            "character varying",
            "NO",
            Some("''::character varying".to_string()),
            Some("C".to_string()),
        );
        assert_eq!(col.data_type, DataType::Varchar);
        assert_eq!(col.raw_type.as_deref(), Some("varchar"));
        assert!(!col.nullable);
        assert_eq!(col.default_val.as_deref(), Some("''::character varying"));
        assert_eq!(col.collation.as_deref(), Some("C"));

        assert_eq!(info_schema_data_type("timestamp with time zone"), DataType::TimestampTz);
        assert_eq!(info_schema_data_type("BIGINT"), DataType::Int8);
        assert_eq!(info_schema_data_type("tsvector"), DataType::Text);
    }
}
