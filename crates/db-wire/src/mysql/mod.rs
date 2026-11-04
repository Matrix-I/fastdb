use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use mem_engine::{ColumnMeta, DataType};
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Params, Pool, SslOpts, TxOpts, Value};
use parking_lot::Mutex;
use platform_hal::socket::{DbKind, detect_local_unix_socket};
use std::sync::Arc;
use std::time::Instant;

mod script;

use crate::stream::{RowSink, report_error};
use crate::traits::{ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary, row_count_error};

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

        let socket = if self.config.use_unix_socket && !self.config.tls_enabled {
            detect_local_unix_socket(DbKind::MySql, &self.config.host, self.config.port)
        } else {
            None
        };

        if let Some(sock_path) = socket {
            builder = builder.socket(Some(sock_path.to_string_lossy().to_string()));
        } else {
            builder = builder
                .ip_or_hostname(self.config.host.clone())
                .tcp_port(self.config.port);
        }

        // Disable mysql_async's implicit TCP-to-socket upgrade when TLS is requested.
        builder = builder.prefer_socket(self.config.use_unix_socket && !self.config.tls_enabled);
        if self.config.tls_enabled {
            let mut ssl = SslOpts::default();
            if let Some(path) = &self.config.tls_ca_cert {
                ssl = ssl.with_root_certs(vec![std::path::PathBuf::from(path).into()]);
            }
            builder = builder.ssl_opts(ssl);
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
        control: Option<Arc<crate::execution::ExecutionControl>>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let mut conn = pool
            .get_conn()
            .await
            .map_err(|e| report_error(&event_tx, format!("MySQL connection error: {e}")))?;
        *active_conn_id.lock() = Some(conn.id());

        let result =
            Self::stream_connection(&mut conn, sql, event_tx, control.as_deref(), start_time).await;
        *active_conn_id.lock() = None;

        result
    }

    async fn stream_connection(
        conn: &mut mysql_async::Conn,
        sql: String,
        event_tx: flume::Sender<StreamEvent>,
        control: Option<&crate::execution::ExecutionControl>,
        start_time: Instant,
    ) -> Result<()> {
        if let Some(control) = &control {
            control.check()?;
        }
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
        let has_columns = !columns.is_empty();
        if !has_columns {
            let _ = event_tx.send(StreamEvent::CommandComplete {
                affected_rows: Some(query_result.affected_rows()),
            });
        }
        let mut sink = RowSink::start(columns, event_tx.clone(), start_time)?;

        {
            let stream = query_result
                .stream::<mysql_async::Row>()
                .await
                .map_err(|e| sink.fail(format!("MySQL row stream error: {e}")))?;
            if let Some(mut stream) = stream {
                while let Some(row_res) = stream.next().await {
                    let row =
                        row_res.map_err(|e| sink.fail(format!("MySQL row stream error: {e}")))?;
                    for (i, cell) in sink.cells_mut().iter_mut().enumerate() {
                        *cell = mysql_value_to_bytes(row.get(i));
                    }
                    sink.commit_row()?;
                }
            }
        }
        let warnings = query_result.warnings();
        let info = query_result.info().into_owned();
        drop(query_result);
        if !info.is_empty() {
            let _ = event_tx.send(StreamEvent::Notice(info));
        }
        if warnings > 0 {
            let _ = event_tx.send(StreamEvent::Notice(format!(
                "MySQL: {warnings} server warning(s); showing up to 20"
            )));
            let messages = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                conn.query::<(String, u16, String), _>("SHOW WARNINGS LIMIT 20"),
            )
            .await;
            match messages {
                Ok(Ok(messages)) => {
                    for (level, code, text) in messages {
                        let _ =
                            event_tx.send(StreamEvent::Notice(format!("{level} {code}: {text}")));
                    }
                }
                Ok(Err(error)) => {
                    let _ = event_tx.send(StreamEvent::Notice(format!(
                        "Cannot load warning details: {error}"
                    )));
                }
                Err(_) => {
                    let _ = event_tx.send(StreamEvent::Notice(
                        "Loading warning details timed out".into(),
                    ));
                }
            }
        }
        sink.finish();
        Ok(())
    }
}

