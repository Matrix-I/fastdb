use std::sync::Arc;
use anyhow::Result;

pub mod traits;
pub mod ssh_tunnel;
pub mod export;
pub mod postgres;
pub mod mysql;
pub mod sqlite;

pub use traits::*;
pub use ssh_tunnel::*;
pub use export::*;
pub use postgres::*;
pub use mysql::*;
pub use sqlite::*;

pub fn get_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("fastdb-worker")
            .build()
            .expect("Failed to initialize FastDB background tokio runtime")
    })
}

pub fn create_driver(config: ConnectionConfig) -> Result<Arc<dyn DatabaseDriver>> {
    match config.driver {
        DbDriverKind::Postgres => Ok(Arc::new(PostgresDriver::new(config))),
        DbDriverKind::MySql => Ok(Arc::new(MySqlDriver::new(config))),
        DbDriverKind::Sqlite => Ok(Arc::new(SqliteDriver::new(config)?)),
    }
}
