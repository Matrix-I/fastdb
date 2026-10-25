use std::sync::Arc;
use std::time::Instant;
use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use mem_engine::{ColumnMeta, DataType};
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Params, Pool, Value};
use parking_lot::Mutex;
use platform_hal::socket::{detect_local_unix_socket, DbKind};

use crate::stream::{report_error, RowSink};
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
        let mut conn = pool
            .get_conn()
            .await
            .map_err(|e| report_error(&event_tx, format!("MySQL connection error: {e}")))?;
        *active_conn_id.lock() = Some(conn.id());

        let result = async {
            let mut query_result = conn
                .query_iter(sql)
                .await
                .map_err(|e| report_error(&event_tx, format!("MySQL query execution error: {e}")))?;

            let columns: Vec<ColumnMeta> = query_result
                .columns()
                .as_ref()
                .map(|cols| {
                    cols.iter()
                        .map(|c| ColumnMeta::new(c.name_str().to_string(), mysql_column_data_type(c)))
                        .collect()
                })
                .unwrap_or_default();
            let mut sink = RowSink::start(columns, event_tx.clone(), start_time)?;

            let stream = query_result
                .stream::<mysql_async::Row>()
                .await
                .map_err(|e| sink.fail(format!("MySQL row stream error: {e}")))?;
            if let Some(mut stream) = stream {
                while let Some(row_res) = stream.next().await {
                    let row = row_res.map_err(|e| sink.fail(format!("MySQL row stream error: {e}")))?;
                    for (i, cell) in sink.cells_mut().iter_mut().enumerate() {
                        *cell = mysql_value_to_bytes(row.get(i));
                    }
                    sink.commit_row()?;
                }
            }
            Ok::<RowSink, anyhow::Error>(sink)
        }
        .await;
        *active_conn_id.lock() = None;

        result?.finish();
        Ok(())
    }
}

/// Maps a result column's wire type to the engine's DataType
fn mysql_column_data_type(column: &mysql_async::Column) -> DataType {
    use mysql_async::consts::{ColumnFlags, ColumnType};
    match column.column_type() {
        ColumnType::MYSQL_TYPE_TINY | ColumnType::MYSQL_TYPE_SHORT | ColumnType::MYSQL_TYPE_YEAR => DataType::Int2,
        ColumnType::MYSQL_TYPE_LONG | ColumnType::MYSQL_TYPE_INT24 => DataType::Int4,
        ColumnType::MYSQL_TYPE_LONGLONG => DataType::Int8,
        ColumnType::MYSQL_TYPE_FLOAT => DataType::Float4,
        ColumnType::MYSQL_TYPE_DOUBLE => DataType::Float8,
        ColumnType::MYSQL_TYPE_DECIMAL | ColumnType::MYSQL_TYPE_NEWDECIMAL => DataType::Numeric,
        ColumnType::MYSQL_TYPE_DATE | ColumnType::MYSQL_TYPE_NEWDATE => DataType::Date,
        ColumnType::MYSQL_TYPE_TIME | ColumnType::MYSQL_TYPE_TIME2 => DataType::Time,
        ColumnType::MYSQL_TYPE_DATETIME
        | ColumnType::MYSQL_TYPE_DATETIME2
        | ColumnType::MYSQL_TYPE_TIMESTAMP
        | ColumnType::MYSQL_TYPE_TIMESTAMP2 => DataType::Timestamp,
        ColumnType::MYSQL_TYPE_JSON => DataType::Json,
        ColumnType::MYSQL_TYPE_VARCHAR | ColumnType::MYSQL_TYPE_VAR_STRING => DataType::Varchar,
        ColumnType::MYSQL_TYPE_TINY_BLOB
        | ColumnType::MYSQL_TYPE_MEDIUM_BLOB
        | ColumnType::MYSQL_TYPE_LONG_BLOB
        | ColumnType::MYSQL_TYPE_BLOB
            if column.flags().contains(ColumnFlags::BINARY_FLAG) && column.character_set() == 63 =>
        {
            // Character set 63 is "binary": BLOB rather than TEXT
            DataType::Bytea
        }
        _ => DataType::Text,
    }
}

/// Maps an information_schema.columns data_type to the engine's DataType
fn info_schema_data_type(data_type: &str) -> DataType {
    match data_type.to_lowercase().as_str() {
        "tinyint" | "smallint" | "year" => DataType::Int2,
        "int" | "integer" | "mediumint" => DataType::Int4,
        "bigint" => DataType::Int8,
        "float" => DataType::Float4,
        "double" => DataType::Float8,
        "decimal" => DataType::Numeric,
        "json" => DataType::Json,
        "varchar" => DataType::Varchar,
        "date" => DataType::Date,
        "time" => DataType::Time,
        "datetime" | "timestamp" => DataType::Timestamp,
        "blob" | "tinyblob" | "mediumblob" | "longblob" | "binary" | "varbinary" => DataType::Bytea,
        _ => DataType::Text,
    }
}

/// Builds column metadata from an information_schema.columns row
fn info_schema_column(name: String, data_type: &str, is_nullable: &str, column_key: &str) -> ColumnMeta {
    ColumnMeta::new(name, info_schema_data_type(data_type))
        .with_nullable(is_nullable == "YES")
        .with_pk(column_key == "PRI")
        .with_raw_type(data_type)
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
            .map(|(name, dtype, is_null, col_key)| info_schema_column(name, &dtype, &is_null, &col_key))
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
            let col = info_schema_column(name, &dtype, &is_null, &col_key);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_info_schema_column_mapping() {
        let col = info_schema_column("id".to_string(), "bigint", "NO", "PRI");
        assert_eq!(col.data_type, DataType::Int8);
        assert!(col.is_primary_key);
        assert!(!col.nullable);
        assert_eq!(col.raw_type.as_deref(), Some("bigint"));

        assert_eq!(info_schema_data_type("DATETIME"), DataType::Timestamp);
        assert_eq!(info_schema_data_type("varbinary"), DataType::Bytea);
        assert_eq!(info_schema_data_type("enum"), DataType::Text);
    }

    #[test]
    fn test_value_to_bytes() {
        assert_eq!(mysql_value_to_bytes(None), None);
        assert_eq!(mysql_value_to_bytes(Some(Value::NULL)), None);
        assert_eq!(mysql_value_to_bytes(Some(Value::Int(-7))), Some(b"-7".to_vec()));
        assert_eq!(
            mysql_value_to_bytes(Some(Value::Date(2024, 2, 29, 13, 5, 9, 0))),
            Some(b"2024-02-29 13:05:09".to_vec())
        );
        assert_eq!(mysql_value_to_bytes(Some(Value::Time(true, 1, 2, 3, 4, 0))), Some(b"-1d 02:03:04".to_vec()));
    }
}
