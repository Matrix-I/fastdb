//! Native provider plans on physical tab sessions; live engines are opt-in.
use db_wire::query_plan::{QueryPlan, explain_sql};
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, MySqlDriver, StreamEvent};
use std::time::Duration;

#[test]
fn sqlite_plan_control_keeps_setup_and_transaction_statements_on_the_owned_session() {
    use db_wire::script::{ScriptControl, ScriptEvent, TabSession, run_tab_script};
    use std::sync::Arc;
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(db_wire::SqliteDriver::new_memory());
        let owner = Arc::new(TabSession::default());
        let run = |statements, control| {
            let driver = driver.clone();
            let owner = owner.clone();
            async move {
                let (tx, rx) = flume::unbounded();
                run_tab_script(driver, statements, Arc::new(control), tx, Arc::downgrade(&owner)).await;
                rx.drain().collect::<Vec<_>>()
            }
        };
        run(vec!["CREATE TEMP TABLE owned_plan (id INTEGER PRIMARY KEY)".into(), "INSERT INTO owned_plan VALUES (1)".into()], ScriptControl::default()).await;
        let events = run(vec!["BEGIN".into(), "DELETE FROM owned_plan WHERE id = 1".into()], ScriptControl::default().with_plan(DbDriverKind::Sqlite, false)).await;
        assert!(events.iter().any(|event| matches!(event, ScriptEvent::StatementStarted { sql } if sql == "BEGIN")));
        assert!(events.iter().any(|event| matches!(event, ScriptEvent::StatementStarted { sql } if sql.starts_with("EXPLAIN QUERY PLAN"))));
        assert!(events.iter().any(|event| matches!(event, ScriptEvent::TransactionState { active: true })));
        assert!(events.iter().any(|event| matches!(event, ScriptEvent::Finished(outcome) if outcome.error.is_none() && !outcome.cancelled)));
        let events = run(vec!["SELECT count(*) FROM owned_plan".into(), "ROLLBACK".into()], ScriptControl::default()).await;
        assert!(events.iter().any(|event| matches!(event, ScriptEvent::Stream(StreamEvent::SchemaReady(pager)) if pager.get_cell(0, 0).as_deref() == Some(b"1"))));
        owner.close();
    });
}

fn config(prefix: &str, kind: DbDriverKind) -> Option<ConnectionConfig> {
    let port = match std::env::var(format!("FASTDB_TEST_{prefix}_PORT")) {
        Ok(port) => port,
        Err(_) => {
            assert!(
                std::env::var_os(if kind == DbDriverKind::MariaDb {
                    "FASTDB_REQUIRE_MARIADB_TESTS"
                } else {
                    "FASTDB_REQUIRE_DB_TESTS"
                })
                .is_none(),
                "FASTDB_TEST_{prefix}_PORT required"
            );
            eprintln!("skipped: FASTDB_TEST_{prefix}_PORT not set");
            return None;
        }
    };
    let env = |suffix| std::env::var(format!("FASTDB_TEST_{prefix}_{suffix}")).ok();
    Some(ConnectionConfig {
        driver: kind,
        host: env("HOST").unwrap_or_else(|| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        user: env("USER").unwrap_or_else(|| {
            if kind == DbDriverKind::Postgres {
                "postgres"
            } else {
                "root"
            }
            .into()
        }),
        password: env("PASSWORD"),
        database: env("DATABASE").unwrap_or_else(|| "fastdb_test".into()),
        use_unix_socket: false,
        connect_timeout: Duration::from_secs(5),
        ..Default::default()
    })
}

async fn execute(session: &dyn db_wire::script::ScriptSession, sql: &str) -> Option<Vec<u8>> {
    let (tx, rx) = flume::unbounded();
    tokio::time::timeout(Duration::from_secs(15), session.execute_streaming(sql, tx))
        .await
        .unwrap()
        .unwrap();
    rx.drain().find_map(|event| match event {
        StreamEvent::SchemaReady(pager) => pager.get_cell(0, 0),
        _ => None,
    })
}

async fn native_plan(
    session: &dyn db_wire::script::ScriptSession,
    provider: DbDriverKind,
    statement: &str,
    analyze: bool,
) -> Vec<u8> {
    let (tx, rx) = flume::unbounded();
    tokio::time::timeout(
        Duration::from_secs(15),
        session.execute_plan(provider, statement, analyze, tx),
    )
    .await
    .unwrap()
    .unwrap();
    rx.drain()
        .find_map(|event| match event {
            StreamEvent::SchemaReady(pager) => pager.get_cell(0, 0),
            _ => None,
        })
        .expect("native plan cell")
}

fn verify(prefix: &str, kind: DbDriverKind) {
    let Some(config) = config(prefix, kind) else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = MySqlDriver::new(config);
        let session = driver.open_script_session().await.unwrap();
        execute(session.as_ref(), "CREATE TEMPORARY TABLE native_plan (id INT PRIMARY KEY, n INT)").await;
        execute(session.as_ref(), "INSERT INTO native_plan VALUES (1,2),(2,3),(3,4)").await;
        let identity = execute(session.as_ref(), "SELECT CONNECTION_ID()").await.unwrap();
        let statement = "SELECT n FROM native_plan WHERE n > 2";
        for analyze in [false, true] {
            let raw = native_plan(session.as_ref(), kind, statement, analyze).await;
            let plan = QueryPlan::parse_for_provider(kind, &raw).unwrap();
            assert_eq!(plan.provider, kind);
            assert!(plan.nodes.iter().any(|node| node.title.contains("native_plan")));
            assert!(plan.nodes.iter().any(|node| node.metrics.estimated_rows.is_some()));
            if analyze {
                assert!(plan.summary.contains("Actual"));
                assert!(plan.nodes.iter().any(|node| node.metrics.actual_loops.is_some()));
                assert!(plan.nodes.iter().any(|node| node.metrics.inclusive_work_ms().is_some()));
            } else {
                assert!(plan.summary.contains("Estimated"));
                assert!(plan.nodes.iter().all(|node| node.metrics.actual_loops.is_none()));
            }
        }
        assert_eq!(execute(session.as_ref(), "SELECT CONNECTION_ID()").await.unwrap(), identity);
        assert_eq!(execute(session.as_ref(), "SELECT count(*) FROM native_plan").await.unwrap(), b"3");
        if kind == DbDriverKind::MySql {
            let sql = explain_sql(kind, "SELECT ROW_NUMBER() OVER (ORDER BY n), SUM(n) OVER (PARTITION BY n) FROM native_plan", false).unwrap();
            let raw = execute(session.as_ref(), &sql).await.unwrap();
            let plan = QueryPlan::parse_for_provider(kind, &raw).unwrap();
            assert_eq!(plan.nodes.iter().filter(|node| node.title == "Window").count(), 2);
        }
        session.finish().await.unwrap();
    });
}

