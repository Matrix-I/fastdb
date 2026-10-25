use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use async_trait::async_trait;
pub use mem_engine::{ColumnMeta, QueryResultPager};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbDriverKind {
    Postgres,
    MySql,
    Sqlite,
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
            ssh_tunnel: None,
            connect_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone)]
pub enum StreamEvent {
    SchemaReady(Vec<ColumnMeta>),
    PageAppended { total_rows: u64 },
    Completed { total_rows: u64, elapsed_ms: u64 },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct TableSummary {
    pub schema: String,
    pub name: String,
    pub table_type: String, // "BASE TABLE" or "VIEW"
}

#[async_trait]
pub trait DatabaseDriver: Send + Sync {
    /// Test connection to verify credentials and connectivity
    async fn ping(&self) -> Result<()>;

    /// Stream query results straight into QueryResultPager and emit StreamEvents
    async fn execute_streaming(
        &self,
        sql: &str,
        pager: Arc<QueryResultPager>,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()>;

    /// Execute query and return populated QueryResultPager, total rows, and elapsed ms
    async fn execute_query(&self, sql: &str) -> Result<(Arc<QueryResultPager>, u64, u64)>;

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
            if let Ok(cols) = self.list_columns(&t.schema, &t.name).await {
                results.push((t.schema.clone(), t.name.clone(), cols));
            }
        }
        Ok(results)
    }

    /// Batch list all constraints across all tables: (schema, table, constraint_name, constraint_type)
    async fn list_all_constraints(&self) -> Result<Vec<(String, String, String, String)>> {
        Ok(Vec::new())
    }

    /// Batch list all indexes across all tables: (schema, table, index_name)
    async fn list_all_indexes(&self) -> Result<Vec<(String, String, String)>> {
        Ok(Vec::new())
    }
}
