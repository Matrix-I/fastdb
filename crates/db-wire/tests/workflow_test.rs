//! Automated data-path acceptance; desktop interaction is intentionally separate.
use db_wire::export::{export_to_csv, export_to_sql_inserts_for_target};
use db_wire::sql::{RowChanges, SqlDialect};
use db_wire::{DatabaseDriver, DbDriverKind, SqliteDriver, StreamEvent};
use std::sync::Arc;

#[tokio::test]
async fn sqlite_browse_edit_failure_recovery_and_export_readback() {
    let source = SqliteDriver::new_memory();
    let other = SqliteDriver::new_memory();
    for driver in [&source, &other] {
        driver
            .execute_query("CREATE TABLE sample(id INTEGER PRIMARY KEY, value TEXT)")
            .await
            .unwrap();
    }
    source
        .execute_query("INSERT INTO sample VALUES (1,'before'),(2,NULL),(3,'')")
        .await
        .unwrap();
    other
        .execute_query("INSERT INTO sample VALUES (1,'other connection')")
        .await
        .unwrap();
    assert!(
        source
            .list_tables()
            .await
            .unwrap()
            .iter()
            .any(|t| t.name == "sample")
    );
    let columns = source.list_columns("main", "sample").await.unwrap();
    assert!(columns[0].is_primary_key);
    let (sender, receiver) = flume::unbounded();
    source
        .execute_streaming("SELECT id,value FROM sample ORDER BY id", sender)
        .await
        .unwrap();
    let events: Vec<_> = receiver.drain().collect();
    let StreamEvent::SchemaReady(original) = &events[0] else {
        panic!("missing streamed schema");
    };
    assert!(matches!(
        events.last(),
        Some(StreamEvent::Completed { total_rows: 3, .. })
    ));
    let mut draft = RowChanges::default();
    draft
        .changes
        .insert((0, 1), Some("名,'quoted'\nline".into()));
    let planned = draft
        .plan_sql(
            "\"sample\"",
            original,
            &[0],
            SqlDialect(DbDriverKind::Sqlite),
        )
        .unwrap();
    assert_eq!(source.apply_row_changes(&planned).await.unwrap(), 1);
    let failed = vec![
        "UPDATE sample SET value='must roll back' WHERE id=1".into(),
        "UPDATE sample SET value='missing' WHERE id=99".into(),
    ];
    assert!(source.apply_row_changes(&failed).await.is_err());
    let (saved, count, _) = source
        .execute_query("SELECT id,value FROM sample ORDER BY id")
        .await
        .unwrap();
    assert_eq!(count, 3);
    assert_eq!(
        saved.get_cell(0, 1),
        Some("名,'quoted'\nline".as_bytes().to_vec())
    );
    assert_eq!(saved.get_cell(1, 1), None);
    assert_eq!(saved.get_cell(2, 1), Some(Vec::new()));
    let mut csv = Vec::new();
    assert_eq!(export_to_csv(&saved, &mut csv).unwrap(), 3);
    assert_eq!(
        String::from_utf8(csv).unwrap(),
        "id,value\n1,\"名,'quoted'\nline\"\n2,\n3,\n"
    );
    let destination = SqliteDriver::new_memory();
    destination
        .execute_query("CREATE TABLE sample(id INTEGER PRIMARY KEY,value TEXT)")
        .await
        .unwrap();
    let mut sql = Vec::new();
    assert_eq!(
        export_to_sql_inserts_for_target(
            "\"sample\"",
            SqlDialect(DbDriverKind::Sqlite),
            &saved,
            &mut sql,
            1,
            count
        )
        .unwrap(),
        3
    );
    for statement in String::from_utf8(sql).unwrap().split_inclusive(';') {
        if !statement.trim().is_empty() {
            destination.execute_query(statement).await.unwrap();
        }
    }
    let (restored, rows, _) = destination
        .execute_query("SELECT id,value FROM sample ORDER BY id")
        .await
        .unwrap();
    assert_eq!(rows, count);
    for row in 0..count {
        for col in 0..2 {
            assert_eq!(restored.get_cell(row, col), saved.get_cell(row, col));
        }
    }
    let (isolated, _, _) = other
        .execute_query("SELECT value FROM sample WHERE id=1")
        .await
        .unwrap();
    assert_eq!(isolated.get_cell(0, 0), Some(b"other connection".to_vec()));
}

/// A live provider for the shared data-path workflow. Each run owns two disposable
/// databases with identical table names so connection identity can be checked.
struct LiveProvider {
    kind: DbDriverKind,
    base: db_wire::ConnectionConfig,
    admin_database: &'static str,
    table_ddl: &'static str,
    long_query: &'static str,
    begin: &'static str,
}