/// Maps a result column's wire type to the engine's DataType
fn mysql_column_data_type(column: &mysql_async::Column) -> DataType {
    use mysql_async::consts::{ColumnFlags, ColumnType};
    match column.column_type() {
        ColumnType::MYSQL_TYPE_TINY
        | ColumnType::MYSQL_TYPE_SHORT
        | ColumnType::MYSQL_TYPE_YEAR => DataType::Int2,
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
            if column.flags().contains(ColumnFlags::BINARY_FLAG)
                && column.character_set() == 63 =>
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
fn info_schema_column(
    name: String,
    data_type: &str,
    is_nullable: &str,
    column_key: &str,
) -> ColumnMeta {
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
    fn supports_transfer(&self) -> bool {
        true
    }

    fn with_database(&self, database: &str) -> Result<Arc<dyn DatabaseDriver>> {
        crate::session_context::validate_name(database).map_err(anyhow::Error::msg)?;
        let mut config = self.config.clone();
        config.database = database.into();
        Ok(Arc::new(Self::new(config)))
    }

    fn supports_script_session(&self) -> bool {
        true
    }

    async fn open_script_session(&self) -> Result<Arc<dyn crate::script::ScriptSession>> {
        let opts = self.get_opts();
        let timeout = self.config.connect_timeout;
        crate::run_on_shared_runtime(async move {
            let conn = tokio::time::timeout(timeout, mysql_async::Conn::new(opts))
                .await
                .map_err(|_| anyhow::anyhow!("MySQL script connection timed out"))??;
            Ok(Arc::new(script::MySqlScriptSession::new(conn))
                as Arc<dyn crate::script::ScriptSession>)
        })
        .await
    }

    fn metadata_capabilities(&self) -> crate::metadata::MetadataCapabilities {
        use crate::catalog::CatalogSection as C;
        use crate::metadata::{MetadataCapabilities, MetadataFeature as F};
        MetadataCapabilities::new(
            &[F::Columns, F::Constraints, F::Indexes, F::ForeignKeys],
            &[F::Catalog(C::Rules), F::Catalog(C::Policies)],
        )
    }

    async fn ping(&self) -> Result<()> {
        let pool = self.get_pool()?;
        crate::run_on_shared_runtime(async move {
            let mut conn = pool.get_conn().await?;
            conn.query_drop("SELECT 1;").await?;
            Ok(())
        })
        .await
    }

    async fn execute_schema_ddl(&self, sql: &str) -> Result<()> {
        let pool = self.get_pool()?;
        let sql = sql.to_string();
        crate::run_on_shared_runtime(async move {
            let mut conn = pool.get_conn().await?;
            conn.query_drop(&sql).await?;
            Ok(())
        })
        .await
    }

    async fn execute_streaming(
        &self,
        sql: &str,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()> {
        let pool = self.get_pool()?;
        let sql = sql.to_string();
        let active_conn_id = Arc::clone(&self.active_conn_id);
        crate::run_on_shared_runtime(Self::stream_query(
            pool,
            sql,
            event_tx,
            active_conn_id,
            None,
        ))
        .await
    }

    async fn execute_streaming_controlled(
        &self,
        sql: &str,
        tx: flume::Sender<StreamEvent>,
        control: Arc<crate::execution::ExecutionControl>,
    ) -> Result<()> {
        control.check()?;
        let pool = self.get_pool()?;
        crate::run_on_shared_runtime(Self::stream_query(
            pool,
            sql.to_owned(),
            tx,
            self.active_conn_id.clone(),
            Some(control),
        ))
        .await
    }

    async fn apply_row_changes(&self, statements: &[String]) -> Result<u64> {
        let pool = self.get_pool()?;
        let statements = statements.to_vec();
        crate::run_on_shared_runtime(async move {
            let mut conn = pool.get_conn().await?;
            let mut tx = conn.start_transaction(TxOpts::default()).await?;
            for sql in &statements {
                let outcome = match tx.query_drop(sql.as_str()).await {
                    Ok(()) if tx.affected_rows() == 1 => Ok(()),
                    Ok(()) => Err(row_count_error(sql, tx.affected_rows())),
                    Err(e) => Err(anyhow::anyhow!("MySQL error: {e}")),
                };
                if let Err(e) = outcome {
                    tx.rollback().await?;
                    return Err(e);
                }
            }
            tx.commit().await?;
            Ok(statements.len() as u64)
        })
        .await
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

        let rows: Vec<(String, String, String, Option<u64>)> =
            self.query_rows(sql.to_string(), ()).await?;
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

        let rows: Vec<(String, String, String, String)> = self
            .query_rows(sql.to_string(), (schema.to_string(), table.to_string()))
            .await?;
        let cols = rows
            .into_iter()
            .map(|(name, dtype, is_null, col_key)| {
                info_schema_column(name, &dtype, &is_null, &col_key)
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

        let rows: Vec<(String, String, String, String, String, String)> =
            self.query_rows(sql.to_string(), ()).await?;
        let mut map: std::collections::BTreeMap<(String, String), Vec<ColumnMeta>> =
            std::collections::BTreeMap::new();
        for (schema, table, name, dtype, is_null, col_key) in rows {
            let col = info_schema_column(name, &dtype, &is_null, &col_key);
            map.entry((schema, table)).or_default().push(col);
        }

        Ok(map.into_iter().map(|((s, t), cols)| (s, t, cols)).collect())
    }

    async fn list_foreign_keys(&self) -> Result<Option<Vec<crate::ForeignKey>>> {
        let rows: Vec<(String, String, String, String, String, String, String)> = self
            .query_rows(
                "SELECT constraint_name, table_schema, table_name, column_name,
                        referenced_table_schema, referenced_table_name, referenced_column_name
                 FROM information_schema.key_column_usage
                 WHERE referenced_table_name IS NOT NULL
                 ORDER BY table_schema, table_name, constraint_name, ordinal_position"
                    .to_string(),
                (),
            )
            .await?;
        Ok(Some(
            rows.into_iter()
                .map(|r| crate::ForeignKey {
                    name: r.0,
                    source_schema: r.1,
                    source_table: r.2,
                    source_column: r.3,
                    target_schema: r.4,
                    target_table: r.5,
                    target_column: r.6,
                })
                .collect(),
        ))
    }

    async fn list_all_constraints(&self) -> Result<Vec<(String, String, String, String)>> {
        let sql = r#"
            SELECT table_schema, table_name, constraint_name, constraint_type
            FROM information_schema.table_constraints
            WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
            ORDER BY table_schema, table_name, constraint_name;
        "#;
        let rows: Vec<(String, String, String, String)> =
            self.query_rows(sql.to_string(), ()).await?;
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
        assert_eq!(
            mysql_value_to_bytes(Some(Value::Int(-7))),
            Some(b"-7".to_vec())
        );
        assert_eq!(
            mysql_value_to_bytes(Some(Value::Date(2024, 2, 29, 13, 5, 9, 0))),
            Some(b"2024-02-29 13:05:09".to_vec())
        );
        assert_eq!(
            mysql_value_to_bytes(Some(Value::Time(true, 1, 2, 3, 4, 0))),
            Some(b"-1d 02:03:04".to_vec())
        );
    }
}

#[cfg(test)]
mod tls_tests {
    use super::*;

    #[test]
    fn tls_requires_encryption_and_identity_verification_over_tcp() {
        let opts = MySqlDriver::new(ConnectionConfig {
            tls_enabled: true,
            use_unix_socket: true,
            tls_ca_cert: Some("/private-ca.pem".into()),
            ..ConnectionConfig::default()
        })
        .get_opts();
        assert!(!opts.prefer_socket());
        assert!(opts.socket().is_none());
        let ssl = opts.ssl_opts().unwrap();
        assert!(!ssl.skip_domain_validation());
        assert!(!ssl.accept_invalid_certs());
        assert_eq!(ssl.root_certs().len(), 1);
    }

    #[test]
    fn legacy_connection_does_not_enable_tls() {
        let opts = MySqlDriver::new(ConnectionConfig {
            use_unix_socket: false,
            ..ConnectionConfig::default()
        })
        .get_opts();
        assert!(opts.ssl_opts().is_none());
        assert!(!opts.prefer_socket());
    }
}

#[cfg(test)]
mod filter_mode_tests {
    use super::{ConnectionConfig, MySqlDriver};
    use crate::filter::{Criterion, FilterOp, FilterSpec};
    use mysql_async::prelude::Queryable;
    #[test]
    fn criteria_values_roundtrip_with_and_without_backslash_escapes() {
        let Ok(port) = std::env::var("FASTDB_TEST_MYSQL_PORT") else {
            assert!(
                std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
                "FASTDB_TEST_MYSQL_PORT required"
            );
            return;
        };
        crate::get_runtime().block_on(async {
            let driver = MySqlDriver::new(ConnectionConfig {
                driver: crate::DbDriverKind::MySql,
                host: std::env::var("FASTDB_TEST_MYSQL_HOST")
                    .unwrap_or_else(|_| "127.0.0.1".into()),
                port: port.parse().unwrap(),
                user: std::env::var("FASTDB_TEST_MYSQL_USER").unwrap_or_else(|_| "root".into()),
                password: std::env::var("FASTDB_TEST_MYSQL_PASSWORD").ok(),
                database: std::env::var("FASTDB_TEST_MYSQL_DATABASE")
                    .unwrap_or_else(|_| "mysql".into()),
                use_unix_socket: false,
                ..Default::default()
            });
            let mut conn = driver.get_pool().unwrap().get_conn().await.unwrap();
            let original: String = conn
                .query_first("SELECT @@SESSION.sql_mode")
                .await
                .unwrap()
                .unwrap();
            let value_column=crate::sql::SqlDialect(crate::DbDriverKind::MySql).quote_ident("value");
            conn.query_drop(format!("CREATE TEMPORARY TABLE fastdb_filter_values ({value_column} TEXT,score INTEGER) CHARACTER SET utf8mb4")).await.unwrap();
            for mode in ["", "NO_BACKSLASH_ESCAPES"] {
                conn.exec_drop("SET SESSION sql_mode=?", (mode,))
                    .await
                    .unwrap();
                for value in ["", "Tiếng Việt 'quote'\\path", "x' OR 1=1 --"] {
                    let spec = FilterSpec {
                        criteria: vec![Criterion {
                            column: "value".into(),
                            op: FilterOp::Equal,
                            value: value.into(),
                        }],
                        ..Default::default()
                    };
                    let predicate = spec
                        .sql(crate::DbDriverKind::MySql, &["value".into()])
                        .unwrap();
                    conn.query_drop("DELETE FROM fastdb_filter_values").await.unwrap();
                    conn.exec_drop(format!("INSERT INTO fastdb_filter_values({value_column},score) VALUES (?,10),('decoy',0)"),(value,)).await.unwrap();
                    let actual:Vec<String>=conn.query(format!("SELECT {value_column} FROM fastdb_filter_values {predicate}")).await.unwrap();
                    assert_eq!(actual,vec![value.to_string()]);
                    let numeric=FilterSpec{criteria:vec![Criterion{column:"score".into(),op:FilterOp::Greater,value:"5".into()}],..Default::default()};
                    let numeric=numeric.sql(crate::DbDriverKind::MySql,&["score".into()]).unwrap();
                    let rows:Vec<String>=conn.query(format!("SELECT {value_column} FROM fastdb_filter_values {numeric}")).await.unwrap();
                    assert_eq!(rows,vec![value.to_string()]);
                }
            }
            conn.exec_drop("SET SESSION sql_mode=?", (original,))
                .await
                .unwrap();
            conn.query_drop("SET SESSION MAX_EXECUTION_TIME=15000").await.unwrap();
            conn.query_drop("CREATE TEMPORARY TABLE fastdb_filter_plan (id INTEGER PRIMARY KEY)").await.unwrap();
            conn.query_drop("INSERT INTO fastdb_filter_plan WITH d AS (SELECT 0 AS n UNION ALL SELECT 1 UNION ALL SELECT 2 UNION ALL SELECT 3 UNION ALL SELECT 4 UNION ALL SELECT 5 UNION ALL SELECT 6 UNION ALL SELECT 7 UNION ALL SELECT 8 UNION ALL SELECT 9) SELECT 1+a.n+10*b.n+100*c.n+1000*d.n+10000*e.n FROM d a CROSS JOIN d b CROSS JOIN d c CROSS JOIN d d CROSS JOIN d e").await.unwrap();
            let predicate = FilterSpec {
                criteria: vec![Criterion { column: "id".into(), op: FilterOp::GreaterEqual, value: "99990".into() }],
                order: Some(crate::filter::FilterOrder {column: "id".into(), descending: false}),
                ..Default::default()
            }.sql(crate::DbDriverKind::MySql, &["id".into()]).unwrap();
            let plan: String = conn.query_first(format!("EXPLAIN ANALYZE SELECT id FROM fastdb_filter_plan {predicate} LIMIT 200")).await.unwrap().unwrap();
            assert!(plan.contains("index range scan") && plan.contains("99990 <= id") && plan.contains("rows=11 loops=1"), "{plan}");
            assert!(!plan.contains("Sort:"), "{plan}");
            eprintln!("structured MySQL filter plan (100000 rows): {plan}");

        });
    }
}
