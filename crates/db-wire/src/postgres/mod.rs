pub mod binary_decoder;
mod catalog;
mod column;
mod metadata;
mod objects;
mod query_edit;
mod script;
mod transfer;

use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use mem_engine::ColumnMeta;
use parking_lot::Mutex;
use platform_hal::socket::{DbKind, detect_local_unix_socket};
use std::sync::Arc;
use std::time::Instant;
use tokio_postgres::{CancelToken, Client, NoTls};
use tracing::{debug, info, warn};

use crate::stream::{RowSink, report_error};
use crate::traits::{ConnectionConfig, DatabaseDriver, StreamEvent, TableSummary, row_count_error};
use binary_decoder::{decode_pg_cell, pg_type_to_data_type};

type NoticeTarget = Arc<Mutex<Option<crate::stream::NoticeForwarder>>>;

pub struct PostgresDriver {
    pub config: ConnectionConfig,
    notices: NoticeTarget,
    client: tokio::sync::Mutex<Option<Arc<Client>>>,
    cancel_token: Arc<Mutex<Option<CancelToken>>>,
}

struct SocketGuard(tokio::task::JoinHandle<()>);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Server errors carry the useful text in their DbError; the Display impl only says "db error"
fn pg_error_message(e: &tokio_postgres::Error) -> String {
    match e.as_db_error() {
        Some(db) => {
            let position = match db.position() {
                Some(tokio_postgres::error::ErrorPosition::Original(p)) => {
                    format!(" · character {p}")
                }
                Some(tokio_postgres::error::ErrorPosition::Internal { position, .. }) => {
                    format!(" · internal query character {position}")
                }
                None => String::new(),
            };
            format!("{}: {}{position}", db.severity(), db.message())
        }
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

/// Uses system roots plus an optional private CA, always verifying the server identity.
fn tls_connector(config: &ConnectionConfig) -> Result<postgres_native_tls::MakeTlsConnector> {
    let mut builder = native_tls::TlsConnector::builder();
    builder.min_protocol_version(Some(native_tls::Protocol::Tlsv12));
    if let Some(path) = &config.tls_ca_cert {
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read TLS CA certificate: {path}"))?;
        let cert = native_tls::Certificate::from_pem(&bytes)
            .or_else(|_| native_tls::Certificate::from_der(&bytes))
            .with_context(|| format!("Invalid TLS CA certificate: {path}"))?;
        builder.add_root_certificate(cert);
    }
    Ok(postgres_native_tls::MakeTlsConnector::new(builder.build()?))
}

impl PostgresDriver {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            config,
            notices: Arc::default(),
            client: tokio::sync::Mutex::new(None),
            cancel_token: Arc::new(Mutex::new(None)),
        }
    }

    async fn get_or_connect(&self) -> Result<Arc<Client>> {
        let mut guard = self.client.lock().await;
        if let Some(ref client) = *guard
            && !client.is_closed()
        {
            return Ok(Arc::clone(client));
        }

        // Connect on the shared runtime so the socket outlives the caller's runtime
        let config = self.config.clone();
        let notices = self.notices.clone();
        let client = Arc::new(
            crate::run_on_shared_runtime(async move {
                Self::connect_with_notices(config, notices)
                    .await
                    .map(|(client, _)| client)
            })
            .await?,
        );
        *self.cancel_token.lock() = Some(client.cancel_token());
        *guard = Some(Arc::clone(&client));
        Ok(client)
    }

    async fn connect_with_task(
        config: ConnectionConfig,
    ) -> Result<(Client, tokio::task::JoinHandle<()>)> {
        Self::connect_with_notices(config, Arc::default()).await
    }

    async fn connect_with_notices(
        config: ConnectionConfig,
        notices: NoticeTarget,
    ) -> Result<(Client, tokio::task::JoinHandle<()>)> {
        let mut pg_config = tokio_postgres::Config::new();
        pg_config.user(&config.user);
        if let Some(ref pw) = config.password {
            pg_config.password(pw);
        }
        pg_config.dbname(&config.database);
        pg_config.connect_timeout(config.connect_timeout);

        // Check for local Unix Domain Socket if enabled
        let unix_socket = if config.use_unix_socket && !config.tls_enabled {
            detect_local_unix_socket(DbKind::Postgres, &config.host, config.port)
        } else {
            None
        };

        if let Some(socket_path) = unix_socket {
            info!("Connecting to PostgreSQL via Unix domain socket");
            pg_config.host_path(socket_path);
        } else {
            info!("Connecting to PostgreSQL via TCP");
            pg_config.host(&config.host);
            pg_config.port(config.port);
        }

        let client = if config.tls_enabled {
            pg_config.ssl_mode(tokio_postgres::config::SslMode::Require);
            let (client, mut connection) = pg_config
                .connect(tls_connector(&config)?)
                .await
                .with_context(|| {
                    format!(
                        "Failed to connect to PostgreSQL with TLS ({}:{})",
                        config.host, config.port
                    )
                })?;
            let task = crate::get_runtime().spawn(async move {
                while let Some(message) =
                    futures::future::poll_fn(|cx| connection.poll_message(cx)).await
                {
                    match message {
                        Ok(tokio_postgres::AsyncMessage::Notice(notice)) => {
                            if let Some(target) = notices.lock().as_mut() {
                                target.send(format!("{}: {}", notice.severity(), notice.message()));
                            }
                        }
                        Err(_) => {
                            warn!("PostgreSQL connection closed with an error");
                            break;
                        }
                        _ => {}
                    }
                }
            });
            (client, task)
        } else {
            pg_config.ssl_mode(tokio_postgres::config::SslMode::Disable);
            let (client, mut connection) = pg_config.connect(NoTls).await.with_context(|| {
                format!(
                    "Failed to connect to PostgreSQL ({}:{})",
                    config.host, config.port
                )
            })?;
            let task = crate::get_runtime().spawn(async move {
                while let Some(message) =
                    futures::future::poll_fn(|cx| connection.poll_message(cx)).await
                {
                    match message {
                        Ok(tokio_postgres::AsyncMessage::Notice(notice)) => {
                            if let Some(target) = notices.lock().as_mut() {
                                target.send(format!("{}: {}", notice.severity(), notice.message()));
                            }
                        }
                        Err(_) => {
                            warn!("PostgreSQL connection closed with an error");
                            break;
                        }
                        _ => {}
                    }
                }
            });
            (client, task)
        };

        Ok(client)
    }
}