impl LiveProvider {
    fn from_env(kind: DbDriverKind) -> Option<Self> {
        let (prefix, default_user, admin_database) = match kind {
            DbDriverKind::Postgres => ("FASTDB_TEST_PG", "postgres", "postgres"),
            _ => ("FASTDB_TEST_MYSQL", "root", "mysql"),
        };
        let env = |key: &str| std::env::var(format!("{prefix}_{key}")).ok();
        let Some(port) = env("PORT") else {
            assert!(
                std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
                "FASTDB_REQUIRE_DB_TESTS is set but {prefix}_PORT is not"
            );
            eprintln!("skipped: {prefix}_PORT not set");
            return None;
        };
        let base = db_wire::ConnectionConfig {
            driver: kind,
            host: env("HOST").unwrap_or_else(|| "127.0.0.1".into()),
            port: port.parse().expect("port must be a number"),
            database: admin_database.into(),
            user: env("USER").unwrap_or_else(|| default_user.into()),
            password: env("PASSWORD"),
            use_unix_socket: false,
            ..db_wire::ConnectionConfig::default()
        };
        Some(match kind {
            DbDriverKind::Postgres => Self {
                kind,
                base,
                admin_database,
                table_ddl: "CREATE TABLE wf_items (id integer PRIMARY KEY, label text, \
                            amount numeric(12,4) NOT NULL, parent_id integer REFERENCES wf_items(id))",
                long_query: "SELECT pg_sleep(30)",
                begin: "BEGIN",
            },
            _ => Self {
                kind,
                base,
                admin_database,
                table_ddl: "CREATE TABLE wf_items (id INT PRIMARY KEY, label TEXT, \
                            amount DECIMAL(12,4) NOT NULL, parent_id INT, \
                            FOREIGN KEY (parent_id) REFERENCES wf_items(id)) ENGINE=InnoDB",
                // SLEEP() returns 1 instead of failing when killed, so use real work.
                long_query: "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n \
                             WHERE i < 1000000000) SELECT /*+ SET_VAR(cte_max_recursion_depth = \
                             1000000000) */ COUNT(*) FROM n",
                begin: "START TRANSACTION",
            },
        })
    }

    fn driver(&self, database: &str) -> Arc<dyn DatabaseDriver> {
        let mut config = self.base.clone();
        config.database = database.into();
        db_wire::create_driver(config).unwrap()
    }

    fn schema(&self, database: &str) -> String {
        match self.kind {
            DbDriverKind::Postgres => "public".into(),
            _ => database.into(),
        }
    }

    fn dialect(&self) -> SqlDialect {
        SqlDialect(self.kind)
    }

    async fn recreate_database(&self, name: &str) {
        let admin = self.driver(self.admin_database);
        let drop = match self.kind {
            DbDriverKind::Postgres => format!("DROP DATABASE IF EXISTS {name} WITH (FORCE)"),
            _ => format!("DROP DATABASE IF EXISTS {name}"),
        };
        admin.execute_query(&drop).await.unwrap();
        admin
            .execute_query(&format!("CREATE DATABASE {name}"))
            .await
            .unwrap();
    }
}

fn text(pager: &db_wire::QueryResultPager, row: u64, col: usize) -> Option<String> {
    pager
        .get_cell(row, col)
        .map(|b| String::from_utf8(b).unwrap())
}

async fn rows(driver: &Arc<dyn DatabaseDriver>, sql: &str) -> Vec<Vec<Option<String>>> {
    let (pager, count, _) = driver.execute_query(sql).await.unwrap();
    (0..count)
        .map(|r| {
            (0..pager.num_cols() as usize)
                .map(|c| text(&pager, r, c))
                .collect()
        })
        .collect()
}

fn row(values: &[Option<&str>]) -> Vec<Option<String>> {
    values.iter().map(|v| v.map(str::to_string)).collect()
}

async fn run_statements(
    driver: Arc<dyn DatabaseDriver>,
    statements: &[&str],
    owner: Option<&Arc<db_wire::script::TabSession>>,
) -> (Vec<String>, Vec<u64>, db_wire::script::ScriptOutcome) {
    use db_wire::script::{ScriptControl, ScriptEvent};
    let (tx, rx) = flume::unbounded();
    let statements = statements.iter().map(|s| s.to_string()).collect();
    let control = Arc::new(ScriptControl::default());
    match owner {
        Some(owner) => {
            db_wire::script::run_tab_script(driver, statements, control, tx, Arc::downgrade(owner))
                .await
        }
        None => db_wire::script::run_script(driver, statements, control, tx).await,
    }
    let (mut started, mut completed, mut outcome) = (Vec::new(), Vec::new(), None);
    for event in rx.drain() {
        match event {
            ScriptEvent::StatementStarted { sql } => started.push(sql),
            ScriptEvent::Stream(StreamEvent::Completed { total_rows, .. }) => {
                completed.push(total_rows)
            }
            ScriptEvent::Finished(result) => outcome = Some(result),
            _ => {}
        }
    }
    (started, completed, outcome.expect("script must finish"))
}

