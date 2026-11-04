use anyhow::Result;
use async_trait::async_trait;
pub use mem_engine::{ColumnMeta, DataType, QueryResultPager};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbDriverKind {
    Postgres,
    MySql,
    MariaDb,
    SqlServer,
    Oracle,
    Db2,
    Sqlite,
}

impl DbDriverKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Postgres => "Postgres",
            Self::MySql => "MySql",
            Self::MariaDb => "MariaDb",
            Self::Sqlite => "Sqlite",
            Self::SqlServer => "SqlServer",
            Self::Oracle => "Oracle",
            Self::Db2 => "Db2",
        }
    }
    pub fn from_name(name: &str) -> Option<Self> {
        [
            Self::Postgres,
            Self::MySql,
            Self::MariaDb,
            Self::Sqlite,
            Self::SqlServer,
            Self::Oracle,
            Self::Db2,
        ]
        .into_iter()
        .find(|kind| kind.name().eq_ignore_ascii_case(name))
    }
    pub fn default_port(self) -> u16 {
        match self {
            Self::Postgres => 5432,
            Self::MySql | Self::MariaDb => 3306,
            Self::Sqlite => 0,
            Self::SqlServer => 1433,
            Self::Oracle => 1521,
            Self::Db2 => 50000,
        }
    }
    pub fn uses_odbc(self) -> bool {
        matches!(self, Self::SqlServer | Self::Oracle | Self::Db2)
    }
}

#[derive(Debug, Clone)]
pub struct SshTunnelConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_path: Option<String>,
    pub password: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    pub driver: DbDriverKind,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
    pub use_unix_socket: bool,
    /// Require TLS with certificate and hostname verification (TCP only).
    pub tls_enabled: bool,
    /// Optional PEM or DER root CA certificate; system trust is used otherwise.
    pub tls_ca_cert: Option<String>,
    pub ssh_tunnel: Option<SshTunnelConfig>,
    pub connect_timeout: Duration,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            driver: DbDriverKind::Postgres,
            host: "localhost".to_string(),
            port: 5432,
            database: "postgres".to_string(),
            user: "postgres".to_string(),
            password: None,
            use_unix_socket: true,
            tls_enabled: false,
            tls_ca_cert: None,
            ssh_tunnel: None,
            connect_timeout: Duration::from_secs(5),
        }
    }
}

/// Query progress: schema/rows and optional notices/command count, then Completed or Error.
#[derive(Clone)]
pub enum StreamEvent {
    /// Result columns are known; rows are appended into this pager as they arrive
    SchemaReady(Arc<QueryResultPager>),
    PageAppended {
        total_rows: u64,
    },
    Completed {
        total_rows: u64,
        elapsed_ms: u64,
    },
    /// Native command count; None means the provider cannot determine it.
    CommandComplete {
        affected_rows: Option<u64>,
    },
    Notice(String),
    Error(String),
}

impl std::fmt::Debug for StreamEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamEvent::SchemaReady(pager) => {
                write!(f, "SchemaReady({} columns)", pager.num_cols())
            }
            StreamEvent::PageAppended { total_rows } => {
                write!(f, "PageAppended {{ total_rows: {total_rows} }}")
            }
            StreamEvent::Completed {
                total_rows,
                elapsed_ms,
            } => {
                write!(
                    f,
                    "Completed {{ total_rows: {total_rows}, elapsed_ms: {elapsed_ms} }}"
                )
            }
            StreamEvent::CommandComplete { affected_rows } => {
                write!(f, "CommandComplete({affected_rows:?})")
            }
            StreamEvent::Notice(message) => write!(f, "Notice({message:?})"),
            StreamEvent::Error(e) => write!(f, "Error({e:?})"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TableSummary {
    pub schema: String,
    pub name: String,
    pub table_type: String, // "BASE TABLE" or "VIEW"
    pub estimated_rows: Option<u64>,
}

/// Optional catalog data. None means unknown or inapplicable, never zero/false.
#[derive(Debug, Clone, Default)]
pub struct TableDetails {
    /// Catalog-derived structural DDL; dependencies must already exist.
    pub ddl: Option<String>,
    pub object_id: Option<String>,
    pub owner: Option<String>,
    pub tablespace: Option<String>,
    pub comment: Option<String>,
    pub persistence: Option<String>,
    pub row_security: Option<bool>,
    pub force_row_security: Option<bool>,
    pub partitioned: Option<bool>,
    pub partition_key: Option<String>,
    pub estimated_rows: Option<i64>,
    pub table_bytes: Option<i64>,
    pub index_bytes: Option<i64>,
    pub total_bytes: Option<i64>,
    pub last_vacuum: Option<String>,
    pub last_autovacuum: Option<String>,
    pub last_analyze: Option<String>,
    pub last_autoanalyze: Option<String>,
    pub indexes: Vec<IndexDetails>,
    pub constraints: Vec<ConstraintDetails>,
}

#[derive(Debug, Clone)]
pub struct IndexDetails {
    pub name: String,
    pub method: String,
    /// Ordered key expressions, followed by any INCLUDE columns.
    pub columns: Vec<String>,
    pub unique: bool,
    pub valid: bool,
    pub definition: String,
}

#[derive(Debug, Clone)]
pub struct ConstraintDetails {
    pub name: String,
    pub kind: String,
    pub columns: Vec<String>,
    pub definition: String,
    pub validated: bool,
}

/// One ordered column mapping in a foreign-key constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForeignKey {
    pub name: String,
    pub source_schema: String,
    pub source_table: String,
    pub source_column: String,
    pub target_schema: String,
    pub target_table: String,
    pub target_column: String,
}

