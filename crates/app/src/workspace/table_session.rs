//! Checked table writes on the physical session owned by a table tab.
use db_wire::script::{ScriptControl, ScriptEvent, TabSession, run_tab_script};
use db_wire::{DatabaseDriver, DbDriverKind, QueryResultPager, StreamEvent};
use std::sync::{Arc, Weak};
use std::time::Duration;

const STATEMENT_TIMEOUT: Duration = Duration::from_secs(15);
const OPEN_AND_STATEMENT_TIMEOUT: Duration = Duration::from_secs(45);
const EVENT_CAPACITY: usize = 64;
const SAVEPOINT: &str = "fastdb_row_batch";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Boundary {
    Save,
    Commit,
    Rollback,
}

#[derive(Default)]
pub(super) struct Statement {
    pub affected: Option<u64>,
    pub active: Option<bool>,
    pub pager: Option<Arc<QueryResultPager>>,
}

impl std::fmt::Debug for Statement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Statement")
            .field("affected", &self.affected)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

pub(super) struct Outcome {
    pub state: String,
    pub error: Option<String>,
}

impl Outcome {
    fn success(active: bool) -> Self {
        Self {
            state: if active { "In transaction" } else { "Idle" }.into(),
            error: None,
        }
    }
    pub(super) fn failed(state: &str, error: String) -> Self {
        Self {
            state: state.into(),
            error: Some(error),
        }
    }
}

pub(super) async fn statement(
    driver: Arc<dyn DatabaseDriver>,
    owner: Weak<TabSession>,
    sql: String,
) -> Result<Statement, String> {
    let (tx, rx) = flume::bounded(EVENT_CAPACITY);
    let control = Arc::new(ScriptControl::with_timeout(Some(STATEMENT_TIMEOUT)));
    let run = run_tab_script(driver, vec![sql], control, tx, owner.clone());
    let collect = async move {
        let mut result = Statement::default();
        let mut finished = None;
        let mut stream_error = None;
        while let Ok(event) = rx.recv_async().await {
            match event {
                ScriptEvent::Stream(StreamEvent::SchemaReady(pager)) => result.pager = Some(pager),
                ScriptEvent::Stream(StreamEvent::CommandComplete { affected_rows }) => {
                    result.affected = affected_rows
                }
                ScriptEvent::Stream(StreamEvent::Error(error)) => stream_error = Some(error),
                ScriptEvent::TransactionState { active } => result.active = Some(active),
                ScriptEvent::Finished(outcome) => {
                    finished = Some(outcome);
                    break;
                }
                _ => {}
            }
        }
        let outcome = finished.ok_or("Table session stopped before completion")?;
        if outcome.cancelled || outcome.cleanup_error.is_some() {
            return Err(format!(
                "Table session closed: {}",
                outcome
                    .error
                    .or(outcome.cleanup_error)
                    .unwrap_or_else(|| "operation cancelled".into())
            ));
        }
        if let Some(error) = outcome.error.or(stream_error) {
            return Err(error);
        }
        Ok(result)
    };
    match tokio::time::timeout(OPEN_AND_STATEMENT_TIMEOUT, async {
        let (_, result) = tokio::join!(run, collect);
        result
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            if let Some(owner) = owner.upgrade() {
                owner.close();
            }
            Err(
                "Table session timed out and was closed; transaction outcome may be uncertain"
                    .into(),
            )
        }
    }
}

fn savepoint_sql(provider: DbDriverKind) -> (String, String, Option<String>) {
    match provider {
        DbDriverKind::SqlServer => (
            format!("SAVE TRANSACTION {SAVEPOINT}"),
            format!("ROLLBACK TRANSACTION {SAVEPOINT}"),
            None,
        ),
        DbDriverKind::Oracle => (
            format!("SAVEPOINT {SAVEPOINT}"),
            format!("ROLLBACK TO {SAVEPOINT}"),
            None,
        ),
        DbDriverKind::Db2 => (
            format!("SAVEPOINT {SAVEPOINT} ON ROLLBACK RETAIN CURSORS"),
            format!("ROLLBACK TO SAVEPOINT {SAVEPOINT}"),
            Some(format!("RELEASE SAVEPOINT {SAVEPOINT}")),
        ),
        _ => (
            format!("SAVEPOINT {SAVEPOINT}"),
            format!("ROLLBACK TO SAVEPOINT {SAVEPOINT}"),
            Some(format!("RELEASE SAVEPOINT {SAVEPOINT}")),
        ),
    }
}

