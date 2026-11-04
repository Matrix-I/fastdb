use db_wire::DbDriverKind;

pub(super) const IMPLICIT_BEGIN_SQL: &str = "BEGIN";
pub(super) const FAILED_TRANSACTION_STATE: &str =
    "Transaction failed; Rollback or ROLLBACK TO required";

pub(super) fn client_managed(provider: DbDriverKind) -> bool {
    matches!(provider, DbDriverKind::Postgres | DbDriverKind::Sqlite)
}

/// Called after script preflight. Ordinary statements begin a manual transaction
/// lazily; explicit controls stay in order and execution still stops on errors.
pub(super) fn script_control(
    provider: DbDriverKind,
    manual: bool,
    timeout: Option<std::time::Duration>,
) -> db_wire::script::ScriptControl {
    let control = db_wire::script::ScriptControl::with_timeout(timeout);
    if manual && provider == DbDriverKind::Sqlite {
        control.with_native_manual_transactions(|sql| {
            sql_intel::session_transaction_transition(sql).is_none()
        })
    } else {
        control
    }
}

pub(super) fn manual_statements(
    provider: DbDriverKind,
    manual: bool,
    initial_state: &str,
    statements: Vec<String>,
) -> Result<Vec<String>, String> {
    if !manual || !client_managed(provider) {
        return Ok(statements);
    }
    let mut state = match initial_state {
        "Idle" | "Session closed" => Some(false),
        "In transaction" | FAILED_TRANSACTION_STATE => Some(true),
        _ => None,
    };
    let mut planned = Vec::with_capacity(statements.len());
    for sql in statements {
        if sql.trim().is_empty() {
            continue;
        }
        if let Some(transition) = sql_intel::session_transaction_transition(&sql) {
            state = Some(transition == "In transaction");
        } else {
            match state {
                None => {
                    return Err(
                        "Transaction state is unknown; run Commit or Rollback before manual SQL"
                            .into(),
                    );
                }
                Some(false) => {
                    if provider != DbDriverKind::Sqlite {
                        planned.push(IMPLICIT_BEGIN_SQL.into());
                    }
                    state = Some(true);
                }
                Some(true) => {}
            }
        }
        planned.push(sql);
    }
    Ok(planned)
}

pub(super) fn statement_is_plan(requested: bool, sql: &str) -> bool {
    requested && sql != IMPLICIT_BEGIN_SQL
}

#[cfg(test)]
mod tests {
    use super::manual_statements;
    use db_wire::script::{ScriptEvent, TabSession, run_tab_script};
    use db_wire::{DatabaseDriver, DbDriverKind, QueryResultPager, StreamEvent};
    use std::sync::Arc;

    async fn execute(
        driver: Arc<dyn DatabaseDriver>,
        owner: &Arc<TabSession>,
        provider: DbDriverKind,
        manual: bool,
        state: &str,
        sql: &str,
    ) -> Result<Vec<Arc<QueryResultPager>>, String> {
        let statements = manual_statements(
            provider,
            manual,
            state,
            sql_intel::split_session_script(sql)?,
        )?;
        let (tx, rx) = flume::unbounded();
        run_tab_script(
            driver,
            statements,
            Arc::new(super::script_control(
                provider,
                manual,
                Some(std::time::Duration::from_secs(5)),
            )),
            tx,
            Arc::downgrade(owner),
        )
        .await;
        let mut pagers = Vec::new();
        let mut finished = false;
        for event in rx.drain() {
            match event {
                ScriptEvent::Stream(StreamEvent::SchemaReady(pager)) => pagers.push(pager),
                ScriptEvent::Finished(outcome) => {
                    finished = true;
                    if let Some(error) = outcome.error.or(outcome.cleanup_error) {
                        return Err(error);
                    }
                    if outcome.cancelled {
                        return Err("Script cancelled".into());
                    }
                }
                _ => {}
            }
        }
        if !finished {
            return Err("Script stopped without completion".into());
        }
        Ok(pagers)
    }