#[async_trait]
impl DatabaseDriver for PostgresDriver {
    fn supports_transfer(&self) -> bool {
        true
    }

    async fn load_editable_query(&self, sql: &str) -> Result<Arc<crate::QueryResultPager>> {
        query_edit::load(self.config.clone(), sql.to_owned()).await
    }
    async fn save_query_edits(
        &self,
        pager: Arc<crate::QueryResultPager>,
        edits: Vec<crate::QueryCellUpdate>,
    ) -> Result<()> {
        query_edit::save(self.config.clone(), pager, edits).await
    }

    fn with_database(&self, database: &str) -> Result<Arc<dyn DatabaseDriver>> {
        crate::session_context::validate_name(database).map_err(anyhow::Error::msg)?;
        let mut config = self.config.clone();
        config.database = database.into();
        Ok(Arc::new(Self::new(config)))
    }

    fn metadata_capabilities(&self) -> crate::metadata::MetadataCapabilities {
        use crate::catalog::CatalogSection as C;
        use crate::metadata::{MetadataCapabilities, MetadataFeature as F};
        MetadataCapabilities::new(
            &[
                F::Columns,
                F::Constraints,
                F::Indexes,
                F::ForeignKeys,
                F::TableDetails,
                F::Catalog(C::Dependencies),
                F::Catalog(C::References),
                F::Catalog(C::Partitions),
                F::Catalog(C::Triggers),
                F::Catalog(C::Rules),
                F::Catalog(C::Policies),
                F::Catalog(C::Permissions),
            ],
            &[],
        )
    }

    fn supports_script_session(&self) -> bool {
        true
    }

    async fn open_script_session(&self) -> Result<Arc<dyn crate::script::ScriptSession>> {
        let config = self.config.clone();
        let notices = NoticeTarget::default();
        let target = notices.clone();
        let (client, task) = crate::run_on_shared_runtime(async move {
            Self::connect_with_notices(config, target).await
        })
        .await?;
        Ok(Arc::new(script::PostgresScriptSession::new(
            client,
            task,
            self.config.clone(),
            notices,
        )))
    }

    async fn list_objects(
        &self,
        kind: crate::objects::ObjectKind,
        after: u32,
    ) -> Result<Option<crate::objects::ObjectPage>> {
        self.object_page(kind, after).await.map(Some)
    }