pub(super) async fn apply(
    driver: Arc<dyn DatabaseDriver>,
    owner: Weak<TabSession>,
    provider: DbDriverKind,
    initial_state: String,
    statements: Vec<String>,
    boundary: Boundary,
) -> Outcome {
    let mut active = match initial_state.as_str() {
        "Idle" | "Session closed" => false,
        "In transaction" => true,
        _ if boundary == Boundary::Rollback => true,
        _ => {
            return Outcome::failed(
                &initial_state,
                "Transaction state requires Rollback before saving table edits".into(),
            );
        }
    };
    if boundary == Boundary::Rollback {
        if active && let Err(error) = statement(driver, owner.clone(), "ROLLBACK".into()).await {
            if let Some(owner) = owner.upgrade() {
                owner.close();
            }
            return Outcome::failed(
                "Session closed",
                format!("Rollback could not be confirmed; reload the table: {error}"),
            );
        }
        return Outcome::success(false);
    }
    if !statements.is_empty() {
        if !active {
            match statement(driver.clone(), owner.clone(), "BEGIN".into()).await {
                Ok(_) => active = true,
                Err(error) => return Outcome::failed(&initial_state, error),
            }
        }
        let (save, rollback, release) = savepoint_sql(provider);
        if let Err(error) = statement(driver.clone(), owner.clone(), save).await {
            return Outcome::failed("In transaction", error);
        }
        let mut failure = None;
        for sql in statements {
            match statement(driver.clone(), owner.clone(), sql).await {
                Ok(result) if result.affected == Some(1) => {}
                Ok(result) => {
                    failure = Some(format!(
                        "Edit affected {:?} rows; expected exactly one",
                        result.affected
                    ));
                    break;
                }
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            }
        }
        if let Some(error) = failure {
            let recovered = statement(driver.clone(), owner.clone(), rollback).await;
            if recovered.is_ok() {
                if let Some(release) = release
                    && let Err(cleanup) = statement(driver.clone(), owner.clone(), release).await
                {
                    if let Some(owner) = owner.upgrade() {
                        owner.close();
                    }
                    return Outcome::failed(
                        "Session closed",
                        format!("{error}; batch cleanup failed; transaction closed: {cleanup}"),
                    );
                }
                if initial_state == "Idle" && boundary == Boundary::Commit {
                    let _ = statement(driver, owner, "ROLLBACK".into()).await;
                    return Outcome::failed("Idle", error);
                }
                let explanation = if initial_state == "In transaction" {
                    format!(
                        "{error}. This batch was rolled back; earlier saved edits remain uncommitted."
                    )
                } else {
                    format!("{error}. This batch was rolled back.")
                };
                return Outcome::failed("In transaction", explanation);
            }
            if let Some(owner) = owner.upgrade() {
                owner.close();
            }
            return Outcome::failed(
                "Session closed",
                format!(
                    "{error}; batch rollback failed; the session was closed. Reload before retrying."
                ),
            );
        }
        if let Some(release) = release
            && let Err(error) = statement(driver.clone(), owner.clone(), release).await
        {
            if let Some(owner) = owner.upgrade() {
                owner.close();
            }
            return Outcome::failed(
                "Session closed",
                format!("Batch cleanup failed; session closed: {error}"),
            );
        }
    }
    if boundary == Boundary::Commit && active {
        if let Err(error) = statement(driver, owner.clone(), "COMMIT".into()).await {
            if let Some(owner) = owner.upgrade() {
                owner.close();
            }
            return Outcome::failed(
                "Session closed",
                format!("Commit could not be confirmed; reload before retrying: {error}"),
            );
        }
        active = false;
    }
    Outcome::success(active)
}

