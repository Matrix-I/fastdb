//! SQL Server, Oracle and Db2 through installed vendor ODBC drivers.
//! The driver manager is bundled; vendor clients are discovered at runtime.
use crate::stream::{RowSink, report_error};
use crate::{ConnectionConfig, DatabaseDriver, DbDriverKind, StreamEvent, TableSummary};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use mem_engine::{ColumnMeta, DataType};
use odbc_api::handles::{AsStatementRef, Statement};
use odbc_api::{Connection, ConnectionOptions, Cursor, Environment, ResultSetMetadata};
use parking_lot::Mutex;
use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::Instant;

fn environment() -> Result<&'static Environment> {
    static ENV: OnceLock<Result<Environment, String>> = OnceLock::new();
    ENV.get_or_init(|| Environment::new().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| anyhow::anyhow!("ODBC initialization failed: {e}"))
}

/// Brace every value, including credentials, to prevent connection-string injection.
fn value(input: &str) -> String {
    format!("{{{}}}", input.replace('}', "}}"))
}
// Oracle and Db2 treat braces as literal field text rather than unescaping them.
// Reject delimiters instead of emitting a string that can change options.
fn unquoted_value(input: &str) -> Result<&str> {
    if input.contains([';', '\0', '\r', '\n']) {
        bail!(
            "ODBC connection fields for Oracle/Db2 cannot contain semicolons or control characters"
        );
    }
    Ok(input)
}

fn literal(input: &str) -> String {
    format!("'{}'", input.replace('\'', "''"))
}

fn driver_matches(kind: DbDriverKind, name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    match kind {
        DbDriverKind::SqlServer => {
            name.contains("sql server")
                && name
                    .split_whitespace()
                    .filter_map(|p| p.parse::<u32>().ok())
                    .any(|version| version >= 17)
        }
        DbDriverKind::Oracle => name.contains("oracle"),
        DbDriverKind::Db2 => name.contains("db2") || name.contains("ibm data server"),
        _ => false,
    }
}

fn connection_string(config: &ConnectionConfig, driver: &str) -> Result<String> {
    let common = if matches!(config.driver, DbDriverKind::Oracle | DbDriverKind::Db2) {
        format!(
            "DRIVER={};UID={};PWD={};",
            value(driver),
            unquoted_value(&config.user)?,
            unquoted_value(config.password.as_deref().unwrap_or(""))?
        )
    } else {
        format!(
            "DRIVER={};UID={};PWD={};",
            value(driver),
            value(&config.user),
            value(config.password.as_deref().unwrap_or(""))
        )
    };
    let database = value(&config.database);
    // TLS parameters differ between vendor clients; never silently ignore a supplied CA.
    if config.tls_enabled && config.tls_ca_cert.is_some() && config.driver != DbDriverKind::Db2 {
        bail!(
            "This ODBC provider uses the system/vendor trust store. Install the CA there and leave Root CA empty."
        );
    }
    let endpoint = match config.driver {
        DbDriverKind::SqlServer => format!(
            "SERVER={};DATABASE={database};Encrypt={};TrustServerCertificate=no;",
            value(&format!("{},{}", config.host, config.port)),
            if config.tls_enabled { "yes" } else { "no" }
        ),
        DbDriverKind::Oracle => {
            // Oracle Easy Connect Plus: certificate identity is always verified for TCPS.
            let protocol = if config.tls_enabled { "tcps" } else { "tcp" };
            if config.host.contains(['?', '&', '/']) || config.database.contains(['?', '&']) {
                bail!("Oracle host/service must not contain Easy Connect query parameters");
            }
            let endpoint = format!(
                "{protocol}://{}:{}/{}?ssl_server_dn_match=yes",
                unquoted_value(&config.host)?,
                config.port,
                unquoted_value(&config.database)?
            );
            format!("DBQ={};", unquoted_value(&endpoint)?)
        }
        DbDriverKind::Db2 => {
            let mut endpoint = format!(
                "HOSTNAME={};PORT={};DATABASE={};PROTOCOL=TCPIP;",
                unquoted_value(&config.host)?,
                config.port,
                unquoted_value(&config.database)?
            );
            if config.tls_enabled {
                endpoint.push_str("SECURITY=SSL;SSLClientHostnameValidation=Basic;");
                if let Some(ca) = &config.tls_ca_cert {
                    endpoint.push_str(&format!("SSLServerCertificate={};", unquoted_value(ca)?));
                }
            }
            endpoint
        }
        _ => bail!("Not an ODBC provider"),
    };
    Ok(common + &endpoint)
}