    async fn verify_manual_transactions(driver: Arc<dyn DatabaseDriver>, provider: DbDriverKind) {
        let table = format!("fastdb_manual_{}", std::process::id());
        driver
            .execute_query(&format!("CREATE TABLE {table} (id INTEGER PRIMARY KEY)"))
            .await
            .unwrap();
        let owner = Arc::new(TabSession::default());
        execute(
            driver.clone(),
            &owner,
            provider,
            true,
            "Idle",
            &format!("INSERT INTO {table} VALUES (1)"),
        )
        .await
        .unwrap();
        execute(
            driver.clone(),
            &owner,
            provider,
            true,
            "In transaction",
            &format!("SAVEPOINT kept; INSERT INTO {table} VALUES (2)"),
        )
        .await
        .unwrap();
        assert!(
            execute(
                driver.clone(),
                &owner,
                provider,
                true,
                "In transaction",
                &format!("INSERT INTO {table} VALUES (1)")
            )
            .await
            .is_err()
        );
        execute(
            driver.clone(),
            &owner,
            provider,
            true,
            "Transaction failed; Rollback or ROLLBACK TO required",
            &format!("ROLLBACK TO kept; INSERT INTO {table} VALUES (2)"),
        )
        .await
        .unwrap();
        let (count, _, _) = driver
            .execute_query(&format!("SELECT count(*) FROM {table}"))
            .await
            .unwrap();
        assert_eq!(count.get_cell(0, 0), Some(b"0".to_vec()));
        execute(
            driver.clone(),
            &owner,
            provider,
            true,
            "In transaction",
            "ROLLBACK",
        )
        .await
        .unwrap();
        execute(
            driver.clone(),
            &owner,
            provider,
            true,
            "Idle",
            &format!(
                "INSERT INTO {table} VALUES (3); COMMIT; INSERT INTO {table} VALUES (4); ROLLBACK"
            ),
        )
        .await
        .unwrap();
        execute(
            driver.clone(),
            &owner,
            provider,
            true,
            "Idle",
            &format!("SAVEPOINT nested; INSERT INTO {table} VALUES (5); RELEASE nested; ROLLBACK"),
        )
        .await
        .unwrap();
        if provider == DbDriverKind::Postgres {
            let sql = db_wire::query_plan::explain_sql(
                provider,
                &format!("UPDATE {table} SET id = 13 WHERE id = 3"),
                true,
            )
            .unwrap();
            let results = execute(driver.clone(), &owner, provider, true, "Idle", &sql)
                .await
                .unwrap();
            let json = results
                .iter()
                .find_map(|pager| pager.get_cell(0, 0))
                .unwrap();
            assert!(
                !db_wire::query_plan::QueryPlan::parse(&json)
                    .unwrap()
                    .nodes
                    .is_empty()
            );
            let (outside, _, _) = driver
                .execute_query(&format!("SELECT id FROM {table}"))
                .await
                .unwrap();
            assert_eq!(outside.get_cell(0, 0), Some(b"3".to_vec()));
            execute(
                driver.clone(),
                &owner,
                provider,
                true,
                "In transaction",
                "ROLLBACK",
            )
            .await
            .unwrap();
        }
        execute(
            driver.clone(),
            &owner,
            provider,
            false,
            "Idle",
            &format!("INSERT INTO {table} VALUES (6)"),
        )
        .await
        .unwrap();
        let (rows, count, _) = driver
            .execute_query(&format!("SELECT id FROM {table} ORDER BY id"))
            .await
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(rows.get_cell(0, 0), Some(b"3".to_vec()));
        assert_eq!(rows.get_cell(1, 0), Some(b"6".to_vec()));
        owner.close();
        driver
            .execute_query(&format!("DROP TABLE {table}"))
            .await
            .unwrap();
    }

    #[test]
    fn sqlite_manual_mode_preserves_transactions_across_runs_and_boundaries() {
        let directory = tempfile::tempdir().unwrap();
        let driver = db_wire::SqliteDriver::new(db_wire::ConnectionConfig {
            driver: DbDriverKind::Sqlite,
            database: directory
                .path()
                .join("manual.db")
                .to_string_lossy()
                .into_owned(),
            ..Default::default()
        })
        .unwrap();
        db_wire::get_runtime().block_on(verify_manual_transactions(
            Arc::new(driver),
            DbDriverKind::Sqlite,
        ));
    }

