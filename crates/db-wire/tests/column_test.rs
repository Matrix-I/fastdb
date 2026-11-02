use db_wire::column::{ColumnApplyOutcome, ColumnDraft, column_change_sql};
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, PostgresDriver};

fn config() -> Option<ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        return None;
    };
    Some(ConnectionConfig {
        driver: DbDriverKind::Postgres,
        host: std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: port.parse().unwrap(),
        database: "postgres".into(),
        user: std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_| "postgres".into()),
        password: std::env::var("FASTDB_TEST_PG_PASSWORD").ok(),
        use_unix_socket: false,
        ..Default::default()
    })
}
#[test]
fn column_drafts_apply_atomically_detect_conflicts_and_preserve_sessions() {
    let Some(config) = config() else {
        return;
    };
    db_wire::get_runtime().block_on(async{
        let driver=PostgresDriver::new(config);
        let schema=format!("fastdb_columns_{}",std::process::id());
        driver.execute_query(&format!("CREATE SCHEMA {schema}")).await.unwrap();
        for sql in [
            format!("CREATE TABLE {schema}.customer (host varchar(50) DEFAULT 'old', note text, id integer GENERATED ALWAYS AS IDENTITY, length integer GENERATED ALWAYS AS (length(host)) STORED)"),
            format!("CREATE TABLE {schema}.isolated (id int)"),
            format!("CREATE TABLE {schema}.plain (host varchar(50) DEFAULT 'old', note text)"),
            format!("INSERT INTO {schema}.plain VALUES (NULL,NULL),('abc','x')"),
            format!("CREATE TABLE {schema}.parent (id int)"),
            format!("CREATE TABLE {schema}.child () INHERITS ({schema}.parent)"),
        ]{driver.execute_query(&sql).await.unwrap();}
        let original=driver.load_column(&schema,"plain","host").await.unwrap();
        assert!(original.editable);
        assert!(original.native_type.contains("(50)"));
        assert!(!driver.list_column_types().await.unwrap().is_empty());
        let mut draft=ColumnDraft::from(&original);
        draft.name="odd\"name".into();
        draft.default=Some("NULL".into());
        draft.comment=Some("Tiếng Việt 'comment'\\path\nsecond line".into());
        let sql=column_change_sql(&original,&draft).unwrap();
        assert!(sql.last().unwrap().contains("RENAME COLUMN"));
        let applied=match driver.apply_column(&original,&draft).await{
            ColumnApplyOutcome::Applied(definition)=>*definition,
            other=>panic!("{other:?}"),
        };
        assert_eq!(applied.name,draft.name);
        assert_eq!(applied.comment,draft.comment);
        assert!(applied.default.is_some());
        assert_eq!(applied.attribute,original.attribute);
        assert_eq!(driver.reload_column(&original).await.unwrap(),applied);

        // Existing NULLs reject NOT NULL. All changes, including a comment, roll back.
        let mut invalid=ColumnDraft::from(&applied);
        invalid.nullable=false;invalid.comment=Some("must roll back".into());
        assert!(matches!(driver.apply_column(&applied,&invalid).await,ColumnApplyOutcome::Rejected(_)));
        assert_eq!(driver.reload_column(&applied).await.unwrap(),applied);
        let mut invalid=ColumnDraft::from(&applied);
        invalid.native_type="integer".into();invalid.using_expression=r#""odd""name"::integer"#.into();
        assert!(matches!(driver.apply_column(&applied,&invalid).await,ColumnApplyOutcome::Rejected(_)));
        assert_eq!(driver.reload_column(&applied).await.unwrap(),applied);

        // Concurrent catalog changes require a fresh snapshot.
        driver.execute_query(&format!("COMMENT ON COLUMN {schema}.plain.\"odd\"\"name\" IS 'concurrent'")).await.unwrap();
        let mut conflict=ColumnDraft::from(&applied);conflict.name="host2".into();
        assert!(matches!(driver.apply_column(&applied,&conflict).await,ColumnApplyOutcome::Rejected(_)));
        let current=driver.reload_column(&original).await.unwrap();
        assert_eq!(current.comment.as_deref(),Some("concurrent"));

        let session=driver.open_script_session().await.unwrap();
        for sql in ["BEGIN".into(),format!("INSERT INTO {schema}.isolated VALUES (1)")]{
            let(tx,_rx)=flume::unbounded();session.execute_streaming(&sql,tx).await.unwrap();
        }
        let mut next=ColumnDraft::from(&current);
        next.default=None;
        let next=match driver.apply_column(&current,&next).await{
            ColumnApplyOutcome::Applied(c)=>*c,other=>panic!("{other:?}")
        };
        assert_eq!(next.default,None);
        let(_,rows,_)=driver.execute_query(&format!("SELECT * FROM {schema}.isolated")).await.unwrap();
        assert_eq!(rows,0);
        session.finish().await.unwrap();
        for (table,column) in [("customer","id"),("customer","length"),("parent","id"),("child","id")]{
            let c=driver.load_column(&schema,table,column).await.unwrap();
            assert!(!c.editable,"{table}.{column}");
            let mut d=ColumnDraft::from(&c);d.comment=Some("blocked".into());
            assert!(column_change_sql(&c,&d).is_err());
        }
        // Dropped/recreated names do not acquire the original stable identity.
        driver.execute_query(&format!("ALTER TABLE {schema}.plain DROP COLUMN \"odd\"\"name\"")).await.unwrap();
        driver.execute_query(&format!("ALTER TABLE {schema}.plain ADD COLUMN \"odd\"\"name\" text")).await.unwrap();
        assert!(driver.reload_column(&next).await.is_err());
        let mut d=ColumnDraft::from(&next);d.comment=Some("cannot touch replacement".into());
        assert!(matches!(driver.apply_column(&next,&d).await,ColumnApplyOutcome::Rejected(_)));
        driver.execute_query(&format!("DROP SCHEMA {schema} CASCADE")).await.unwrap();
    });
}