    async fn object_source(
        &self,
        kind: crate::objects::ObjectKind,
        object: &crate::objects::ObjectSummary,
    ) -> Result<Option<String>> {
        self.read_object_source(kind, object).await.map(Some)
    }

    async fn execute_schema_ddl(&self, sql: &str) -> Result<()> {
        let (client, task) = Self::connect_with_task(self.config.clone()).await?;
        let _guard = SocketGuard(task);
        client
            .batch_execute("SET statement_timeout='15s'; SET lock_timeout='5s'")
            .await?;
        client
            .batch_execute(sql)
            .await
            .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))
    }

    async fn load_catalog_section(
        &self,
        schema: &str,
        table: &str,
        section: crate::catalog::CatalogSection,
    ) -> Result<Option<Vec<crate::catalog::CatalogRow>>> {
        self.catalog_section(schema, table, section).await
    }

    async fn load_column(
        &self,
        schema: &str,
        table: &str,
        column: &str,
    ) -> Result<crate::column::ColumnDefinition> {
        self.read_column_definition(schema, table, column).await
    }
    async fn reload_column(
        &self,
        original: &crate::column::ColumnDefinition,
    ) -> Result<crate::column::ColumnDefinition> {
        self.reload_column_identity(original).await
    }
    async fn list_column_types(&self) -> Result<Vec<String>> {
        self.column_type_choices().await
    }
    async fn apply_column(
        &self,
        original: &crate::column::ColumnDefinition,
        draft: &crate::column::ColumnDraft,
    ) -> crate::column::ColumnApplyOutcome {
        self.persist_column(original, draft).await
    }

    async fn ping(&self) -> Result<()> {
        let client = self.get_or_connect().await?;
        client.simple_query("SELECT 1;").await?;
        Ok(())
    }

    async fn execute_streaming(
        &self,
        sql: &str,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()> {
        let start_time = Instant::now();
        let client = self
            .get_or_connect()
            .await
            .map_err(|e| report_error(&event_tx, format!("{e:#}")))?;

        // Update cancel token for this connection
        *self.cancel_token.lock() = Some(client.cancel_token());

        stream_client(
            &client,
            sql,
            event_tx,
            start_time,
            &self.notices,
            None,
            false,
        )
        .await
    }

    async fn execute_streaming_controlled(
        &self,
        sql: &str,
        tx: flume::Sender<StreamEvent>,
        control: Arc<crate::execution::ExecutionControl>,
    ) -> Result<()> {
        control.check()?;
        let started = Instant::now();
        let client = self.get_or_connect().await?;
        control.check()?;
        *self.cancel_token.lock() = Some(client.cancel_token());
        stream_client(
            &client,
            sql,
            tx,
            started,
            &self.notices,
            Some(&control),
            false,
        )
        .await
    }

    async fn apply_row_changes(&self, statements: &[String]) -> Result<u64> {
        let client = self.get_or_connect().await?;
        client
            .batch_execute("BEGIN")
            .await
            .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
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
        client
            .batch_execute("COMMIT")
            .await
            .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
        Ok(statements.len() as u64)
    }

    async fn cancel_active_query(&self) -> Result<()> {
        let maybe_token = self.cancel_token.lock().clone();
        if let Some(token) = maybe_token {
            if self.config.tls_enabled {
                token.cancel_query(tls_connector(&self.config)?).await?;
            } else {
                token.cancel_query(NoTls).await?;
            }
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
                    estimated_rows: est_rows
                        .and_then(|x| if x >= 0 { Some(x as u64) } else { None }),
                }
            })
            .collect();

        Ok(tables)
    }

    async fn list_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnMeta>> {
        let client = self.get_or_connect().await?;
        Ok(metadata::columns(&client, Some(schema), Some(table))
            .await?
            .into_iter()
            .flat_map(|(_, _, columns)| columns)
            .collect())
    }

    async fn list_all_columns(&self) -> Result<Vec<(String, String, Vec<ColumnMeta>)>> {
        let client = self.get_or_connect().await?;
        metadata::columns(&client, None, None).await
    }

    async fn list_foreign_keys(&self) -> Result<Option<Vec<crate::ForeignKey>>> {
        let client = self.get_or_connect().await?;
        let rows = client
            .query(
                "SELECT c.conname, ns.nspname, src.relname, sa.attname,
                    nt.nspname, dst.relname, ta.attname
             FROM pg_constraint c
             JOIN pg_class src ON src.oid = c.conrelid
             JOIN pg_namespace ns ON ns.oid = src.relnamespace
             JOIN information_schema.tables visible
               ON visible.table_schema = ns.nspname AND visible.table_name = src.relname
             JOIN pg_class dst ON dst.oid = c.confrelid
             JOIN pg_namespace nt ON nt.oid = dst.relnamespace
             CROSS JOIN LATERAL unnest(c.conkey, c.confkey) WITH ORDINALITY k(s,t,n)
             JOIN pg_attribute sa ON sa.attrelid = src.oid AND sa.attnum = k.s
             JOIN pg_attribute ta ON ta.attrelid = dst.oid AND ta.attnum = k.t
             WHERE c.contype = 'f' AND ns.nspname NOT IN ('pg_catalog','information_schema')
             ORDER BY ns.nspname, src.relname, c.conname, k.n",
                &[],
            )
            .await?;
        Ok(Some(
            rows.into_iter()
                .map(|r| crate::ForeignKey {
                    name: r.get(0),
                    source_schema: r.get(1),
                    source_table: r.get(2),
                    source_column: r.get(3),
                    target_schema: r.get(4),
                    target_table: r.get(5),
                    target_column: r.get(6),
                })
                .collect(),
        ))
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
        Ok(rows
            .into_iter()
            .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
            .collect())
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
        Ok(rows
            .into_iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
    }

    async fn list_table_details(&self) -> Result<Vec<(String, String, crate::TableDetails)>> {
        metadata::load(self.get_or_connect().await?.as_ref()).await
    }
}