#[derive(Clone)]
pub struct OdbcDriver {
    config: ConnectionConfig,
    connection: Arc<Mutex<Option<Connection<'static>>>>,
    active_statement: Arc<Mutex<Option<usize>>>,
    cancelled: Arc<AtomicBool>,
}

// Declared after the statement so it clears the handle before the statement is freed,
// including on errors. SQLCancel holds this same mutex for the entire FFI call.
struct ActiveStatement(Arc<Mutex<Option<usize>>>);
impl Drop for ActiveStatement {
    fn drop(&mut self) {
        *self.0.lock() = None;
    }
}

impl OdbcDriver {
    pub fn new(config: ConnectionConfig) -> Self {
        Self {
            config,
            connection: Arc::new(Mutex::new(None)),
            active_statement: Arc::new(Mutex::new(None)),
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn connect(&self) -> Result<Connection<'static>> {
        let env = environment()?;
        let mut drivers: Vec<_> = env
            .drivers()?
            .into_iter()
            .filter(|d| driver_matches(self.config.driver, &d.description))
            .collect();
        drivers.sort_by_key(|d| {
            std::cmp::Reverse((
                d.description
                    .split_whitespace()
                    .find_map(|p| p.parse::<u32>().ok())
                    .unwrap_or(0),
                d.description.clone(),
            ))
        });
        let driver = drivers.first().with_context(|| format!("No {:?} ODBC driver installed. Install and register the vendor ODBC driver in odbcinst.ini (see docs/PROVIDERS.md).", self.config.driver))?;
        let text = connection_string(&self.config, &driver.description)?;
        let connection = env
            .connect_with_connection_string(
                &text,
                ConnectionOptions {
                    login_timeout_sec: Some(
                        self.config
                            .connect_timeout
                            .as_secs()
                            .clamp(1, u32::MAX as u64) as u32,
                    ),
                    ..ConnectionOptions::default()
                },
            )
            .context("ODBC connection failed")?;
        if self.config.driver == DbDriverKind::SqlServer {
            drop(connection.execute("SET QUOTED_IDENTIFIER ON", (), None)?);
        }
        Ok(connection)
    }

    async fn with_connection<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection<'static>, &Self) -> Result<T> + Send + 'static,
    {
        let this = self.clone();
        crate::run_on_shared_runtime(async move {
            tokio::task::spawn_blocking(move || {
                let mut connection = this.connection.lock();
                if connection.is_none() {
                    *connection = Some(this.connect()?);
                }
                operation(connection.as_ref().unwrap(), &this)
            })
            .await
            .context("ODBC worker failed")?
        })
        .await
    }

    async fn metadata(&self, sql: String) -> Result<Vec<Vec<Option<String>>>> {
        self.with_connection(move |conn, _| {
            let mut rows = Vec::new();
            if let Some(mut cursor) = conn.execute(&sql, (), None)? {
                let count = cursor.num_result_cols()? as u16;
                while let Some(mut row) = cursor.next_row()? {
                    let mut values = Vec::new();
                    for col in 1..=count {
                        let mut text = Vec::new();
                        values.push(if row.get_wide_text(col, &mut text)? {
                            Some(String::from_utf16(&text)?)
                        } else {
                            None
                        });
                    }
                    rows.push(values);
                }
            }
            Ok(rows)
        })
        .await
    }
}

/// Oracle SQL does not accept SQL*Plus semicolons; PL/SQL blocks require theirs.
fn executable_sql(kind: DbDriverKind, sql: &str) -> String {
    let sql = sql.trim();
    if kind == DbDriverKind::Db2 {
        return sql.trim_end_matches(';').trim_end().to_owned();
    }
    if kind != DbDriverKind::Oracle {
        return sql.to_owned();
    }
    let upper = sql.to_ascii_uppercase();
    let block = upper.starts_with("BEGIN")
        || upper.starts_with("DECLARE")
        || (upper.starts_with("CREATE")
            && ["PROCEDURE", "FUNCTION", "PACKAGE", "TRIGGER", "TYPE"]
                .iter()
                .any(|word| upper.split_whitespace().take(5).any(|part| part == *word)));
    if block {
        sql.strip_suffix("\n/").unwrap_or(sql).trim_end().to_owned()
    } else {
        sql.trim_end_matches(';').trim_end().to_owned()
    }
}

fn metadata_column(row: &[Option<String>]) -> ColumnMeta {
    let mut col = ColumnMeta::new(
        row[0].clone().unwrap_or_default().trim().to_owned(),
        DataType::Text,
    )
    .with_raw_type(row[1].clone().unwrap_or_default().trim().to_owned())
    .with_nullable(matches!(
        row[2].as_deref().map(str::trim),
        Some("Y" | "YES")
    ));
    if let Some(default) = &row[3] {
        col = col.with_default(default.clone());
    }
    col
}

fn engine_type(kind: odbc_api::DataType) -> DataType {
    use odbc_api::DataType as O;
    match kind {
        O::SmallInt | O::TinyInt => DataType::Int2,
        O::Integer => DataType::Int4,
        O::BigInt => DataType::Int8,
        O::Numeric { .. } | O::Decimal { .. } => DataType::Numeric,
        O::Real => DataType::Float4,
        O::Float { .. } | O::Double => DataType::Float8,
        O::Bit => DataType::Bool,
        O::Date => DataType::Date,
        O::Time { .. } => DataType::Time,
        O::Timestamp { .. } => DataType::Timestamp,
        O::Binary { .. } | O::Varbinary { .. } | O::LongVarbinary { .. } => DataType::Bytea,
        _ => DataType::Text,
    }
}

#[async_trait]
impl DatabaseDriver for OdbcDriver {
    async fn ping(&self) -> Result<()> {
        let sql = match self.config.driver {
            DbDriverKind::Oracle => "SELECT 1 FROM DUAL",
            DbDriverKind::Db2 => "SELECT 1 FROM SYSIBM.SYSDUMMY1",
            _ => "SELECT 1",
        };
        self.metadata(sql.into()).await?;
        Ok(())
    }

