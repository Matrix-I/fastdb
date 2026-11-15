//! Scoped activity and administrative signals on one physical PostgreSQL server.

use super::{PostgresDriver, SocketGuard, pg_error_message};
use crate::{ConnectionConfig, DbDriverKind};
use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, Utc};
use std::time::Duration;
use tokio_postgres::{Client, Row};

const SNAPSHOT_SQL: &str = include_str!("session_snapshot.sql");
const SIGNAL_SQL: &str = include_str!("session_signal.sql");
const SESSION_LIMIT: usize = 256;
const BLOCKERS_PER_SESSION: usize = 64;
const QUERY_TIMEOUT: Duration = Duration::from_secs(6);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionIdentity {
    pub pid: i32,
    pub backend_start: DateTime<Utc>,
    pub query_start: Option<DateTime<Utc>>,
}

pub struct SessionActivity {
    pub pid: i32,
    pub database: Option<String>,
    pub role: Option<String>,
    pub application: String,
    pub state: Option<String>,
    pub wait_type: Option<String>,
    pub wait_event: Option<String>,
    pub identity: Option<SessionIdentity>,
    pub active_query: Option<String>,
    pub active_ms: Option<i64>,
    pub blockers: Vec<i32>,
    pub blockers_truncated: bool,
}

#[derive(Default)]
pub struct SessionSnapshot {
    pub rows: Vec<SessionActivity>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionSignal {
    Cancel,
    Terminate,
}

pub struct SessionMonitor {
    client: Client,
    _socket: SocketGuard,
    read_only: bool,
}

impl SessionMonitor {
    pub async fn connect(config: ConnectionConfig) -> Result<Self> {
        ensure!(
            config.driver == DbDriverKind::Postgres,
            "Session Monitor requires PostgreSQL"
        );
        let read_only = config.read_only;
        let (client, task) = PostgresDriver::connect_with_task(config).await?;
        let socket = SocketGuard(task);
        tokio::time::timeout(
            QUERY_TIMEOUT,
            client.batch_execute(
                "SET search_path = pg_catalog; SET statement_timeout = '5s'; SET default_transaction_read_only = on",
            ),
        )
        .await
        .context("Session Monitor setup timed out")?
        .map_err(server_error)?;
        Ok(Self {
            client,
            _socket: socket,
            read_only,
        })
    }

    pub async fn snapshot(&self) -> Result<SessionSnapshot> {
        let limit = (SESSION_LIMIT + 1) as i64;
        let blocker_limit = (BLOCKERS_PER_SESSION + 1) as i32;
        let rows = tokio::time::timeout(
            QUERY_TIMEOUT,
            self.client.query(SNAPSHOT_SQL, &[&limit, &blocker_limit]),
        )
        .await
        .context("Session refresh timed out")?
        .map_err(server_error)?;
        let truncated = rows.len() > SESSION_LIMIT;
        let rows = rows
            .into_iter()
            .take(SESSION_LIMIT)
            .map(activity)
            .collect::<Result<_>>()?;
        Ok(SessionSnapshot { rows, truncated })
    }

    pub async fn signal(&self, target: &SessionIdentity, action: SessionSignal) -> Result<()> {
        ensure!(
            !self.read_only,
            "Session actions are disabled for read-only connections"
        );
        ensure!(target.pid > 0, "Invalid session PID");
        let cancel = action == SessionSignal::Cancel;
        ensure!(
            !cancel || target.query_start.is_some(),
            "There is no visible active query to cancel"
        );
        // The server accepts only a PID. Keep the snapshot identity guard and signaling
        // in one statement to minimize the gap between checking and acting.
        let params: &[&(dyn tokio_postgres::types::ToSql + Sync)] = &[
            &target.pid,
            &target.backend_start,
            &cancel,
            &target.query_start,
        ];
        let row = tokio::time::timeout(QUERY_TIMEOUT, self.client.query_opt(SIGNAL_SQL, params))
            .await
            .context("Session action timed out. Its outcome is unknown")?
            .map_err(server_error)?;
        match row {
            Some(row) if row.try_get::<_, bool>(0)? => Ok(()),
            Some(_) => bail!("PostgreSQL did not send the session signal"),
            None => bail!("The selected session or query changed or is no longer visible"),
        }
    }
}

fn activity(row: Row) -> Result<SessionActivity> {
    let pid = row.try_get("pid")?;
    let backend_start: Option<DateTime<Utc>> = row.try_get("backend_start")?;
    let query_start = row.try_get("active_since")?;
    let mut blockers: Vec<i32> = row.try_get("blockers")?;
    // Parallel workers may report the same client PID more than once.
    let blockers_truncated = blockers.len() > BLOCKERS_PER_SESSION;
    blockers.truncate(BLOCKERS_PER_SESSION);
    blockers.sort_unstable();
    blockers.dedup();
    Ok(SessionActivity {
        pid,
        database: row.try_get("datname")?,
        role: row.try_get("usename")?,
        application: row
            .try_get::<_, Option<String>>("application_name")?
            .unwrap_or_default(),
        state: row.try_get("state")?,
        wait_type: row.try_get("wait_event_type")?,
        wait_event: row.try_get("wait_event")?,
        identity: backend_start.map(|backend_start| SessionIdentity {
            pid,
            backend_start,
            query_start,
        }),
        active_query: row.try_get("active_query")?,
        active_ms: row.try_get("elapsed_ms")?,
        blockers,
        blockers_truncated,
    })
}

fn server_error(error: tokio_postgres::Error) -> anyhow::Error {
    let message = pg_error_message(&error);
    anyhow::Error::new(error).context(message)
}