async fn stream_client(
    client: &Client,
    sql: &str,
    event_tx: flume::Sender<StreamEvent>,
    start_time: Instant,
    notices: &NoticeTarget,
    control: Option<&crate::execution::ExecutionControl>,
    editable: bool,
) -> Result<()> {
    struct NoticeGuard(NoticeTarget);
    impl Drop for NoticeGuard {
        fn drop(&mut self) {
            self.0.lock().take();
        }
    }
    *notices.lock() = Some(crate::stream::NoticeForwarder::new(event_tx.clone()));
    let _notices = NoticeGuard(notices.clone());
    let statement = client.prepare(sql).await.map_err(|e| {
        let reported = report_error(
            &event_tx,
            format!("SQL Preparation Error: {}", pg_error_message(&e)),
        );
        anyhow::Error::new(e).context(reported.to_string())
    })?;

    let columns: Vec<ColumnMeta> = statement
        .columns()
        .iter()
        .map(|c| ColumnMeta::new(c.name(), pg_type_to_data_type(c.type_())))
        .collect();
    let has_columns = !columns.is_empty();
    let origin = if editable {
        query_edit::origin(client, sql, &statement).await
    } else {
        None
    };
    let mut sink = RowSink::start_with_origin(columns, event_tx.clone(), start_time, origin)?;

    if let Some(control) = control {
        control.check()?;
    }
    let row_stream = client
        .query_raw(&statement, std::iter::empty::<i32>())
        .await
        .map_err(|e| {
            let reported = sink.fail(format!("Query execution error: {}", pg_error_message(&e)));
            anyhow::Error::new(e).context(reported.to_string())
        })?;
    futures::pin_mut!(row_stream);

    while let Some(row_res) = row_stream.next().await {
        let row = row_res.map_err(|e| {
            let reported = sink.fail(format!("Row stream error: {}", pg_error_message(&e)));
            anyhow::Error::new(e).context(reported.to_string())
        })?;
        for (col_idx, cell) in sink.cells_mut().iter_mut().enumerate() {
            *cell = decode_pg_cell(&row, col_idx);
        }
        sink.commit_row()?;
    }

    if !has_columns {
        let _ = event_tx.send(StreamEvent::CommandComplete {
            affected_rows: row_stream.rows_affected(),
        });
    }
    let total_rows = sink.finish();
    debug!(
        "Postgres query streaming completed: {} rows in {}ms",
        total_rows,
        start_time.elapsed().as_millis()
    );
    Ok(())
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

        assert_eq!(
            info_schema_data_type("timestamp with time zone"),
            DataType::TimestampTz
        );
        assert_eq!(info_schema_data_type("BIGINT"), DataType::Int8);
        assert_eq!(info_schema_data_type("tsvector"), DataType::Text);
    }
}