    async fn execute_schema_ddl(&self, sql: &str) -> Result<()> {
        let sql = executable_sql(self.config.driver, sql);
        self.with_connection(move |conn, _| {
            let mut statement = conn.preallocate()?;
            statement.execute(&sql, ())?;
            Ok(())
        })
        .await
    }

    async fn execute_streaming(
        &self,
        sql: &str,
        event_tx: flume::Sender<StreamEvent>,
    ) -> Result<()> {
        self.execute_streaming_controlled(sql, event_tx, Arc::default())
            .await
    }

    async fn execute_streaming_controlled(
        &self,
        sql: &str,
        event_tx: flume::Sender<StreamEvent>,
        control: Arc<crate::execution::ExecutionControl>,
    ) -> Result<()> {
        control.check()?;
        let sql = executable_sql(self.config.driver, sql);
        let errors = event_tx.clone();
        self.cancelled.store(false, Ordering::SeqCst);
        let result = self
            .with_connection(move |conn, this| {
                let started = Instant::now();
                let mut statement = conn.preallocate()?;
                *this.active_statement.lock() = Some(statement.as_stmt_ref().as_sys().0 as usize);
                let _active = ActiveStatement(this.active_statement.clone());
                if this.cancelled.load(Ordering::SeqCst) {
                    bail!("Query cancelled");
                }
                control.check()?;
                let no_rows = match statement.execute(&sql, ())? {
                    Some(mut cursor) => {
                        let count = cursor.num_result_cols()? as u16;
                        let mut columns = Vec::new();
                        let mut binary = Vec::new();
                        for col in 1..=count {
                            let mut description = odbc_api::ColumnDescription::default();
                            cursor.describe_col(col, &mut description)?;
                            let kind = engine_type(description.data_type);
                            binary.push(kind == DataType::Bytea);
                            columns.push(
                                ColumnMeta::new(description.name_to_string()?, kind)
                                    .with_nullable(description.could_be_nullable()),
                            );
                        }
                        let mut sink = RowSink::start(columns, event_tx.clone(), started)?;
                        while let Some(mut row) = cursor.next_row()? {
                            if this.cancelled.load(Ordering::SeqCst) {
                                bail!("Query cancelled");
                            }
                            for col in 1..=count {
                                sink.cells_mut()[(col - 1) as usize] = if binary[(col - 1) as usize]
                                {
                                    let mut bytes = Vec::new();
                                    if row.get_binary(col, &mut bytes)? {
                                        Some(
                                            format!(
                                                "\\x{}",
                                                bytes
                                                    .iter()
                                                    .map(|b| format!("{b:02x}"))
                                                    .collect::<String>()
                                            )
                                            .into_bytes(),
                                        )
                                    } else {
                                        None
                                    }
                                } else {
                                    let mut text = Vec::new();
                                    if row.get_wide_text(col, &mut text)? {
                                        Some(String::from_utf16(&text)?.into_bytes())
                                    } else {
                                        None
                                    }
                                };
                            }
                            sink.commit_row()?;
                        }
                        // Drain subsequent results so session state and late SQL errors are preserved.
                        while let Some(mut next) = cursor.more_results()? {
                            while next.next_row()?.is_some() {
                                if this.cancelled.load(Ordering::SeqCst) {
                                    bail!("Query cancelled");
                                }
                            }
                            cursor = next;
                        }
                        if this.cancelled.load(Ordering::SeqCst) {
                            bail!("Query cancelled");
                        }
                        sink.finish();
                        false
                    }
                    None => true,
                };
                if no_rows {
                    let affected_rows = statement.row_count()?.map(|n| n as u64);
                    let _ = event_tx.send(StreamEvent::CommandComplete { affected_rows });
                    RowSink::start(Vec::new(), event_tx, started)?.finish();
                }
                Ok(())
            })
            .await;
        result.map_err(|e| report_error(&errors, format!("{e:#}")))
    }