#[test]
fn mysql_native_estimates_actuals_and_windows_share_the_tab_session() {
    verify("MYSQL", DbDriverKind::MySql);
}

#[test]
fn mariadb_native_estimates_and_actuals_preserve_engine_metrics() {
    verify("MARIADB", DbDriverKind::MariaDb);
}

#[test]
fn mariadb_shared_driver_preserves_ordinary_and_generated_column_metadata() {
    use futures::FutureExt as _;
    let Some(config) = config("MARIADB", DbDriverKind::MariaDb) else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let schema = config.database.clone();
        let driver = MySqlDriver::new(config);
        let table = format!("native_columns_{}", uuid::Uuid::new_v4().simple());
        driver
            .execute_query(&format!(
                "CREATE TABLE {table} (id INT UNSIGNED, computed INT AS (id + 1) STORED)"
            ))
            .await
            .unwrap();
        let result = std::panic::AssertUnwindSafe(async {
            let columns = driver.list_columns(&schema, &table).await.unwrap();
            assert_eq!(columns.len(), 2);
            assert!(!columns[0].is_generated);
            assert!(columns[0].raw_type.as_deref().unwrap().contains("unsigned"));
            assert!(columns[1].is_generated);
            let all = driver.list_all_columns().await.unwrap();
            let (_, _, bulk) = all
                .iter()
                .find(|(s, t, _)| s == &schema && t == &table)
                .unwrap();
            assert_eq!(bulk, &columns);
        })
        .catch_unwind()
        .await;
        driver
            .execute_query(&format!("DROP TABLE {table}"))
            .await
            .unwrap();
        if let Err(panic) = result {
            std::panic::resume_unwind(panic);
        }
    });
}

#[test]
fn postgres_plan_api_preserves_native_metrics_and_temp_session_identity() {
    let Some(config) = config("PG", DbDriverKind::Postgres) else {
        return;
    };
    db_wire::get_runtime().block_on(async {
        let driver = db_wire::PostgresDriver::new(config);
        let session = driver.open_script_session().await.unwrap();
        execute(
            session.as_ref(),
            "CREATE TEMP TABLE native_plan (id INTEGER PRIMARY KEY, n INTEGER)",
        )
        .await;
        execute(
            session.as_ref(),
            "INSERT INTO native_plan VALUES (1,2),(2,3),(3,4)",
        )
        .await;
        let identity = execute(session.as_ref(), "SELECT pg_backend_pid()")
            .await
            .unwrap();
        for analyze in [false, true] {
            let raw = native_plan(
                session.as_ref(),
                DbDriverKind::Postgres,
                "SELECT n FROM native_plan WHERE n > 2",
                analyze,
            )
            .await;
            let plan = QueryPlan::parse_for_provider(DbDriverKind::Postgres, &raw).unwrap();
            assert!(
                plan.nodes
                    .iter()
                    .any(|node| node.title.contains("native_plan"))
            );
            assert!(
                plan.nodes
                    .iter()
                    .any(|node| node.metrics.estimated_rows.is_some())
            );
            assert!(
                plan.nodes
                    .iter()
                    .all(|node| node.metrics.actual_loops.is_some() == analyze)
            );
            if analyze {
                assert!(
                    plan.nodes
                        .iter()
                        .any(|node| node.metrics.inclusive_work_ms().is_some())
                );
            }
        }
        assert_eq!(
            execute(session.as_ref(), "SELECT pg_backend_pid()")
                .await
                .unwrap(),
            identity
        );
        assert_eq!(
            execute(session.as_ref(), "SELECT count(*) FROM native_plan")
                .await
                .unwrap(),
            b"3"
        );
        session.finish().await.unwrap();
    });
}
