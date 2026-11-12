use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use mem_engine::{ColumnMeta, DataType};
use mysql_async::prelude::*;
use mysql_async::{Opts, OptsBuilder, Params, Pool, SslOpts, TxOpts, Value};
use parking_lot::Mutex;
use platform_hal::socket::{DbKind, detect_local_unix_socket};
use std::sync::Arc;
use std::time::{Duration, Instant};

mod data_search;
mod definition_search;
mod metadata;
mod read_only;
mod script;

use crate::stream::{RowSink, report_error};
use crate::traits::{ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary, row_count_error};

pub struct MySqlDriver {
    pub config: ConnectionConfig,
    pool: tokio::sync::Mutex<Option<Arc<ConnectionPool>>>,
    read_only: bool,
    /// Server connection id of the connection running the current query, for KILL QUERY
    active_query: ActiveQuerySlot,
}

type ActiveQuerySlot = Arc<Mutex<Option<Arc<ActiveQuery>>>>;

struct ActiveQuery {
    id: u32,
    pool: Arc<ConnectionPool>,
}

struct ActiveQueryGuard {
    slot: ActiveQuerySlot,
    query: Arc<ActiveQuery>,
}

impl Drop for ActiveQueryGuard {
    fn drop(&mut self) {
        let mut slot = self.slot.lock();
        if slot
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, &self.query))
        {
            *slot = None;
        }
    }
}

struct ConnectionPool {
    pool: Pool,
    tunnel: Option<crate::SshTunnel>,
    timeout: Duration,
    read_only: bool,
}

impl ConnectionPool {
    async fn get_conn(&self) -> Result<mysql_async::Conn> {
        tokio::time::timeout(self.timeout, self.pool.get_conn())
            .await
            .map_err(|_| anyhow::anyhow!("MySQL connection timed out"))?
            .map_err(anyhow::Error::from)
    }
}