/// connect → browse/filter → run script with a failing statement → edit/save/read-back
/// with rollback on failure → export/read-back → cancel + fresh read → disconnect.
async fn live_workflow(p: LiveProvider) {
    let (db_a, db_b) = ("fastdb_wf_a", "fastdb_wf_b");
    p.recreate_database(db_a).await;
    p.recreate_database(db_b).await;
    let source = p.driver(db_a);
    let other = p.driver(db_b);
    let schema = p.schema(db_a);
    let dialect = p.dialect();
    let target = dialect.qualified_table(&schema, "wf_items");

    // Connect: valid profiles ping; a closed port fails with an actionable error.
    source.ping().await.unwrap();
    let mut refused = p.base.clone();
    refused.port = 1;
    let error = db_wire::create_driver(refused)
        .unwrap()
        .ping()
        .await
        .unwrap_err();
    assert!(!format!("{error:#}").is_empty());

    for (driver, data) in [
        (&source, "(1,'before',1.25,NULL),(2,NULL,0,1),(3,'',-3.5,1)"),
        (&other, "(1,'other connection',9,NULL)"),
    ] {
        driver.execute_query(p.table_ddl).await.unwrap();
        driver
            .execute_query(&format!("INSERT INTO wf_items VALUES {data}"))
            .await
            .unwrap();
    }

    // Browse: metadata, streamed rows and a filtered/ordered read.
    let tables = source.list_tables().await.unwrap();
    assert!(
        tables
            .iter()
            .any(|t| t.schema == schema && t.name == "wf_items")
    );
    let columns = source.list_columns(&schema, "wf_items").await.unwrap();
    let names: Vec<_> = columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["id", "label", "amount", "parent_id"]);
    assert!(columns[0].is_primary_key);
    let (sender, receiver) = flume::unbounded();
    source
        .execute_streaming(
            "SELECT id, label, amount, parent_id FROM wf_items ORDER BY id",
            sender,
        )
        .await
        .unwrap();
    let events: Vec<_> = receiver.drain().collect();
    let StreamEvent::SchemaReady(original) = &events[0] else {
        panic!("missing streamed schema");
    };
    assert!(matches!(
        events.last(),
        Some(StreamEvent::Completed { total_rows: 3, .. })
    ));
    assert_eq!(
        rows(
            &source,
            "SELECT id FROM wf_items WHERE label IS NOT NULL ORDER BY id DESC"
        )
        .await,
        [row(&[Some("3")]), row(&[Some("1")])]
    );

    // Run: the failing statement stops the script; the earlier result stays distinct.
    let (started, completed, outcome) = run_statements(
        source.clone(),
        &[
            "SELECT id FROM wf_items",
            "SELECT * FROM wf_missing",
            "SELECT 1",
        ],
        None,
    )
    .await;
    assert_eq!(started.len(), 2, "{started:?}");
    assert_eq!(completed, [3]);
    assert!(outcome.error.is_some());

    // Edit/save: change, insert and delete in one planned save.
    let mut draft = RowChanges::default();
    draft
        .changes
        .insert((0, 1), Some("名,'quoted'\nline".into()));
    draft.inserted_rows.push(vec![
        Some("4".into()),
        Some("inserted".into()),
        Some("2.0000".into()),
        Some("1".into()),
    ]);
    draft.deleted_rows.insert(2);
    let planned = draft.plan_sql(&target, original, &[0], dialect).unwrap();
    assert_eq!(source.apply_row_changes(&planned).await.unwrap(), 3);

    // A failing multi-statement save rolls back atomically (row count and FK errors).
    for failing in [
        vec![
            format!("UPDATE {target} SET label = 'must roll back' WHERE id = 1"),
            format!("UPDATE {target} SET label = 'missing' WHERE id = 99"),
        ],
        vec![
            format!("UPDATE {target} SET label = 'must roll back' WHERE id = 2"),
            format!("INSERT INTO {target} VALUES (5, 'orphan', 1, 999)"),
        ],
    ] {
        assert!(source.apply_row_changes(&failing).await.is_err());
    }

    // Read-back from a fresh, independent connection.
    let fresh = p.driver(db_a);
    let saved_sql = "SELECT id, label, amount, parent_id FROM wf_items ORDER BY id";
    let expected = [
        row(&[Some("1"), Some("名,'quoted'\nline"), Some("1.2500"), None]),
        row(&[Some("2"), None, Some("0.0000"), Some("1")]),
        row(&[Some("4"), Some("inserted"), Some("2.0000"), Some("1")]),
    ];
    assert_eq!(rows(&fresh, saved_sql).await, expected);

    // Export: exact CSV bytes, and SQL INSERTs replayed into a separate table.
    let (saved, count, _) = fresh.execute_query(saved_sql).await.unwrap();
    let mut csv = Vec::new();
    assert_eq!(export_to_csv(&saved, &mut csv).unwrap(), 3);
    assert_eq!(
        String::from_utf8(csv).unwrap(),
        "id,label,amount,parent_id\n1,\"名,'quoted'\nline\",1.2500,\n2,,0.0000,1\n4,inserted,2.0000,1\n"
    );
    let restore_ddl = p
        .table_ddl
        .replace("wf_items", "wf_restore")
        // Drop the MySQL table constraint before the PostgreSQL column constraint.
        .replace(", FOREIGN KEY (parent_id) REFERENCES wf_restore(id)", "")
        .replace(" REFERENCES wf_restore(id)", "");
    source.execute_query(&restore_ddl).await.unwrap();
    let restore = dialect.qualified_table(&schema, "wf_restore");
    let mut sql = Vec::new();
    assert_eq!(
        export_to_sql_inserts_for_target(&restore, dialect, &saved, &mut sql, 1, count).unwrap(),
        3
    );
    for statement in String::from_utf8(sql).unwrap().split_inclusive(";\n") {
        if !statement.trim().is_empty() {
            source.execute_query(statement).await.unwrap();
        }
    }
    assert_eq!(
        rows(
            &fresh,
            "SELECT id, label, amount, parent_id FROM wf_restore ORDER BY id"
        )
        .await,
        expected
    );

    // Connection identity: the same-named table on the other database is untouched.
    assert_eq!(
        rows(&other, "SELECT id, label FROM wf_items").await,
        [row(&[Some("1"), Some("other connection")])]
    );

    // Cancel a long read, then run a fresh read on the same connection.
    let runner = source.clone();
    let long_query = p.long_query;
    let (tx, _rx) = flume::unbounded();
    let mut query = tokio::spawn(async move { runner.execute_streaming(long_query, tx).await });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(15);
    let result = loop {
        // Repeat until the server has started the query; cancelling before it runs is a no-op.
        source.cancel_active_query().await.unwrap();
        match tokio::time::timeout(std::time::Duration::from_millis(250), &mut query).await {
            Ok(joined) => break joined.unwrap(),
            Err(_) => assert!(
                tokio::time::Instant::now() < deadline,
                "cancelled query must stop promptly"
            ),
        }
    };
    assert!(result.is_err(), "cancelled query must report an error");
    assert_eq!(
        rows(&source, "SELECT COUNT(*) FROM wf_items").await,
        [row(&[Some("3")])]
    );

    // Disconnect: closing a tab session rolls back its open transaction.
    let owner = Arc::new(db_wire::script::TabSession::default());
    let insert = "INSERT INTO wf_items VALUES (9, 'uncommitted', 1, NULL)";
    let (_, _, outcome) = run_statements(source.clone(), &[p.begin, insert], Some(&owner)).await;
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    owner.close();
    let probe = "SELECT COUNT(*) FROM wf_items WHERE id = 9";
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
    // The server releases the aborted session's transaction once it sees the closed socket.
    loop {
        let (pager, _, _) = fresh.execute_query(probe).await.unwrap();
        if text(&pager, 0, 0).as_deref() == Some("0") {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "uncommitted row must not persist"
        );
        tokio::task::yield_now().await;
    }

    drop((source, other, fresh));
    let admin = p.driver(p.admin_database);
    for name in [db_a, db_b] {
        let drop = match p.kind {
            DbDriverKind::Postgres => format!("DROP DATABASE {name} WITH (FORCE)"),
            _ => format!("DROP DATABASE {name}"),
        };
        admin.execute_query(&drop).await.unwrap();
    }
}

#[test]
fn postgres_live_workflow() {
    if let Some(provider) = LiveProvider::from_env(DbDriverKind::Postgres) {
        db_wire::get_runtime().block_on(live_workflow(provider));
    }
}

#[test]
fn mysql_live_workflow() {
    if let Some(provider) = LiveProvider::from_env(DbDriverKind::MySql) {
        db_wire::get_runtime().block_on(live_workflow(provider));
    }
}