pub struct QueryCellUpdate {
    pub row: u64,
    pub column: usize,
    pub value: Option<String>,
}

#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    async fn load_editable_query(&self, _sql: &str) -> Result<Arc<QueryResultPager>> {
        anyhow::bail!("Editable query results are unavailable for this provider")
    }

    async fn save_query_edits(
        &self,
        _pager: Arc<QueryResultPager>,
        _edits: Vec<QueryCellUpdate>,
    ) -> Result<()> {
        anyhow::bail!("Editable query results are unavailable for this provider")
    }

    /// Creates an independent driver with the same endpoint/authentication and a new database.
    /// Does not open a session, execute SQL or modify this driver.
    fn with_database(&self, _database: &str) -> Result<Arc<dyn DatabaseDriver>> {
        anyhow::bail!("Database switching is not supported by this provider")
    }

    /// None denotes unsupported discovery, distinct from a successfully empty page.
    async fn list_objects(
        &self,
        _kind: crate::objects::ObjectKind,
        _after: u32,
    ) -> Result<Option<crate::objects::ObjectPage>> {
        Ok(None)
    }

    /// Native identity must still resolve to the requested schema, name and qualifier.
    async fn object_source(
        &self,
        _kind: crate::objects::ObjectKind,
        _object: &crate::objects::ObjectSummary,
    ) -> Result<Option<String>> {
        Ok(None)
    }

    /// Executes a schema DDL statement. Providers with transactional DDL run inside a transaction.
    async fn execute_schema_ddl(&self, sql: &str) -> Result<()> {
        self.execute_query(sql).await.map(|_| ())
    }

    /// Declares implemented readers; availability does not imply permission or nonempty results.
    fn metadata_capabilities(&self) -> crate::metadata::MetadataCapabilities {
        crate::metadata::MetadataCapabilities::default()
    }

    /// None is unsupported; Some(empty) is a successfully loaded empty catalog.
    async fn load_catalog_section(
        &self,
        _schema: &str,
        _table: &str,
        _section: crate::catalog::CatalogSection,
    ) -> Result<Option<Vec<crate::catalog::CatalogRow>>> {
        Ok(None)
    }

    async fn load_column(
        &self,
        _schema: &str,
        _table: &str,
        _column: &str,
    ) -> Result<crate::column::ColumnDefinition> {
        anyhow::bail!("Column editor is available for PostgreSQL only")
    }
    async fn reload_column(
        &self,
        original: &crate::column::ColumnDefinition,
    ) -> Result<crate::column::ColumnDefinition> {
        self.load_column(&original.schema, &original.table, &original.name)
            .await
    }
    async fn list_column_types(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }
    async fn apply_column(
        &self,
        _original: &crate::column::ColumnDefinition,
        _draft: &crate::column::ColumnDraft,
    ) -> crate::column::ColumnApplyOutcome {
        crate::column::ColumnApplyOutcome::Rejected("Column editing unsupported by provider".into())
    }

    /// Whether Run script can lease an isolated physical connection.
    fn supports_script_session(&self) -> bool {
        false
    }

    async fn open_script_session(&self) -> Result<Arc<dyn crate::script::ScriptSession>> {
        anyhow::bail!("Pinned script sessions are not supported by this provider")
    }

    /// Test connection to verify credentials and connectivity
    async fn ping(&self) -> Result<()>;

    /// Stream query results into a new QueryResultPager, announced via StreamEvent::SchemaReady.
    /// Errors are both sent as StreamEvent::Error and returned.
    async fn execute_streaming(
        &self,
        sql: &str,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()>;

    /// Providers check control after acquiring a connection, before sending SQL.
    async fn execute_streaming_controlled(
        &self,
        sql: &str,
        tx: flume::Sender<StreamEvent>,
        control: Arc<crate::execution::ExecutionControl>,
    ) -> Result<()> {
        control.check()?;
        self.execute_streaming(sql, tx).await
    }

    /// Execute query to completion and return the populated pager, total rows, and elapsed ms
    async fn execute_query(&self, sql: &str) -> Result<(Arc<QueryResultPager>, u64, u64)> {
        let (event_tx, event_rx) = flume::unbounded();
        self.execute_streaming(sql, event_tx).await?;

        let mut pager = None;
        let mut summary = None;
        for event in event_rx.drain() {
            match event {
                StreamEvent::SchemaReady(p) => pager = Some(p),
                StreamEvent::Completed {
                    total_rows,
                    elapsed_ms,
                } => summary = Some((total_rows, elapsed_ms)),
                StreamEvent::Error(e) => anyhow::bail!(e),
                StreamEvent::PageAppended { .. }
                | StreamEvent::CommandComplete { .. }
                | StreamEvent::Notice(_) => {}
            }
        }

        match (pager, summary) {
            (Some(pager), Some((total_rows, elapsed_ms))) => Ok((pager, total_rows, elapsed_ms)),
            _ => anyhow::bail!("Query finished without producing a result"),
        }
    }

    /// Runs row edits (UPDATE / INSERT / DELETE) in one transaction. Every statement must
    /// touch exactly one row; otherwise everything is rolled back and an error is returned.
    /// Returns the number of statements applied.
    async fn apply_row_changes(&self, statements: &[String]) -> Result<u64>;

    /// Cancel actively running query out-of-band immediately
    async fn cancel_active_query(&self) -> Result<()>;

    /// List schemas and tables for sidebar tree and autocomplete
    async fn list_tables(&self) -> Result<Vec<TableSummary>>;

    /// List column metadata for a specific table
    async fn list_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnMeta>>;

    /// Batch list all columns for all tables across the database
    async fn list_all_columns(&self) -> Result<Vec<(String, String, Vec<ColumnMeta>)>> {
        let tables = self.list_tables().await?;
        let mut results = Vec::new();
        for t in &tables {
            let cols = self.list_columns(&t.schema, &t.name).await?;
            results.push((t.schema.clone(), t.name.clone(), cols));
        }
        Ok(results)
    }

    /// Batch list all constraints across all tables: (schema, table, constraint_name, constraint_type)
    async fn list_all_constraints(&self) -> Result<Vec<(String, String, String, String)>> {
        Ok(Vec::new())
    }

    /// None means unsupported, distinct from a supported catalog with no FKs.
    async fn list_foreign_keys(&self) -> Result<Option<Vec<ForeignKey>>> {
        Ok(None)
    }

    /// Batch list all indexes across all tables: (schema, table, index_name)
    async fn list_all_indexes(&self) -> Result<Vec<(String, String, String)>> {
        Ok(Vec::new())
    }

    /// Optional table details, fetched during background schema refresh.
    /// Unsupported providers return an empty list, preserving unavailable UI values.
    async fn list_table_details(&self) -> Result<Vec<(String, String, TableDetails)>> {
        Ok(Vec::new())
    }
}

/// Error for a row edit that touched no row or several rows
pub fn row_count_error(statement: &str, affected: u64) -> anyhow::Error {
    if affected == 0 {
        anyhow::anyhow!("The row was not found (changed or deleted by someone else?): {statement}")
    } else {
        anyhow::anyhow!(
            "The statement would change {affected} rows instead of one, so nothing was saved: {statement}"
        )
    }
}