impl MySqlDriver {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            read_only: config.read_only,
            config,
            pool: tokio::sync::Mutex::new(None),
            active_query: Arc::new(Mutex::new(None)),
        }
    }

    fn get_opts(
        &self,
        tunnel_port: Option<u16>,
        identity: Option<mysql_async::ClientIdentity>,
    ) -> Opts {
        let mut builder = OptsBuilder::default();
        builder = builder.user(Some(&self.config.user));
        if let Some(ref pw) = self.config.password {
            builder = builder.pass(Some(pw));
        }
        builder = builder.db_name(Some(&self.config.database));

        let use_socket =
            tunnel_port.is_none() && self.config.use_unix_socket && !self.config.tls_enabled;
        let socket = if use_socket {
            detect_local_unix_socket(DbKind::MySql, &self.config.host, self.config.port)
        } else {
            None
        };

        if let Some(port) = tunnel_port {
            builder = builder.ip_or_hostname("127.0.0.1").tcp_port(port);
        } else if let Some(sock_path) = socket {
            builder = builder.socket(Some(sock_path.to_string_lossy().to_string()));
        } else {
            builder = builder
                .ip_or_hostname(self.config.host.clone())
                .tcp_port(self.config.port);
        }

        // Disable mysql_async's implicit TCP-to-socket upgrade when TLS is requested.
        builder = builder.prefer_socket(use_socket);
        if self.config.tls_enabled {
            let mut ssl = SslOpts::default();
            if let Some(path) = &self.config.tls_ca_cert {
                ssl = ssl.with_root_certs(vec![std::path::PathBuf::from(path).into()]);
            }
            if tunnel_port.is_some() {
                ssl = ssl.with_danger_tls_hostname_override(Some(self.config.host.clone()));
            }
            ssl = ssl.with_client_identity(identity);
            builder = builder.ssl_opts(ssl);
        }
        builder.into()
    }

    async fn connection_opts(&self, tunnel_port: Option<u16>) -> Result<Opts> {
        self.config.validate_tls_identity()?;
        let identity = if let Some(identity) = self.config.tls_client_identity.clone() {
            let deadline = self.config.connect_timeout;
            Some(
                crate::run_on_shared_runtime(async move {
                    tokio::time::timeout(
                        deadline,
                        tokio::task::spawn_blocking(move || {
                            let archive = identity.read_archive()?;
                            identity.parse_archive(&archive)?;
                            Ok::<_, anyhow::Error>(
                                mysql_async::ClientIdentity::new(archive.into())
                                    .with_password(identity.password.unwrap_or_default()),
                            )
                        }),
                    )
                    .await
                    .map_err(|_| anyhow::anyhow!("Loading the TLS client identity timed out"))?
                    .map_err(|_| anyhow::anyhow!("Could not load the TLS client identity"))?
                })
                .await?,
            )
        } else {
            None
        };
        Ok(self.get_opts(tunnel_port, identity))
    }

    async fn open_tunnel(&self) -> Result<Option<crate::SshTunnel>> {
        let Some(settings) = &self.config.ssh_tunnel else {
            return Ok(None);
        };
        crate::SshTunnel::start_forwarding(
            settings.clone(),
            self.config.host.clone(),
            self.config.port.into(),
        )
        .await
        .map(Some)
    }

    async fn get_pool(&self) -> Result<Arc<ConnectionPool>> {
        let mut guard = self.pool.lock().await;
        if let Some(pool) = &*guard
            && !pool
                .tunnel
                .as_ref()
                .is_some_and(crate::SshTunnel::is_closed)
        {
            return Ok(pool.clone());
        }

        let tunnel = self.open_tunnel().await?;
        let opts = self
            .connection_opts(tunnel.as_ref().map(|tunnel| tunnel.local_bound_port))
            .await?;
        let pool = Arc::new(ConnectionPool {
            pool: Pool::new(opts),
            tunnel,
            timeout: self.config.connect_timeout,
            read_only: self.read_only,
        });
        *guard = Some(pool.clone());
        Ok(pool)
    }

    /// Runs a metadata query with bound parameters on the shared runtime
    async fn query_rows<T, P>(&self, sql: String, params: P) -> Result<Vec<T>>
    where
        T: FromRow + Send + 'static,
        P: Into<Params> + Send + 'static,
    {
        let pool = self.get_pool().await?;
        crate::run_on_shared_runtime(async move {
            let mut conn = pool.get_conn().await?;
            Ok(async {
                crate::sql_observer::observe!("MySQL", sql, |query| conn.exec(query, params).await)
            }
            .await?)
        })
        .await
    }

    /// Streams a query's rows into a new pager; runs on the shared runtime via the caller
    async fn stream_query(
        pool: Arc<ConnectionPool>,
        sql: String,
        event_tx: flume::Sender<StreamEvent>,
        active_query: ActiveQuerySlot,
        control: Option<Arc<crate::execution::ExecutionControl>>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let mut conn = pool
            .get_conn()
            .await
            .map_err(|e| report_error(&event_tx, format!("MySQL connection error: {e}")))?;
        let read_only = pool.read_only;
        let query = Arc::new(ActiveQuery {
            id: conn.id(),
            pool,
        });
        *active_query.lock() = Some(query.clone());
        let _active = ActiveQueryGuard {
            slot: active_query,
            query,
        };
        Self::stream_connection(
            &mut conn,
            sql,
            event_tx,
            control.as_deref(),
            start_time,
            read_only,
        )
        .await
    }

    async fn stream_connection(
        conn: &mut mysql_async::Conn,
        sql: String,
        event_tx: flume::Sender<StreamEvent>,
        control: Option<&crate::execution::ExecutionControl>,
        start_time: Instant,
        read_only: bool,
    ) -> Result<()> {
        let mut observation = crate::sql_observer::Statement::start("MySQL", &sql);
        let mut observed_rows = None;
        let outcome = async {
            if let Some(control) = &control {
                control.check()?;
            }
            let (sql, prepared) = if read_only {
                read_only::prepare(conn, &sql)
                    .await
                    .map_err(|error| report_error(&event_tx, format!("{error:#}")))?
            } else {
                (std::borrow::Cow::Borrowed(sql.as_str()), false)
            };
            if let Some(control) = &control {
                control.check()?;
            }
            let report =
                |error| report_error(&event_tx, format!("MySQL query execution error: {error}"));
            let (sink, warnings, info) = if prepared {
                // The server parses exactly one statement under the connection's SQL mode.
                let result = conn
                    .exec_iter(sql.as_ref(), Params::Empty)
                    .await
                    .map_err(report)?;
                Self::stream_result(result, &event_tx, start_time).await?
            } else {
                let result = conn.query_iter(sql.as_ref()).await.map_err(report)?;
                Self::stream_result(result, &event_tx, start_time).await?
            };
            if !info.is_empty() {
                let _ = event_tx.send(StreamEvent::Notice(info));
            }
            if warnings > 0 {
                let _ = event_tx.send(StreamEvent::Notice(format!(
                    "MySQL: {warnings} server warning(s); showing up to 20"
                )));
                let messages = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                    crate::sql_observer::observe!("MySQL", "SHOW WARNINGS LIMIT 20", |query| conn
                        .query::<(String, u16, String), _>(query)
                        .await)
                })
                .await;
                match messages {
                    Ok(Ok(messages)) => {
                        for (level, code, text) in messages {
                            let _ = event_tx
                                .send(StreamEvent::Notice(format!("{level} {code}: {text}")));
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
            observed_rows = Some(if sink.num_columns() == 0 {
                conn.affected_rows()
            } else {
                sink.total_rows()
            });
            sink.finish();
            Ok(())
        }
        .await;
        observation.finish(&outcome, observed_rows);
        outcome
    }
    async fn stream_result<P: mysql_async::prelude::Protocol + Unpin>(
        mut query_result: mysql_async::QueryResult<'_, '_, P>,
        event_tx: &flume::Sender<StreamEvent>,
        start_time: Instant,
    ) -> Result<(RowSink, u16, String)> {
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
                        *cell = mysql_cell_to_bytes(row.get(i), &row.columns_ref()[i]);
                    }
                    sink.commit_row()?;
                }
            }
        }
        let warnings = query_result.warnings();
        let info = query_result.info().into_owned();
        Ok((sink, warnings, info))
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