#[cfg(test)]
mod tests {
    use super::{Boundary, apply, statement};
    use db_wire::script::TabSession;
    use db_wire::{ConnectionConfig, DbDriverKind, create_driver};
    use std::sync::Arc;

    #[test]
    fn manual_table_save_commit_rollback_and_failed_batches_use_one_session() {
        db_wire::get_runtime().block_on(async {
            let file = tempfile::NamedTempFile::new().unwrap();
            let driver = create_driver(ConnectionConfig {
                driver: DbDriverKind::Sqlite,
                database: file.path().to_string_lossy().into_owned(),
                ..Default::default()
            })
            .unwrap();
            let owner = Arc::new(TabSession::default());
            let other = Arc::new(TabSession::default());
            let run = |sql: &str| statement(driver.clone(), Arc::downgrade(&owner), sql.to_owned());
            run("CREATE TABLE values_test (id INTEGER PRIMARY KEY, value TEXT NOT NULL UNIQUE)")
                .await
                .unwrap();
            run("INSERT INTO values_test VALUES (1, 'original'), (2, 'second')")
                .await
                .unwrap();
            let read_other = || {
                statement(
                    driver.clone(),
                    Arc::downgrade(&other),
                    "SELECT value FROM values_test WHERE id = 1".into(),
                )
            };
            let saved = apply(
                driver.clone(),
                Arc::downgrade(&owner),
                DbDriverKind::Sqlite,
                "Idle".into(),
                vec!["UPDATE values_test SET value = 'saved' WHERE id = 1".into()],
                Boundary::Save,
            )
            .await;
            assert!(saved.error.is_none(), "{:?}", saved.error);
            assert_eq!(saved.state, "In transaction");
            assert_eq!(
                run("SELECT value FROM values_test WHERE id = 1")
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"saved"
            );
            assert_eq!(
                read_other()
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"original"
            );
            let failed = apply(
                driver.clone(),
                Arc::downgrade(&owner),
                DbDriverKind::Sqlite,
                saved.state.clone(),
                vec![
                    "UPDATE values_test SET value = 'partial' WHERE id = 1".into(),
                    "UPDATE values_test SET value = 'missing' WHERE id = 99".into(),
                ],
                Boundary::Commit,
            )
            .await;
            assert!(failed.error.unwrap().contains("expected exactly one"));
            assert_eq!(
                run("SELECT value FROM values_test WHERE id = 1")
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"saved"
            );
            let committed = apply(
                driver.clone(),
                Arc::downgrade(&owner),
                DbDriverKind::Sqlite,
                "In transaction".into(),
                vec!["UPDATE values_test SET value = 'committed' WHERE id = 1".into()],
                Boundary::Commit,
            )
            .await;
            assert!(committed.error.is_none(), "{:?}", committed.error);
            assert_eq!(committed.state, "Idle");
            assert_eq!(
                read_other()
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"committed"
            );
            let saved = apply(
                driver.clone(),
                Arc::downgrade(&owner),
                DbDriverKind::Sqlite,
                "Idle".into(),
                vec!["UPDATE values_test SET value = 'discard' WHERE id = 1".into()],
                Boundary::Save,
            )
            .await;
            assert!(saved.error.is_none());
            let rolled_back = apply(
                driver.clone(),
                Arc::downgrade(&owner),
                DbDriverKind::Sqlite,
                saved.state,
                vec![],
                Boundary::Rollback,
            )
            .await;
            assert!(rolled_back.error.is_none());
            assert_eq!(
                read_other()
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"committed"
            );
            let invalid = apply(
                driver.clone(),
                Arc::downgrade(&owner),
                DbDriverKind::Sqlite,
                "Idle".into(),
                vec![
                    "UPDATE values_test SET value = 'partial' WHERE id = 1".into(),
                    "UPDATE values_test SET value = NULL WHERE id = 2".into(),
                ],
                Boundary::Save,
            )
            .await;
            assert!(invalid.error.is_some());
            assert_eq!(
                run("SELECT value FROM values_test WHERE id = 1")
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"committed"
            );
            owner.close();
        });
    }