    #[test]
    fn postgres_manual_mode_preserves_transactions_and_analyze_uses_the_same_session() {
        let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
            assert!(
                std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
                "FASTDB_TEST_PG_PORT required"
            );
            eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
            return;
        };
        let driver = db_wire::PostgresDriver::new(db_wire::ConnectionConfig {
            driver: DbDriverKind::Postgres,
            host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
            port: port.parse().unwrap(),
            database: std::env::var("FASTDB_TEST_PG_DATABASE")
                .unwrap_or_else(|_| "postgres".into()),
            user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
            password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
            use_unix_socket: false,
            ..Default::default()
        });
        db_wire::get_runtime().block_on(verify_manual_transactions(
            Arc::new(driver),
            DbDriverKind::Postgres,
        ));
    }

    #[test]
    fn sqlite_manual_mode_restarts_after_releasing_an_existing_outer_savepoint() {
        db_wire::get_runtime().block_on(async {
            let driver: Arc<dyn DatabaseDriver> = Arc::new(db_wire::SqliteDriver::new(
                db_wire::ConnectionConfig { driver: DbDriverKind::Sqlite, database: ":memory:".into(), ..Default::default() }
            ).unwrap());
            let owner = Arc::new(TabSession::default());
            execute(driver.clone(), &owner, DbDriverKind::Sqlite, false, "Idle",
                "CREATE TABLE items (id INTEGER); SAVEPOINT outermost; INSERT INTO items VALUES (1)").await.unwrap();
            let result = execute(driver.clone(), &owner, DbDriverKind::Sqlite, true, "In transaction",
                "RELEASE outermost; INSERT INTO items VALUES (2); ROLLBACK").await;
            let rows = execute(driver, &owner, DbDriverKind::Sqlite, false, "Idle", "SELECT id FROM items ORDER BY id").await.unwrap();
            assert!(result.is_ok(), "{}", result.err().unwrap_or_default());
            assert_eq!(rows[0].get_cell(0, 0), Some(b"1".to_vec()));
            assert_eq!(rows[0].get_cell(1, 0), None);
        });
    }

    #[test]
    fn implicit_begin_is_execution_feedback_not_a_query_plan() {
        assert!(!super::statement_is_plan(true, "BEGIN"));
        assert!(super::statement_is_plan(
            true,
            "EXPLAIN (FORMAT JSON) SELECT 1"
        ));
        assert!(!super::statement_is_plan(false, "SELECT 1"));
    }

    #[test]
    fn manual_mode_starts_new_transactions_after_explicit_boundaries() {
        let input = vec!["SELECT 1", "COMMIT", "SELECT 2", "ROLLBACK", "SELECT 3"]
            .into_iter()
            .map(String::from)
            .collect();
        assert_eq!(
            manual_statements(DbDriverKind::Postgres, true, "Idle", input).unwrap(),
            [
                "BEGIN", "SELECT 1", "COMMIT", "BEGIN", "SELECT 2", "ROLLBACK", "BEGIN", "SELECT 3"
            ]
        );
        assert_eq!(
            manual_statements(
                DbDriverKind::Sqlite,
                true,
                "In transaction",
                vec!["SELECT 1".into()]
            )
            .unwrap(),
            ["SELECT 1"]
        );
        assert_eq!(
            manual_statements(
                DbDriverKind::Sqlite,
                true,
                "Session closed",
                vec!["SELECT 1".into()]
            )
            .unwrap(),
            ["SELECT 1"]
        );
    }

    #[test]
    fn unknown_state_blocks_ordinary_sql_but_allows_recovery_and_preserves_other_modes() {
        assert!(
            manual_statements(
                DbDriverKind::Postgres,
                true,
                "Unknown",
                vec!["INSERT INTO items VALUES (1)".into()]
            )
            .is_err()
        );
        assert_eq!(
            manual_statements(
                DbDriverKind::Postgres,
                true,
                "Unknown",
                vec!["/* recover */ ROLLBACK".into(), "SELECT 1".into()]
            )
            .unwrap(),
            ["/* recover */ ROLLBACK", "BEGIN", "SELECT 1"]
        );
        for provider in [
            DbDriverKind::Postgres,
            DbDriverKind::Sqlite,
            DbDriverKind::MySql,
        ] {
            assert_eq!(
                manual_statements(provider, false, "Idle", vec!["SELECT 1".into()]).unwrap(),
                ["SELECT 1"]
            );
        }
        assert_eq!(
            manual_statements(DbDriverKind::MySql, true, "Idle", vec!["SELECT 1".into()]).unwrap(),
            ["SELECT 1"]
        );
    }
}