    async fn apply_row_changes(&self, statements: &[String]) -> Result<u64> {
        let statements = statements.to_vec();
        let this_kind = self.config.driver;
        self.with_connection(move |conn, _| {
            conn.set_autocommit(false)?;
            let result = (|| -> Result<u64> {
                for sql in &statements {
                    let mut statement = conn.preallocate()?;
                    drop(statement.execute(&executable_sql(this_kind, sql), ())?);
                    let rows = statement.row_count()?;
                    if rows != Some(1) { bail!("Edit affected {rows:?} rows; expected exactly one. Transaction rolled back."); }
                }
                conn.commit()?;
                Ok(statements.len() as u64)
            })();
            if result.is_err() { conn.rollback().context("Failed to roll back row edits")?; }
            conn.set_autocommit(true)?;
            result
        }).await
    }

    async fn cancel_active_query(&self) -> Result<()> {
        self.cancelled.store(true, Ordering::SeqCst);
        let active = self.active_statement.clone();
        crate::run_on_shared_runtime(async move {
            tokio::task::spawn_blocking(move || {
                let guard = active.lock();
                if let Some(handle) = *guard {
                    // SAFETY: ActiveStatement keeps the allocated statement alive until this lock
                    // is released. ODBC explicitly permits SQLCancel from a different thread.
                    let result = unsafe {
                        odbc_api::sys::SQLCancel(odbc_api::sys::HStmt(
                            handle as *mut std::ffi::c_void,
                        ))
                    };
                    if result != odbc_api::sys::SqlReturn::SUCCESS
                        && result != odbc_api::sys::SqlReturn::SUCCESS_WITH_INFO
                    {
                        bail!("ODBC driver could not cancel the active statement ({result:?})");
                    }
                }
                Ok(())
            })
            .await?
        })
        .await
    }

    async fn list_tables(&self) -> Result<Vec<TableSummary>> {
        let sql = match self.config.driver {
            DbDriverKind::Oracle => {
                "SELECT USER, TABLE_NAME, 'BASE TABLE' FROM USER_TABLES UNION ALL SELECT USER, VIEW_NAME, 'VIEW' FROM USER_VIEWS"
            }
            DbDriverKind::Db2 => {
                "SELECT TABSCHEMA, TABNAME, CASE TYPE WHEN 'V' THEN 'VIEW' ELSE 'BASE TABLE' END FROM SYSCAT.TABLES WHERE TYPE IN ('T','V') AND TABSCHEMA NOT LIKE 'SYS%'"
            }
            _ => {
                "SELECT TABLE_SCHEMA, TABLE_NAME, TABLE_TYPE FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA NOT IN ('sys','INFORMATION_SCHEMA')"
            }
        };
        Ok(self
            .metadata(sql.into())
            .await?
            .into_iter()
            .map(|r| TableSummary {
                schema: r[0].clone().unwrap_or_default().trim().to_string(),
                name: r[1].clone().unwrap_or_default().trim().to_string(),
                table_type: r[2].clone().unwrap_or_default(),
                estimated_rows: None,
            })
            .collect())
    }