    #[test]
    fn autocommit_mode_immediate_commit_and_batch_failure_rolls_back() {
        db_wire::get_runtime().block_on(async {
            let file = tempfile::NamedTempFile::new().unwrap();
            let driver = create_driver(ConnectionConfig {
                driver: DbDriverKind::Sqlite,
                database: file.path().to_string_lossy().into_owned(),
                ..Default::default()
            })
            .unwrap();
            let owner = Arc::new(TabSession::default());
            let other = Arc::new(TabSession::default());
            let run = |sql: &str| statement(driver.clone(), Arc::downgrade(&owner), sql.to_owned());
            let read_other = || {
                statement(
                    driver.clone(),
                    Arc::downgrade(&other),
                    "SELECT name FROM auto_test WHERE id = 1".into(),
                )
            };
            run("CREATE TABLE auto_test (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE)")
                .await
                .unwrap();
            run("INSERT INTO auto_test VALUES (1, 'original')")
                .await
                .unwrap();

            // Auto-commit save: immediate commit
            let committed = apply(
                driver.clone(),
                Arc::downgrade(&owner),
                DbDriverKind::Sqlite,
                "Idle".into(),
                vec!["UPDATE auto_test SET name = 'auto_saved' WHERE id = 1".into()],
                Boundary::Commit,
            )
            .await;
            assert!(committed.error.is_none());
            assert_eq!(committed.state, "Idle");
            assert_eq!(
                read_other()
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"auto_saved"
            );

            // Failing batch in Auto-Commit mode: rolls back to Idle
            let failed = apply(
                driver.clone(),
                Arc::downgrade(&owner),
                DbDriverKind::Sqlite,
                "Idle".into(),
                vec![
                    "UPDATE auto_test SET name = 'partial' WHERE id = 1".into(),
                    "UPDATE auto_test SET name = NULL WHERE id = 1".into(),
                ],
                Boundary::Commit,
            )
            .await;
            assert!(failed.error.is_some());
            assert_eq!(failed.state, "Idle");
            // No partial writes committed
            assert_eq!(
                read_other()
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"auto_saved"
            );
            owner.close();
            other.close();
        });
    }

    #[test]
    fn independent_tab_ownership_and_concurrent_transactions() {
        db_wire::get_runtime().block_on(async {
            let file = tempfile::NamedTempFile::new().unwrap();
            let driver = create_driver(ConnectionConfig {
                driver: DbDriverKind::Sqlite,
                database: file.path().to_string_lossy().into_owned(),
                ..Default::default()
            })
            .unwrap();
            let tab1 = Arc::new(TabSession::default());
            let tab2 = Arc::new(TabSession::default());
            let run1 = |sql: &str| statement(driver.clone(), Arc::downgrade(&tab1), sql.to_owned());
            let run2 = |sql: &str| statement(driver.clone(), Arc::downgrade(&tab2), sql.to_owned());

            run1("CREATE TABLE concurrent_test (id INTEGER PRIMARY KEY, val TEXT NOT NULL)")
                .await
                .unwrap();
            run1("INSERT INTO concurrent_test VALUES (1, 'one'), (2, 'two')")
                .await
                .unwrap();

            // Tab 1 starts manual transaction and updates row 1
            let save1 = apply(
                driver.clone(),
                Arc::downgrade(&tab1),
                DbDriverKind::Sqlite,
                "Idle".into(),
                vec!["UPDATE concurrent_test SET val = 'one_t1' WHERE id = 1".into()],
                Boundary::Save,
            )
            .await;
            assert!(save1.error.is_none());
            assert_eq!(save1.state, "In transaction");

            // Tab 1 sees uncommitted write (read-your-writes)
            assert_eq!(
                run1("SELECT val FROM concurrent_test WHERE id = 1")
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"one_t1"
            );
            // Tab 2 still sees original write (isolation)
            assert_eq!(
                run2("SELECT val FROM concurrent_test WHERE id = 1")
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"one"
            );

            // Tab 1 commits
            let commit1 = apply(
                driver.clone(),
                Arc::downgrade(&tab1),
                DbDriverKind::Sqlite,
                "In transaction".into(),
                vec![],
                Boundary::Commit,
            )
            .await;
            assert!(commit1.error.is_none());
            assert_eq!(commit1.state, "Idle");

            // Tab 2 now sees committed write from Tab 1
            assert_eq!(
                run2("SELECT val FROM concurrent_test WHERE id = 1")
                    .await
                    .unwrap()
                    .pager
                    .unwrap()
                    .get_cell(0, 0)
                    .unwrap(),
                b"one_t1"
            );

            tab1.close();
            tab2.close();
        });
    }

