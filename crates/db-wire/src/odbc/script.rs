//! Private ODBC connections never reconnect after cancellation or connection loss.
use super::*;
use crate::script::ScriptSession;
use std::sync::atomic::AtomicU8;

pub(super) const QUERY_TIMEOUT_SECONDS: usize = 300;
const IDLE: u8 = 0;
const ACTIVE: u8 = 1;
const UNKNOWN: u8 = 2;
const FAILED: u8 = 3;

struct OdbcSession {
    driver: OdbcDriver,
    state: Arc<AtomicU8>,
}

pub(super) async fn open(config: ConnectionConfig) -> Result<Arc<dyn ScriptSession>> {
    let session = tokio::task::spawn_blocking(move || {
        let mut driver = OdbcDriver::new(config);
        driver.dedicated = true;
        let connection = driver.connect()?;
        *driver.connection.lock() = Some(connection);
        Ok::<_, anyhow::Error>(OdbcSession {
            driver,
            state: Arc::new(AtomicU8::new(IDLE)),
        })
    })
    .await
    .context("ODBC session worker failed")??;
    Ok(Arc::new(session))
}

#[derive(PartialEq, Eq)]
enum Boundary {
    Begin,
    Commit,
    Rollback,
}

fn boundary(sql: &str) -> Option<Boundary> {
    let mut rest = without_leading_comments(sql);
    let mut words = [""; 2];
    let mut len = 0;
    while !rest.is_empty() && !rest.starts_with(';') {
        let end = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        if len == words.len() || end == 0 {
            return None;
        }
        words[len] = &rest[..end];
        len += 1;
        rest = without_leading_comments(&rest[end..]);
    }
    if !without_leading_comments(rest.trim_start_matches(';')).is_empty() {
        return None;
    }
    let first = words[0];
    if len == 1 && first.eq_ignore_ascii_case("BEGIN") {
        return Some(Boundary::Begin);
    }
    if len == 2 && !words[1].eq_ignore_ascii_case("WORK") {
        return None;
    }
    if first.eq_ignore_ascii_case("COMMIT") {
        Some(Boundary::Commit)
    } else if first.eq_ignore_ascii_case("ROLLBACK") {
        Some(Boundary::Rollback)
    } else {
        None
    }
}

impl OdbcSession {
    async fn end_connection(&self) -> Result<()> {
        self.driver.closed.store(true, Ordering::Release);
        let connection = self.driver.connection.clone();
        let state = self.state.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(connection) = connection.lock().take()
                && state.load(Ordering::Acquire) != IDLE
            {
                crate::sql_observer::observe!("ODBC", "ROLLBACK", |_query| connection.rollback())
                    .context("ODBC session rollback failed")?;
            }
            Ok(())
        })
        .await
        .context("ODBC cleanup worker failed")?
    }

    async fn apply_boundary(
        &self,
        boundary: Boundary,
        tx: flume::Sender<StreamEvent>,
    ) -> Result<()> {
        let state = self.state.clone();
        self.driver
            .with_connection(move |conn, _| {
                if boundary == Boundary::Begin {
                    if state.load(Ordering::Acquire) != IDLE {
                        bail!("Commit or Rollback before beginning another transaction");
                    }
                    crate::sql_observer::observe!("ODBC", "BEGIN", |_query| conn
                        .set_autocommit(false))?;
                    state.store(ACTIVE, Ordering::Release);
                } else {
                    state.store(UNKNOWN, Ordering::Release);
                    if boundary == Boundary::Commit {
                        crate::sql_observer::observe!("ODBC", "COMMIT", |_query| conn.commit())?;
                    } else {
                        crate::sql_observer::observe!("ODBC", "ROLLBACK", |_query| conn
                            .rollback())?;
                    }
                    conn.set_autocommit(true)?;
                    state.store(IDLE, Ordering::Release);
                }
                let _ = tx.send(StreamEvent::CommandComplete {
                    affected_rows: None,
                });
                let _ = tx.send(StreamEvent::Completed {
                    total_rows: 0,
                    elapsed_ms: 0,
                });
                Ok(())
            })
            .await
    }

    async fn probe_sql_server(&self) -> Result<()> {
        let state = self.state.clone();
        self.driver
            .with_connection(move |conn, _| {
                let mut cursor =
                    crate::sql_observer::observe!("ODBC", "SELECT XACT_STATE()", |query| conn
                        .execute(query, (), Some(5)))?
                    .context("SQL Server did not return transaction state")?;
                let mut value = Vec::new();
                let mut row = cursor
                    .next_row()?
                    .context("SQL Server returned no transaction state")?;
                let present = row.get_text(1, &mut value)?;
                let new_state = match (present, value.as_slice()) {
                    (true, b"0") => IDLE,
                    (true, b"1") => ACTIVE,
                    (true, b"-1") => FAILED,
                    _ => bail!("SQL Server returned an unexpected transaction state"),
                };
                state.store(new_state, Ordering::Release);
                Ok(())
            })
            .await
    }
}