    async fn list_all_columns(&self) -> Result<Vec<(String, String, Vec<ColumnMeta>)>> {
        let sql = match self.config.driver {
            DbDriverKind::Oracle => {
                "SELECT OWNER, TABLE_NAME, COLUMN_NAME, DATA_TYPE, NULLABLE, DATA_DEFAULT FROM ALL_TAB_COLUMNS WHERE OWNER=USER ORDER BY TABLE_NAME, COLUMN_ID"
            }
            DbDriverKind::Db2 => {
                "SELECT TABSCHEMA, TABNAME, COLNAME, TYPENAME, NULLS, DEFAULT FROM SYSCAT.COLUMNS WHERE TABSCHEMA NOT LIKE 'SYS%' ORDER BY TABSCHEMA, TABNAME, COLNO"
            }
            _ => {
                "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, DATA_TYPE, IS_NULLABLE, COLUMN_DEFAULT FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA NOT IN ('sys','INFORMATION_SCHEMA') ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION"
            }
        };
        let mut tables = std::collections::BTreeMap::<(String, String), Vec<ColumnMeta>>::new();
        for row in self.metadata(sql.into()).await? {
            let column = metadata_column(&row[2..]);
            tables
                .entry((
                    row[0].clone().unwrap_or_default().trim().to_owned(),
                    row[1].clone().unwrap_or_default().trim().to_owned(),
                ))
                .or_default()
                .push(column);
        }
        Ok(tables
            .into_iter()
            .map(|((schema, table), cols)| (schema, table, cols))
            .collect())
    }

