use db_wire::administration::{ADMIN_PAGE_SIZE, ADMIN_RESULT_ROWS, AdminFamily};
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, PostgresDriver};
use futures::FutureExt;
use std::time::Duration;

fn config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        eprintln!("skipped: FASTDB_TEST_PG_PORT not set");
        return None;
    };
    let env = |name| std::env::var(format!("FASTDB_TEST_PG_{name}")).ok();
    Some(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: env("HOST").unwrap_or_else(|| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        database: env("DATABASE").unwrap_or_else(|| "postgres".into()),
        user: env("USER").unwrap_or_else(|| "postgres".into()),
        password: env("PASSWORD"),
        use_unix_socket: false,
        connect_timeout: Duration::from_secs(5),
        ..Default::default()
    })
}
async fn run(driver: &PostgresDriver, sql: &str) -> db_wire::administration::AdminOutcome {
    let (_send, cancel) = flume::bounded(1);
    driver
        .execute_administration(sql, cancel)
        .await
        .unwrap_or_else(|e| panic!("administration fixture failed: {e:#}"))
}

#[test]
fn postgres_administration_mutates_catalogs_and_preserves_transaction_boundaries() {
    let Some(config) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async {
            let driver = PostgresDriver::new(config.clone());
            // Dedicated schema and roles are unique per invocation, not shared fixtures.
            let suffix = uuid::Uuid::new_v4().simple().to_string();
            let schema = format!("admin_{suffix}");
            let role = format!("admin_role_{suffix}");
            let member = format!("admin_member_{suffix}");
            let acl_roles = (0..180).map(|i|format!("admin_acl_{suffix}_{i}")).collect::<Vec<_>>();
            run(&driver, &format!("CREATE SCHEMA \"{schema}\";")).await;
            let result = tokio::time::timeout(Duration::from_secs(70), std::panic::AssertUnwindSafe(async {
                run(&driver, &format!("CREATE FUNCTION \"{schema}\".f(value integer) RETURNS integer LANGUAGE sql AS $$ SELECT value+1 $$;")).await;
                assert_eq!(run(&driver, &format!("SELECT \"{schema}\".f(5)")).await.rows, vec![vec![Some("6".into())]]);
                run(&driver, &format!("CREATE OR REPLACE FUNCTION \"{schema}\".f(value integer) RETURNS integer LANGUAGE sql AS $$ SELECT value+2 $$;")).await;
                assert_eq!(run(&driver, &format!("SELECT \"{schema}\".f(5)")).await.rows[0][0].as_deref(), Some("7"));
                run(&driver, &format!("CREATE FUNCTION \"{schema}\".variadic_f(OUT value integer, VARIADIC values_in integer[]) LANGUAGE sql AS $$ SELECT coalesce(array_length(values_in,1),0) $$")).await;
                run(&driver, &format!("CREATE PROCEDURE \"{schema}\".p(value integer) LANGUAGE plpgsql AS $$ BEGIN PERFORM value; END $$")).await;
                run(&driver, &format!("CALL \"{schema}\".p(9)")).await;
                run(&driver, &format!("CREATE SEQUENCE \"{schema}\".s")).await;
                run(&driver, &format!("ALTER SEQUENCE \"{schema}\".s RESTART WITH 42")).await;
                assert_eq!(run(&driver, &format!("SELECT nextval('\"{schema}\".s')")).await.rows[0][0].as_deref(), Some("42"));
                run(&driver, &format!("CREATE TABLE \"{schema}\".t(id integer)")).await;
                run(&driver, &format!("CREATE FUNCTION \"{schema}\".tf() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$")).await;
                run(&driver, &format!("CREATE TRIGGER tr BEFORE INSERT ON \"{schema}\".t FOR EACH ROW EXECUTE FUNCTION \"{schema}\".tf()")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".t DISABLE TRIGGER tr")).await;
                run(&driver, &format!("CREATE RULE rule1 AS ON DELETE TO \"{schema}\".t DO INSTEAD NOTHING")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".t DISABLE RULE rule1")).await;
                run(&driver, &format!("CREATE POLICY policy1 ON \"{schema}\".t FOR SELECT USING (id>0)")).await;
                run(&driver, &format!("ALTER POLICY policy1 ON \"{schema}\".t USING (id>1)")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".t ENABLE ROW LEVEL SECURITY")).await;
                run(&driver, &format!("CREATE TABLE \"{schema}\".root(id integer) PARTITION BY RANGE(id)")).await;
                run(&driver, &format!("CREATE TABLE \"{schema}\".child PARTITION OF \"{schema}\".root FOR VALUES FROM (0) TO (10)")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".root DETACH PARTITION \"{schema}\".child")).await;
                run(&driver, &format!("ALTER TABLE \"{schema}\".root ATTACH PARTITION \"{schema}\".child FOR VALUES FROM (0) TO (10)")).await;
                run(&driver, &format!("CREATE ROLE \"{role}\" NOLOGIN")).await;
                run(&driver, &format!("CREATE ROLE \"{member}\" NOLOGIN")).await;
                run(&driver, &format!("GRANT \"{role}\" TO \"{member}\"")).await;
                run(&driver, &format!("GRANT SELECT ON \"{schema}\".t TO \"{role}\"")).await;
                let acl_fixture = acl_roles.iter().map(|role| format!("CREATE ROLE \"{role}\" NOLOGIN;")).collect::<String>();
                driver.execute_schema_ddl(&acl_fixture).await.unwrap();
                run(&driver,&format!("GRANT SELECT ON \"{schema}\".t TO {}",acl_roles.iter().map(|r|format!("\"{r}\"")).collect::<Vec<_>>().join(","))).await;
                let pagination_fixture = (0..55).map(|i| format!("CREATE SEQUENCE \"{schema}\".page_{i};")).collect::<String>();
                driver.execute_schema_ddl(&pagination_fixture).await.unwrap();
                for family in AdminFamily::ALL {
                    let mut after = 0; let mut found = false; let mut pages = 0;
                    loop {
                        let page = driver.administration_catalog(family, after).await.unwrap();
                        pages += 1;
                        assert!(page.objects.len() <= ADMIN_PAGE_SIZE);
                        for object in &page.objects {
                            found |= if family == AdminFamily::Roles { object.name == role } else { object.schema == schema };
                            if object.schema == schema && family == AdminFamily::Routines && object.name == "f" {
                                assert!(object.invocation.as_ref().unwrap().contains("NULL::integer"));
                                assert!(object.source.as_ref().unwrap().contains("value+2"));
                            }
                            if object.schema == schema && family == AdminFamily::Routines && object.name == "variadic_f" {
                                let sql = object.invocation.as_ref().unwrap();
                                assert!(sql.contains("VARIADIC NULL::integer[]"));
                                assert_eq!(run(&driver,sql).await.rows[0][0].as_deref(),Some("0"));
                            }
                            if object.schema == schema && matches!(family, AdminFamily::Triggers | AdminFamily::Rules) {
                                assert!(object.details.contains("enabled: D"));
                            }
                            if object.schema == schema && family == AdminFamily::Partitions && object.name == "child" {
                                assert_eq!(object.parent,format!("{schema}.root"));
                                assert!(object.details.contains("FOR VALUES FROM (0) TO (10)"));
                            }
                            if family == AdminFamily::Roles && object.name == role { assert!(object.details.contains("login: f")); }
                            if object.schema == schema && family == AdminFamily::Privileges && object.name == "t" {
                                assert!(object.details.contains(&role));
                                assert!(object.details.ends_with("\nDetails truncated at 8192 characters."));
                                assert!(object.details.chars().count() < 8300);
                            }
                            if object.schema == schema && let Some(source) = &object.source {
                                run(&driver,source).await;
                            }
                            if object.schema == schema && family == AdminFamily::Policies {
                                assert!(object.source.as_ref().unwrap().contains("id > 1"));
                            }
                        }
                        if let Some(next) = page.next { assert!(next > after); after = next; } else { break; }
                    }
                    assert!(found, "missing family {family:?}");
                    if family == AdminFamily::Sequences { assert!(pages >= 2); }
                }
                let empty = driver.administration_catalog(AdminFamily::Roles,u32::MAX).await.unwrap();
                assert!(empty.objects.is_empty()); assert!(empty.next.is_none());
                run(&driver, &format!("REVOKE \"{role}\" FROM \"{member}\"")).await;
                assert_eq!(run(&driver, &format!("SELECT pg_has_role('{member}','{role}','MEMBER')")).await.rows[0][0].as_deref(),Some("f"));
                assert_eq!(run(&driver, &format!("SELECT has_table_privilege('{role}','\"{schema}\".t','SELECT')")).await.rows[0][0].as_deref(), Some("t"));
                run(&driver, &format!("REVOKE SELECT ON \"{schema}\".t FROM \"{role}\"")).await;
                assert_eq!(run(&driver, &format!("SELECT has_table_privilege('{role}','\"{schema}\".t','SELECT')")).await.rows[0][0].as_deref(), Some("f"));
                let outcome = run(&driver, "SELECT generate_series(1,150),NULL::text,''::text").await;
                assert_eq!(outcome.rows.len(), ADMIN_RESULT_ROWS); assert!(outcome.truncated);
                assert_eq!(outcome.rows[0][1], None); assert_eq!(outcome.rows[0][2].as_deref(), Some(""));
                let outcome = run(&driver, "SELECT repeat('界',600)").await;
                assert!(outcome.truncated);
                assert_eq!(outcome.rows[0][0].as_ref().unwrap().chars().count(),512);
                let sql = format!("SELECT {}", (0..40).map(|i| format!("{i} AS c{i}")).collect::<Vec<_>>().join(","));
                let outcome = run(&driver,&sql).await;
                assert_eq!(outcome.columns.len(),32);
                assert_eq!(outcome.rows[0].len(),32);
                assert!(outcome.truncated);
                let (_send, cancel) = flume::bounded(1);
                assert!(driver.execute_administration(&format!("CREATE TABLE \"{schema}\".escaped(id int); COMMIT;"), cancel).await.is_err());
                assert_eq!(run(&driver, &format!("SELECT to_regclass('\"{schema}\".escaped') IS NULL")).await.rows[0][0].as_deref(), Some("t"));
                let mut readonly = config.clone(); readonly.read_only = true;
                let readonly = PostgresDriver::new(readonly);
                assert!(!readonly.administration_catalog(AdminFamily::Roles, 0).await.unwrap().objects.is_empty());
                let (_send, cancel) = flume::bounded(1);
                assert!(readonly.execute_administration("SELECT 1", cancel).await.is_err());
                let (send, cancel) = flume::bounded(1); send.send(()).unwrap();
                assert!(driver.execute_administration("SELECT pg_sleep(10)", cancel).await.unwrap_err().to_string().contains("canceled"));
                // Native privilege failure must preserve catalog state.
                let password = uuid::Uuid::new_v4().simple().to_string();
                run(&driver, &format!("ALTER ROLE \"{role}\" LOGIN PASSWORD '{password}'")).await;
                let mut restricted = config.clone(); restricted.user = role.clone(); restricted.password = Some(password);
                let restricted = PostgresDriver::new(restricted);
                let (_send, cancel) = flume::bounded(1);
                let error = restricted.execute_administration(&format!("CREATE TABLE \"{schema}\".denied(id int)"), cancel).await.unwrap_err();
                assert!(format!("{error:#}").contains("permission denied"));
                assert_eq!(run(&driver, &format!("SELECT to_regclass('\"{schema}\".denied') IS NULL")).await.rows[0][0].as_deref(), Some("t"));
                // Wait on server-observed PgSleep, not a timing assumption.
                run(&driver, &format!("CREATE PROCEDURE \"{schema}\".slow() LANGUAGE plpgsql AS $$ BEGIN INSERT INTO \"{schema}\".t VALUES(5); PERFORM pg_sleep(10); END $$")).await;
                let mut cancellable = config.clone(); cancellable.application_name = Some(format!("cancel_{suffix}"));
                let cancellable = PostgresDriver::new(cancellable);
                let (send, cancel) = flume::bounded(1);
                let sql = format!("CALL \"{schema}\".slow()");
                let task = tokio::spawn(async move { cancellable.execute_administration(&sql,cancel).await });
                tokio::time::timeout(Duration::from_secs(5), async {
                    loop {
                        let observed = run(&driver, &format!("SELECT count(*) FROM pg_catalog.pg_stat_activity WHERE application_name='cancel_{suffix}' AND wait_event='PgSleep'")).await;
                        if observed.rows[0][0].as_deref() == Some("1") { break; }
                        tokio::task::yield_now().await;
                    }
                }).await.unwrap();
                send.send(()).unwrap();
                let error = task.await.unwrap().unwrap_err();
                assert!(format!("{error:#}").contains("rolled back"));
                assert_eq!(run(&driver, &format!("SELECT count(*) FROM \"{schema}\".t")).await.rows[0][0].as_deref(), Some("0"));
                anyhow::Ok(())
            }).catch_unwind()).await;
            // Cleanup also runs after assertion panics or a timed-out fixture body.
            run(&driver, &format!("DROP SCHEMA \"{schema}\" CASCADE")).await;
            run(&driver, &format!("DROP ROLE IF EXISTS \"{member}\"")).await;
            run(&driver, &format!("DROP ROLE IF EXISTS \"{role}\"")).await;
            let acl_cleanup = acl_roles.iter().map(|role| format!("DROP ROLE IF EXISTS \"{role}\";")).collect::<String>();
            driver.execute_schema_ddl(&acl_cleanup).await.unwrap();
            match result {
                Ok(Ok(result)) => result.unwrap(),
                Ok(Err(panic)) => std::panic::resume_unwind(panic),
                Err(error) => panic!("administration fixture timed out: {error}"),
            }
    });
}