    #[test]
    fn session_loss_and_abort_fails_cleanly() {
        db_wire::get_runtime().block_on(async {
            let file = tempfile::NamedTempFile::new().unwrap();
            let driver = create_driver(ConnectionConfig {
                driver: DbDriverKind::Sqlite,
                database: file.path().to_string_lossy().into_owned(),
                ..Default::default()
            })
            .unwrap();
            let owner = Arc::new(TabSession::default());
            let run = |sql: &str| statement(driver.clone(), Arc::downgrade(&owner), sql.to_owned());
            run("CREATE TABLE loss_test (id INTEGER PRIMARY KEY, val TEXT)")
                .await
                .unwrap();

            // Open transaction
            let saved = apply(
                driver.clone(),
                Arc::downgrade(&owner),
                DbDriverKind::Sqlite,
                "Idle".into(),
                vec!["INSERT INTO loss_test VALUES (1, 'test')".into()],
                Boundary::Save,
            )
            .await;
            assert!(saved.error.is_none());
            assert_eq!(saved.state, "In transaction");

            // Session owner is dropped (e.g. tab closed or session lost)
            let weak_owner = Arc::downgrade(&owner);
            drop(owner);

            // Next statement fails cleanly
            let next = statement(driver.clone(), weak_owner.clone(), "SELECT 1".into()).await;
            assert!(next.is_err());
            let err_msg = next.unwrap_err();
            assert!(
                err_msg.contains("closed")
                    || err_msg.contains("stopped")
                    || err_msg.contains("cancelled")
            );

            // Next apply fails cleanly
            let next_apply = apply(
                driver.clone(),
                weak_owner,
                DbDriverKind::Sqlite,
                "In transaction".into(),
                vec!["INSERT INTO loss_test VALUES (2, 'lost')".into()],
                Boundary::Commit,
            )
            .await;
            assert!(next_apply.error.is_some());
        });
    }