    async fn list_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnMeta>> {
        let (schema, table) = (literal(schema), literal(table));
        let sql = match self.config.driver {
            DbDriverKind::Oracle => format!(
                "SELECT COLUMN_NAME, DATA_TYPE, NULLABLE, DATA_DEFAULT FROM ALL_TAB_COLUMNS WHERE OWNER={schema} AND TABLE_NAME={table} ORDER BY COLUMN_ID"
            ),
            DbDriverKind::Db2 => format!(
                "SELECT COLNAME, TYPENAME, NULLS, DEFAULT FROM SYSCAT.COLUMNS WHERE TABSCHEMA={schema} AND TABNAME={table} ORDER BY COLNO"
            ),
            _ => format!(
                "SELECT COLUMN_NAME, DATA_TYPE, IS_NULLABLE, COLUMN_DEFAULT FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_SCHEMA={schema} AND TABLE_NAME={table} ORDER BY ORDINAL_POSITION"
            ),
        };
        Ok(self
            .metadata(sql)
            .await?
            .iter()
            .map(|r| metadata_column(r))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oracle_connection_fields_are_not_brace_quoted() {
        let mut config = ConnectionConfig {
            driver: DbDriverKind::Oracle,
            user: "app".into(),
            password: Some("p}word".into()),
            database: "FREEPDB1".into(),
            ..ConnectionConfig::default()
        };
        let text = connection_string(&config, "Oracle Instant Client ODBC").unwrap();
        assert!(text.contains("UID=app;PWD=p}word;DBQ=tcp://"));
        config.password = Some("p;UID=other".into());
        assert!(connection_string(&config, "Oracle Instant Client ODBC").is_err());
    }

    #[test]
    fn oracle_sql_and_plsql_have_different_terminators() {
        assert_eq!(
            executable_sql(DbDriverKind::Oracle, "SELECT 1 FROM DUAL; "),
            "SELECT 1 FROM DUAL"
        );
        assert_eq!(
            executable_sql(DbDriverKind::Oracle, "BEGIN NULL; END;\n/"),
            "BEGIN NULL; END;"
        );
        assert_eq!(
            executable_sql(
                DbDriverKind::Oracle,
                "CREATE OR REPLACE PROCEDURE p AS BEGIN NULL; END;"
            ),
            "CREATE OR REPLACE PROCEDURE p AS BEGIN NULL; END;"
        );
        assert_eq!(executable_sql(DbDriverKind::Db2, "VALUES 1;"), "VALUES 1");
        assert_eq!(
            executable_sql(DbDriverKind::SqlServer, "SELECT 1;"),
            "SELECT 1;"
        );
    }

    #[test]
    fn db2_tls_verifies_hostname_and_rejects_field_delimiters() {
        let config = ConnectionConfig {
            driver: DbDriverKind::Db2,
            user: "fastdb".into(),
            tls_enabled: true,
            tls_ca_cert: Some("/ca.pem".into()),
            ..ConnectionConfig::default()
        };
        let text = connection_string(&config, "IBM DB2 ODBC DRIVER").unwrap();
        assert!(text.contains("SECURITY=SSL;SSLClientHostnameValidation=Basic;"));
        assert!(text.contains("SSLServerCertificate=/ca.pem;"));
        assert!(text.contains("HOSTNAME=localhost;"));
        assert!(text.contains("UID=fastdb;PWD=;"));
        let mut invalid = config;
        invalid.tls_ca_cert = Some("/ca;name.pem".into());
        assert!(connection_string(&invalid, "IBM DB2 ODBC DRIVER").is_err());
        invalid.tls_ca_cert = None;
        invalid.password = Some("p;UID=other".into());
        assert!(connection_string(&invalid, "IBM DB2 ODBC DRIVER").is_err());
    }

    #[test]
    fn unsupported_custom_ca_is_reported_instead_of_ignored() {
        assert!(!driver_matches(
            DbDriverKind::SqlServer,
            "ODBC Driver 13 for SQL Server"
        ));
        for kind in [DbDriverKind::Oracle, DbDriverKind::SqlServer] {
            let config = ConnectionConfig {
                driver: kind,
                tls_enabled: true,
                tls_ca_cert: Some("ca.pem".into()),
                ..ConnectionConfig::default()
            };
            assert!(connection_string(&config, "driver").is_err());
        }
    }

    #[test]
    fn missing_vendor_driver_emits_an_error_event() {
        let env = environment().unwrap();
        let installed = env.drivers().unwrap();
        let Some(kind) = [
            DbDriverKind::SqlServer,
            DbDriverKind::Oracle,
            DbDriverKind::Db2,
        ]
        .into_iter()
        .find(|kind| {
            !installed
                .iter()
                .any(|driver| driver_matches(*kind, &driver.description))
        }) else {
            return;
        };
        let driver = OdbcDriver::new(ConnectionConfig {
            driver: kind,
            ..ConnectionConfig::default()
        });
        let (tx, rx) = flume::unbounded();
        let error = crate::get_runtime()
            .block_on(driver.execute_streaming("SELECT 1", tx))
            .unwrap_err();
        assert!(error.to_string().contains("ODBC driver installed"));
        assert!(matches!(rx.recv().unwrap(), StreamEvent::Error(_)));
    }

    #[test]
    fn connection_values_cannot_inject_odbc_options() {
        let config = ConnectionConfig {
            driver: DbDriverKind::SqlServer,
            user: "u;Encrypt=no".into(),
            password: Some("p};TrustServerCertificate=yes".into()),
            tls_enabled: true,
            ..ConnectionConfig::default()
        };
        let text = connection_string(&config, "ODBC Driver 18 for SQL Server").unwrap();
        assert!(text.contains("UID={u;Encrypt=no};"));
        assert!(text.contains("PWD={p}};TrustServerCertificate=yes};"));
        assert!(text.ends_with("Encrypt=yes;TrustServerCertificate=no;"));
    }
    #[test]
    fn provider_endpoints_and_missing_drivers_are_distinct() {
        for (kind, driver, key) in [
            (
                DbDriverKind::SqlServer,
                "ODBC Driver 18 for SQL Server",
                "SERVER=",
            ),
            (DbDriverKind::Oracle, "Oracle 23 ODBC driver", "DBQ="),
            (DbDriverKind::Db2, "IBM DB2 ODBC DRIVER", "HOSTNAME="),
        ] {
            assert!(driver_matches(kind, driver));
            let config = ConnectionConfig {
                driver: kind,
                database: "app".into(),
                ..ConnectionConfig::default()
            };
            assert!(connection_string(&config, driver).unwrap().contains(key));
            assert!(!driver_matches(kind, "SQLite3"));
        }
    }
}
