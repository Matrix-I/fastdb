//! Optional process-scoped SQL observation. No query results or bound values are retained.
use chrono::{DateTime, Utc};
use std::sync::{Arc, LazyLock, RwLock, Weak};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct SqlRecord {
    pub started_at: DateTime<Utc>,
    pub provider: &'static str,
    pub session: String,
    pub kind: &'static str,
    pub sql: String,
    pub elapsed_ms: u64,
    pub rows: Option<u64>,
    pub result: String,
}

tokio::task_local! {
    static TASK_SESSION: String;
}

thread_local! {
    static THREAD_SESSION: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Scopes an async execution to a non-secret connection or session identifier.
pub async fn with_session<F: std::future::Future>(session: String, future: F) -> F::Output {
    TASK_SESSION.scope(session, future).await
}

/// Scopes a blocking execution to a non-secret connection or session identifier.
pub fn with_thread_session<T>(session: String, run: impl FnOnce() -> T) -> T {
    struct Guard(Option<String>);
    impl Drop for Guard {
        fn drop(&mut self) {
            let previous = self.0.take();
            THREAD_SESSION.with(|slot| *slot.borrow_mut() = previous);
        }
    }
    let previous = THREAD_SESSION.with(|slot| slot.borrow_mut().replace(session));
    let _guard = Guard(previous);
    run()
}

pub fn current_session(provider: &'static str) -> String {
    TASK_SESSION
        .try_with(Clone::clone)
        .ok()
        .or_else(|| THREAD_SESSION.with(|slot| slot.borrow().clone()))
        .unwrap_or_else(|| provider.to_owned())
}

/// Classifies SQL statements into DBeaver-style transaction log categories.
pub fn classify_sql(sql: &str) -> &'static str {
    let mut rest = sql;
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("--") {
            rest = after.split_once('\n').map_or("", |(_, tail)| tail);
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after.split_once("*/").map_or("", |(_, tail)| tail);
        } else {
            break;
        }
    }
    let keyword: String = rest
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .map(|ch| ch.to_ascii_lowercase())
        .collect();
    match keyword.as_str() {
        "commit" | "end" => "Transaction / Commit",
        "rollback" | "abort" => "Transaction / Rollback",
        "begin" | "start" | "savepoint" | "release" => "Transaction / Begin",
        "create" | "alter" | "drop" | "truncate" | "rename" | "comment" => "SQL / Schema",
        _ => "SQL / User",
    }
}

/// Called on database workers. Implementations must use bounded storage and must
/// not send SQL or parameter values to the application's diagnostic logger.
pub trait SqlObserver: Send + Sync {
    fn record(&self, record: SqlRecord);
}

static OBSERVER: LazyLock<RwLock<Option<Weak<dyn SqlObserver>>>> =
    LazyLock::new(|| RwLock::new(None));

/// Keeps observation active; dropping the registration detaches it before cleanup.
pub struct Registration(Arc<dyn SqlObserver>);

pub fn install(observer: Arc<dyn SqlObserver>) -> anyhow::Result<Registration> {
    let mut slot = OBSERVER.write().unwrap_or_else(|error| error.into_inner());
    anyhow::ensure!(
        slot.as_ref().and_then(Weak::upgrade).is_none(),
        "SQL observer already installed"
    );
    *slot = Some(Arc::downgrade(&observer));
    Ok(Registration(observer))
}

impl Drop for Registration {
    fn drop(&mut self) {
        let mut slot = OBSERVER.write().unwrap_or_else(|error| error.into_inner());
        if slot
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some_and(|current| Arc::ptr_eq(&current, &self.0))
        {
            *slot = None;
        }
    }
}

pub(crate) struct Statement {
    observer: Option<Arc<dyn SqlObserver>>,
    record: Option<SqlRecord>,
    started: Instant,
}