#[async_trait]
impl ScriptSession for OdbcSession {
    fn is_closed(&self) -> bool {
        self.driver.closed.load(Ordering::Acquire)
    }

    fn in_transaction(&self) -> Option<bool> {
        match self.state.load(Ordering::Acquire) {
            IDLE => Some(false),
            ACTIVE | FAILED => Some(true),
            _ => None,
        }
    }

    fn transaction_failed(&self) -> bool {
        self.state.load(Ordering::Acquire) == FAILED
    }

    async fn execute_streaming(&self, sql: &str, tx: flume::Sender<StreamEvent>) -> Result<()> {
        let kind = self.driver.config.driver;
        if kind != DbDriverKind::SqlServer
            && let Some(boundary) = boundary(sql)
        {
            return self.apply_boundary(boundary, tx).await;
        }
        let sql = if kind == DbDriverKind::SqlServer && boundary(sql) == Some(Boundary::Begin) {
            if self.in_transaction() != Some(false) {
                bail!("Commit or Rollback before beginning another transaction");
            }
            // RAISERROR leaves pending work alone even if the user enabled XACT_ABORT.
            "IF (2 & @@OPTIONS) = 2 RAISERROR ('Implicit transactions are on. Run SQL to start the transaction.', 16, 1); ELSE BEGIN TRANSACTION;"
        } else {
            sql
        };
        if kind == DbDriverKind::SqlServer {
            self.state.store(UNKNOWN, Ordering::Release);
        }
        let result = self.driver.execute_streaming(sql, tx).await;
        if kind == DbDriverKind::SqlServer && !self.driver.cancelled.load(Ordering::Acquire) {
            self.state.store(UNKNOWN, Ordering::Release);
            let probe = self.probe_sql_server().await;
            if let Err(error) = probe {
                self.driver.closed.store(true, Ordering::Release);
                return result.and(Err(error.context("Cannot confirm SQL Server session state")));
            }
        }
        result
    }

    async fn cancel_active_query(&self) -> Result<()> {
        self.driver.closed.store(true, Ordering::Release);
        self.driver.cancel_active_query().await
    }

    async fn finish(&self) -> Result<()> {
        self.end_connection().await
    }

    fn abort(&self) {
        if !self.driver.closed.swap(true, Ordering::AcqRel) {
            let driver = self.driver.clone();
            let state = self.state.clone();
            crate::get_runtime().spawn(async move {
                if let Err(error) = driver.cancel_active_query().await {
                    tracing::warn!("ODBC session cancellation failed: {error}");
                }
                if let Err(error) = (OdbcSession { driver, state }).end_connection().await {
                    tracing::warn!("ODBC session cleanup failed: {error}");
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Boundary, boundary};

    #[test]
    fn transaction_boundaries_preserve_savepoints_and_procedural_sql() {
        assert!(boundary("-- note\n/* block */ ROLLBACK WORK;") == Some(Boundary::Rollback));
        assert!(boundary("/* note */ BEGIN;") == Some(Boundary::Begin));
        assert!(boundary("COMMIT;") == Some(Boundary::Commit));
        assert!(boundary("COMMIT /* note */;") == Some(Boundary::Commit));
        assert!(boundary("COMMIT\nWORK;") == Some(Boundary::Commit));
        assert!(boundary("ROLLBACK -- note") == Some(Boundary::Rollback));
        assert!(boundary("ROLLBACK; SELECT 1").is_none());
        assert!(boundary("ROLLBACK TO SAVEPOINT kept;").is_none());
        assert!(boundary("BEGIN NULL; END;").is_none());
    }
}