fn mysql_cell_to_bytes(value: Option<Value>, column: &mysql_async::Column) -> Option<Vec<u8>> {
    use mysql_async::consts::ColumnType::{MYSQL_TYPE_DATE, MYSQL_TYPE_NEWDATE};
    let scale = usize::from(column.decimals().min(6));
    let fractional = |micros| {
        if scale == 0 {
            String::new()
        } else {
            let digits = format!("{micros:06}");
            format!(".{}", &digits[..scale])
        }
    };
    match value {
        Some(Value::Date(y, m, d, _, _, _, _))
            if matches!(column.column_type(), MYSQL_TYPE_DATE | MYSQL_TYPE_NEWDATE) =>
        {
            Some(format!("{y:04}-{m:02}-{d:02}").into_bytes())
        }
        Some(Value::Date(y, m, d, h, min, s, micros)) => Some(
            format!(
                "{y:04}-{m:02}-{d:02} {h:02}:{min:02}:{s:02}{}",
                fractional(micros)
            )
            .into_bytes(),
        ),
        Some(Value::Time(negative, days, hours, minutes, seconds, micros)) => {
            let sign = if negative { "-" } else { "" };
            let hours = u64::from(days) * 24 + u64::from(hours);
            Some(
                format!(
                    "{sign}{hours:02}:{minutes:02}:{seconds:02}{}",
                    fractional(micros)
                )
                .into_bytes(),
            )
        }
        value => mysql_value_to_bytes(value),
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
    fn is_read_only(&self) -> bool {
        self.read_only
    }

    fn supports_transfer(&self) -> bool {
        !self.is_read_only()
    }

    fn with_database(&self, database: &str) -> Result<Arc<dyn DatabaseDriver>> {
        crate::session_context::validate_name(database).map_err(anyhow::Error::msg)?;
        let mut config = self.config.clone();
        config.database = database.into();
        config.read_only = self.read_only;
        Ok(Arc::new(Self::new(config)))
    }

    fn supports_script_session(&self) -> bool {
        true
    }

    async fn open_script_session(&self) -> Result<Arc<dyn crate::script::ScriptSession>> {
        let tunnel = self.open_tunnel().await?;
        let opts = self
            .connection_opts(tunnel.as_ref().map(|tunnel| tunnel.local_bound_port))
            .await?;
        let timeout = self.config.connect_timeout;
        let read_only = self.read_only;
        crate::run_on_shared_runtime(async move {
            let conn = tokio::time::timeout(timeout, mysql_async::Conn::new(opts))
                .await
                .map_err(|_| anyhow::anyhow!("MySQL script connection timed out"))??;
            Ok(
                Arc::new(script::MySqlScriptSession::new(conn, tunnel, read_only))
                    as Arc<dyn crate::script::ScriptSession>,
            )
        })
        .await
    }

    fn metadata_capabilities(&self) -> crate::metadata::MetadataCapabilities {
        use crate::catalog::CatalogSection as C;
        use crate::metadata::{MetadataCapabilities, MetadataFeature as F};
        let available: &'static [F] = if self.config.driver == crate::DbDriverKind::MySql {
            &[
                F::Columns,
                F::Constraints,
                F::Indexes,
                F::ForeignKeys,
                F::TableDetails,
            ]
        } else {
            &[F::Columns, F::Constraints, F::Indexes, F::ForeignKeys]
        };
        MetadataCapabilities::new(available, &[F::Catalog(C::Rules), F::Catalog(C::Policies)])
    }

    async fn ping(&self) -> Result<()> {
        let pool = self.get_pool().await?;
        crate::run_on_shared_runtime(async move {
            let mut conn = pool.get_conn().await?;
            async {
                crate::sql_observer::observe!("MySQL", "SELECT 1;", |query| conn
                    .query_drop(query)
                    .await)
            }
            .await?;
            Ok(())
        })
        .await
    }

    async fn search_definitions(
        &self,
        schema: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<Vec<crate::objects::DefinitionHit>>> {
        self.find_definitions(schema, needle, cancel).await
    }

    async fn search_table_data(
        &self,
        schema: &str,
        table: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<Vec<crate::objects::DataSearchHit>>> {
        self.find_table_data(schema, table, needle, cancel).await
    }

    async fn read_data_search_row(
        &self,
        locator: &crate::objects::DataRowLocator,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<crate::objects::DataRowPreview>> {
        anyhow::ensure!(
            !cancel.is_disconnected() && cancel.try_recv().is_err(),
            "Data search cancelled"
        );
        let crate::objects::DataRowIdentity::MySql(row) = &locator.0 else {
            anyhow::bail!("Search result belongs to another provider");
        };
        Ok(Some(row.clone()))
    }

    async fn execute_schema_ddl(&self, sql: &str) -> Result<()> {
        anyhow::ensure!(!self.is_read_only(), "{}", read_only::DENIED);
        let pool = self.get_pool().await?;
        let sql = sql.to_string();
        crate::run_on_shared_runtime(async move {
            let mut conn = pool.get_conn().await?;
            async {
                crate::sql_observer::observe!("MySQL", &sql, |query| conn.query_drop(query).await)
            }
            .await?;
            Ok(())
        })
        .await
    }

    async fn execute_streaming(
        &self,
        sql: &str,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()> {
        let pool = self.get_pool().await?;
        let sql = sql.to_string();
        let active_query = Arc::clone(&self.active_query);
        crate::run_on_shared_runtime(Self::stream_query(pool, sql, event_tx, active_query, None))
            .await
    }

    async fn execute_streaming_controlled(
        &self,
        sql: &str,
        tx: flume::Sender<StreamEvent>,
        control: Arc<crate::execution::ExecutionControl>,
    ) -> Result<()> {
        control.check()?;
        let pool = self.get_pool().await?;
        crate::run_on_shared_runtime(Self::stream_query(
            pool,
            sql.to_owned(),
            tx,
            self.active_query.clone(),
            Some(control),
        ))
        .await
    }

    async fn apply_row_changes(&self, statements: &[String]) -> Result<u64> {
        anyhow::ensure!(!self.is_read_only(), "{}", read_only::DENIED);
        let pool = self.get_pool().await?;
        let statements = statements.to_vec();
        crate::run_on_shared_runtime(async move {
            let mut conn = pool.get_conn().await?;
            let mut tx =
                crate::sql_observer::observe!("MySQL", "START TRANSACTION", |_query| conn
                    .start_transaction(TxOpts::default())
                    .await)?;
            for sql in &statements {
                let outcome = match async {
                    crate::sql_observer::observe!(
                        "MySQL",
                        sql.as_str(),
                        |query| tx.query_drop(query).await,
                        |_result| Some(tx.affected_rows())
                    )
                }
                .await
                {
                    Ok(()) if tx.affected_rows() == 1 => Ok(()),
                    Ok(()) => Err(row_count_error(sql, tx.affected_rows())),
                    Err(e) => Err(anyhow::anyhow!("MySQL error: {e}")),
                };
                if let Err(e) = outcome {
                    crate::sql_observer::observe!("MySQL", "ROLLBACK", |_query| tx
                        .rollback()
                        .await)?;
                    return Err(e);
                }
            }
            crate::sql_observer::observe!("MySQL", "COMMIT", |_query| tx.commit().await)?;
            Ok(statements.len() as u64)
        })
        .await
    }

    async fn cancel_active_query(&self) -> Result<()> {
        let Some(active) = self.active_query.lock().clone() else {
            return Ok(());
        };
        crate::run_on_shared_runtime(async move {
            // A reconnect can target another server. Use the exact pool which
            // opened the running connection when issuing its cancellation.
            let mut conn = active.pool.get_conn().await?;
            let conn_id = active.id;
            async {
                crate::sql_observer::observe!("MySQL", format!("KILL QUERY {conn_id}"), |query| {
                    conn.query_drop(query).await
                })
            }
            .await?;
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

    async fn load_relation_ddl(&self, schema: &str, table: &str) -> Result<Option<String>> {
        use anyhow::Context as _;
        anyhow::ensure!(
            !schema.is_empty() && !table.is_empty(),
            "A database and relation are required"
        );
        let qualified =
            crate::sql::SqlDialect(crate::DbDriverKind::MySql).qualified_table(schema, table);
        let rows: Vec<mysql_async::Row> = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.query_rows(format!("SHOW CREATE TABLE {qualified}"), ()),
        )
        .await
        .context("MySQL relation DDL timed out")??;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let source: String = row
            .get_opt(1)
            .context("MySQL did not return relation DDL")??;
        anyhow::ensure!(source.len() <= 1024 * 1024, "Relation DDL exceeds 1 MiB");
        Ok(Some(format!(
            "{};\n",
            source.trim_end().trim_end_matches(';')
        )))
    }

    async fn list_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnMeta>> {
        let rows: Vec<mysql_async::Row> = self
            .query_rows(
                metadata::column_query(true),
                (schema.to_owned(), table.to_owned()),
            )
            .await?;
        rows.iter().map(metadata::column).collect()
    }

    async fn list_all_columns(&self) -> Result<Vec<(String, String, Vec<ColumnMeta>)>> {
        metadata::grouped(self.query_rows(metadata::column_query(false), ()).await?)
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

    async fn list_table_details(&self) -> Result<Vec<(String, String, crate::TableDetails)>> {
        use anyhow::Context as _;
        if self.config.driver != crate::DbDriverKind::MySql {
            return Ok(Vec::new());
        }
        let rows = tokio::time::timeout(
            Duration::from_secs(15),
            self.query_rows(
                metadata::TABLE_STATISTICS.to_owned(),
                (metadata::MAX_STATISTICS_TABLES + 1,),
            ),
        )
        .await
        .context("MySQL table statistics timed out")??;
        metadata::table_statistics(rows)
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
        .get_opts(None, None);
        assert!(!opts.prefer_socket());
        assert!(opts.socket().is_none());
        let ssl = opts.ssl_opts().unwrap();
        assert!(!ssl.skip_domain_validation());
        assert!(!ssl.accept_invalid_certs());
        assert_eq!(ssl.root_certs().len(), 1);
    }

    #[test]
    fn ssh_dials_loopback_and_verifies_the_database_hostname() {
        let opts = MySqlDriver::new(ConnectionConfig {
            host: "database.internal".into(),
            tls_enabled: true,
            use_unix_socket: true,
            ..ConnectionConfig::default()
        })
        .get_opts(Some(15432), None);
        assert_eq!(opts.ip_or_hostname(), "127.0.0.1");
        assert_eq!(opts.tcp_port(), 15432);
        assert!(!opts.prefer_socket());
        assert!(opts.socket().is_none());
        let ssl = opts.ssl_opts().unwrap();
        assert_eq!(
            ssl,
            &SslOpts::default().with_danger_tls_hostname_override(Some("database.internal"))
        );
        assert!(!ssl.skip_domain_validation());
        assert!(!ssl.accept_invalid_certs());
    }

    #[test]
    fn legacy_connection_does_not_enable_tls() {
        let opts = MySqlDriver::new(ConnectionConfig {
            use_unix_socket: false,
            ..ConnectionConfig::default()
        })
        .get_opts(None, None);
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
            let mut conn = driver.get_pool().await.unwrap().get_conn().await.unwrap();
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