    #[test]
    fn live_provider_table_transactions() {
        for (prefix, provider) in [
            ("MYSQL", DbDriverKind::MySql),
            ("PG", DbDriverKind::Postgres),
        ] {
            let env = |key: &str| std::env::var(format!("FASTDB_TEST_{prefix}_{key}"));
            let Ok(port) = env("PORT") else {
                assert!(
                    std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
                    "FASTDB_TEST_{prefix}_PORT required"
                );
                eprintln!("skipped: FASTDB_TEST_{prefix}_PORT not set");
                continue;
            };
            let config = db_wire::ConnectionConfig {
                driver: provider,
                host: env("HOST").unwrap_or_else(|_| "127.0.0.1".into()),
                port: port.parse().unwrap(),
                database: env("DATABASE").unwrap_or_else(|_| {
                    if provider == DbDriverKind::Postgres {
                        "postgres"
                    } else {
                        "mysql"
                    }
                    .into()
                }),
                user: env("USER").unwrap_or_else(|_| {
                    if provider == DbDriverKind::Postgres {
                        "postgres"
                    } else {
                        "root"
                    }
                    .into()
                }),
                password: env("PASSWORD").ok(),
                use_unix_socket: false,
                ..Default::default()
            };
            db_wire::get_runtime().block_on(async {
                let driver = db_wire::create_driver(config).unwrap();
                let owner = Arc::new(TabSession::default());
                let other = Arc::new(TabSession::default());
                let run =
                    |sql: &str| statement(driver.clone(), Arc::downgrade(&owner), sql.to_owned());
                let read_other = || {
                    statement(
                        driver.clone(),
                        Arc::downgrade(&other),
                        "SELECT name FROM live_tx_test WHERE id = 1".into(),
                    )
                };
                let _ = run("DROP TABLE IF EXISTS live_tx_test").await;
                run("CREATE TABLE live_tx_test (id INT PRIMARY KEY, name VARCHAR(64) NOT NULL)")
                    .await
                    .unwrap();
                run("INSERT INTO live_tx_test VALUES (1, 'initial')")
                    .await
                    .unwrap();

                // Save in manual mode
                let saved = apply(
                    driver.clone(),
                    Arc::downgrade(&owner),
                    provider,
                    "Idle".into(),
                    vec!["UPDATE live_tx_test SET name = 'saved' WHERE id = 1".into()],
                    Boundary::Save,
                )
                .await;
                assert!(saved.error.is_none(), "{:?}", saved.error);
                assert_eq!(saved.state, "In transaction");

                // Read-your-writes
                assert_eq!(
                    run("SELECT name FROM live_tx_test WHERE id = 1")
                        .await
                        .unwrap()
                        .pager
                        .unwrap()
                        .get_cell(0, 0)
                        .unwrap(),
                    b"saved"
                );
                // Isolation
                assert_eq!(
                    read_other()
                        .await
                        .unwrap()
                        .pager
                        .unwrap()
                        .get_cell(0, 0)
                        .unwrap(),
                    b"initial"
                );

                // Batch failure in transaction rolls back to savepoint
                let failed = apply(
                    driver.clone(),
                    Arc::downgrade(&owner),
                    provider,
                    "In transaction".into(),
                    vec!["UPDATE live_tx_test SET name = 'invalid' WHERE id = 99".into()],
                    Boundary::Save,
                )
                .await;
                assert!(failed.error.is_some());
                assert_eq!(failed.state, "In transaction");
                // Earlier saved write is preserved
                assert_eq!(
                    run("SELECT name FROM live_tx_test WHERE id = 1")
                        .await
                        .unwrap()
                        .pager
                        .unwrap()
                        .get_cell(0, 0)
                        .unwrap(),
                    b"saved"
                );

                // Commit
                let committed = apply(
                    driver.clone(),
                    Arc::downgrade(&owner),
                    provider,
                    "In transaction".into(),
                    vec![],
                    Boundary::Commit,
                )
                .await;
                assert!(committed.error.is_none());
                assert_eq!(committed.state, "Idle");
                assert_eq!(
                    read_other()
                        .await
                        .unwrap()
                        .pager
                        .unwrap()
                        .get_cell(0, 0)
                        .unwrap(),
                    b"saved"
                );

                // Rollback test
                let saved2 = apply(
                    driver.clone(),
                    Arc::downgrade(&owner),
                    provider,
                    "Idle".into(),
                    vec!["UPDATE live_tx_test SET name = 'will_revert' WHERE id = 1".into()],
                    Boundary::Save,
                )
                .await;
                assert!(saved2.error.is_none());
                let rolled_back = apply(
                    driver.clone(),
                    Arc::downgrade(&owner),
                    provider,
                    saved2.state,
                    vec![],
                    Boundary::Rollback,
                )
                .await;
                assert!(rolled_back.error.is_none());
                assert_eq!(rolled_back.state, "Idle");
                assert_eq!(
                    read_other()
                        .await
                        .unwrap()
                        .pager
                        .unwrap()
                        .get_cell(0, 0)
                        .unwrap(),
                    b"saved"
                );

                let _ = run("DROP TABLE live_tx_test").await;
                owner.close();
                other.close();
            });
        }
    }
}