impl Statement {
    pub(crate) fn start(provider: &'static str, sql: &str) -> Self {
        let observer = OBSERVER
            .read()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .and_then(Weak::upgrade);
        let record = observer.as_ref().map(|_| SqlRecord {
            started_at: Utc::now(),
            provider,
            session: current_session(provider),
            kind: classify_sql(sql),
            sql: sql.to_owned(),
            elapsed_ms: 0,
            rows: None,
            result: "Cancelled or interrupted".into(),
        });
        Self {
            observer,
            record,
            started: Instant::now(),
        }
    }
    pub(crate) fn finish<T, E: std::fmt::Display>(
        &mut self,
        result: &Result<T, E>,
        rows: Option<u64>,
    ) {
        if let Some(record) = &mut self.record {
            record.rows = rows;
            record.result = match result {
                Ok(_) => "Success".into(),
                Err(error) => format!("Error: {error}"),
            };
        }
        self.emit();
    }
    fn emit(&mut self) {
        if let (Some(observer), Some(mut record)) = (&self.observer, self.record.take()) {
            record.elapsed_ms = self.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            observer.record(record);
        }
    }
}
impl Drop for Statement {
    fn drop(&mut self) {
        self.emit();
    }
}

// The SQL expression is evaluated once. Bound parameters remain in the native
// call; only its template is copied into the observer.
macro_rules! observe {
    ($provider:expr, $sql:expr, |$query:ident| $body:expr) => {{
        let $query = $sql;
        let mut observation =
            $crate::sql_observer::Statement::start($provider, AsRef::<str>::as_ref(&$query));
        let outcome = $body;
        observation.finish(&outcome, None);
        outcome
    }};
    ($provider:expr, $sql:expr, |$query:ident| $body:expr, |$value:ident| $count:expr) => {{
        let $query = $sql;
        let mut observation =
            $crate::sql_observer::Statement::start($provider, AsRef::<str>::as_ref(&$query));
        let outcome = $body;
        let rows = outcome.as_ref().ok().and_then(|$value| $count);
        observation.finish(&outcome, rows);
        outcome
    }};
}
pub(crate) use observe;

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Collector(std::sync::Mutex<Vec<SqlRecord>>);
    impl SqlObserver for Collector {
        fn record(&self, record: SqlRecord) {
            self.0.lock().unwrap().push(record);
        }
    }
    #[test]
    fn completion_and_drop_emit_once_without_bound_values() {
        let collector = Arc::new(Collector::default());
        let make = || Statement {
            observer: Some(collector.clone()),
            record: Some(SqlRecord {
                started_at: DateTime::from_timestamp(0, 0).unwrap(),
                provider: "test",
                session: "test#1".into(),
                kind: classify_sql("SELECT ?"),
                sql: "SELECT ?".into(),
                elapsed_ms: 0,
                rows: None,
                result: "Cancelled or interrupted".into(),
            }),
            started: Instant::now(),
        };
        {
            let mut statement = make();
            statement.finish(&Ok::<_, String>(()), Some(2));
        }
        {
            let mut statement = make();
            statement.finish(&Err::<(), _>("syntax"), None);
        }
        drop(make());
        let records = collector.0.lock().unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[0].rows, Some(2));
        assert_eq!(records[0].result, "Success");
        assert_eq!(records[0].kind, "SQL / User");
        assert_eq!(records[0].session, "test#1");
        assert_eq!(records[1].result, "Error: syntax");
        assert_eq!(records[2].result, "Cancelled or interrupted");
        assert!(records.iter().all(|record| record.sql == "SELECT ?"));
    }

    #[test]
    fn classifies_statements_and_scopes_session_identity() {
        assert_eq!(
            classify_sql(r#"SELECT cs.* FROM public."componentScore" AS cs"#),
            "SQL / User"
        );
        assert_eq!(
            classify_sql("/* leading comment */\n-- line\nCREATE TABLE items (id INT)"),
            "SQL / Schema"
        );
        assert_eq!(classify_sql("BEGIN DEFERRED"), "Transaction / Begin");
        assert_eq!(classify_sql("COMMIT"), "Transaction / Commit");
        assert_eq!(classify_sql("ROLLBACK TO sp1"), "Transaction / Rollback");
        let scoped = with_thread_session("sqlite-session-7".into(), || current_session("SQLite"));
        assert_eq!(scoped, "sqlite-session-7");
        assert_eq!(current_session("SQLite"), "SQLite");
    }
}
