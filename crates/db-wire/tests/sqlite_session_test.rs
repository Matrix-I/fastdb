use db_wire::{DatabaseDriver, SqliteDriver, StreamEvent};

async fn execute(session: &dyn db_wire::script::ScriptSession, sql: &str) -> Vec<StreamEvent> {
    let (tx, rx) = flume::unbounded();
    session.execute_streaming(sql, tx).await.unwrap();
    rx.drain().collect()
}

#[test]
fn sqlite_sessions_share_database_but_not_temp_objects_and_rollback_on_close() {
    db_wire::get_runtime().block_on(async {
        let driver = SqliteDriver::new_memory();
        driver
            .execute_query("CREATE TABLE items (id INTEGER PRIMARY KEY)")
            .await
            .unwrap();
        assert!(driver.supports_script_session());
        let session = driver.open_script_session().await.unwrap();
        assert_eq!(session.in_transaction(), Some(false));
        execute(session.as_ref(), "SAVEPOINT outer_savepoint").await;
        assert_eq!(session.in_transaction(), Some(true));
        execute(session.as_ref(), "RELEASE outer_savepoint").await;
        assert_eq!(session.in_transaction(), Some(false));
        execute(
            session.as_ref(),
            "CREATE TEMP TABLE private_items (id INTEGER)",
        )
        .await;
        execute(session.as_ref(), "BEGIN").await;
        execute(session.as_ref(), "INSERT INTO items VALUES (1)").await;
        execute(session.as_ref(), "SAVEPOINT saved").await;
        execute(session.as_ref(), "INSERT INTO items VALUES (2)").await;
        execute(session.as_ref(), "ROLLBACK TO saved").await;
        execute(session.as_ref(), "COMMIT").await;
        let (pager, count, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(count, 1);
        assert_eq!(pager.get_cell(0, 0), Some(b"1".to_vec()));
        assert!(
            driver
                .execute_query("SELECT * FROM private_items")
                .await
                .is_err()
        );
        let second = driver.open_script_session().await.unwrap();
        let (tx, _rx) = flume::unbounded();
        let error = second
            .execute_streaming("SELECT * FROM private_items", tx)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("no such table"), "{error}");
        second.finish().await.unwrap();
        execute(session.as_ref(), "BEGIN").await;
        execute(session.as_ref(), "INSERT INTO items VALUES (3)").await;
        session.finish().await.unwrap();
        let (_, count, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(count, 1);
        let (tx, _rx) = flume::unbounded();
        assert!(
            session
                .execute_streaming("INSERT INTO items VALUES (4)", tx)
                .await
                .is_err()
        );
        let (_, count, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(count, 1);
    });
}

#[test]
fn sqlite_abort_rolls_back_and_memory_drivers_remain_independent() {
    db_wire::get_runtime().block_on(async {
        let driver = SqliteDriver::new_memory();
        driver
            .execute_query("CREATE TABLE items (id INTEGER)")
            .await
            .unwrap();
        let other = SqliteDriver::new_memory();
        let error = other
            .execute_query("SELECT * FROM items")
            .await
            .err()
            .expect("independent memory database");
        assert!(error.to_string().contains("no such table"));
        let session = driver.open_script_session().await.unwrap();
        execute(session.as_ref(), "BEGIN").await;
        execute(session.as_ref(), "INSERT INTO items VALUES (1)").await;
        session.abort();
        let (_, count, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(count, 0);
        session.finish().await.unwrap();
    });
}

#[test]
fn sqlite_session_cancellation_stops_worker_and_rolls_back() {
    db_wire::get_runtime().block_on(async {
        let driver = SqliteDriver::new_memory();
        driver.execute_query("CREATE TABLE items (id INTEGER)").await.unwrap();
        let session = driver.open_script_session().await.unwrap();
        execute(session.as_ref(), "BEGIN").await;
        execute(session.as_ref(), "INSERT INTO items VALUES (1)").await;
        let worker_session = session.clone();
        let (tx, rx) = flume::unbounded();
        let worker = tokio::spawn(async move {
            worker_session.execute_streaming(
                "WITH RECURSIVE numbers(n) AS (VALUES(1) UNION ALL SELECT n + 1 FROM numbers WHERE n < 1000000000) SELECT sum(n) FROM numbers",
                tx,
            ).await
        });
        assert!(matches!(rx.recv_async().await.unwrap(), StreamEvent::SchemaReady(_)));
        session.cancel_active_query().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), session.finish()).await.unwrap().unwrap();
        assert!(worker.await.unwrap().is_err());
        let (_, count, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(count, 0);
    });
}

#[test]
fn sqlite_tab_runs_preserve_temp_state_and_owner_drop_rolls_back() {
    use db_wire::script::{ScriptControl, ScriptEvent, TabSession, run_tab_script};
    use std::sync::Arc;
    db_wire::get_runtime().block_on(async {
        let driver = Arc::new(SqliteDriver::new_memory());
        driver.execute_query("CREATE TABLE items (id INTEGER)").await.unwrap();
        let owner = Arc::new(TabSession::default());
        for sql in ["CREATE TEMP TABLE private_items (id INTEGER)", "INSERT INTO private_items VALUES (42)", "BEGIN", "INSERT INTO items SELECT id FROM private_items"] {
            let (tx, rx) = flume::unbounded();
            run_tab_script(driver.clone(), vec![sql.into()], Arc::new(ScriptControl::default()), tx, Arc::downgrade(&owner)).await;
            assert!(rx.drain().any(|event| matches!(event, ScriptEvent::Finished(outcome) if outcome.error.is_none() && outcome.cleanup_error.is_none() && !outcome.cancelled)), "{sql}");
        }
        drop(owner);
        let (_, count, _) = driver.execute_query("SELECT id FROM items").await.unwrap();
        assert_eq!(count, 0);
    });
}