#[test]
fn administration_is_explicitly_unsupported_for_sqlite() {
    let driver = db_wire::create_driver(ConnectionConfig {
        driver: DbDriverKind::Sqlite,
        database: ":memory:".into(),
        ..Default::default()
    })
    .unwrap();
    assert!(!driver.supports_administration());
    db_wire::get_runtime().block_on(async {
        let error = driver
            .administration_catalog(AdminFamily::Roles, 0)
            .await
            .err()
            .unwrap();
        assert!(error.to_string().contains("not supported"));
        let (_send, cancel) = flume::bounded(1);
        let error = driver
            .execute_administration("SELECT 1", cancel)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("not supported"));
    });
}

#[test]
fn administration_preserves_original_read_only_policy_after_config_mutation() {
    let mut driver = PostgresDriver::new(ConnectionConfig {
        read_only: true,
        ..Default::default()
    });
    driver.config.read_only = false;
    assert!(driver.is_read_only());
    db_wire::get_runtime().block_on(async {
        // A queued cancellation makes this regression independent of the network.
        // The immutable profile policy must deny execution before dispatch.
        let (send, cancel) = flume::bounded(1);
        send.send(()).unwrap();
        let error = driver
            .execute_administration("SELECT 1", cancel)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("read-only"), "{error:#}");
    });
}
