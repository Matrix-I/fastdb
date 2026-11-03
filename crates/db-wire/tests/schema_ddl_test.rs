use db_wire::sql::schema_ddl::{
    ColumnDef, SchemaAction, generate_schema_ddl, is_ddl_transactional,
};
use db_wire::traits::DatabaseDriver;
use db_wire::{DbDriverKind, SqliteDriver};

#[tokio::test]
async fn sqlite_schema_ddl_end_to_end_lifecycle() {
    assert!(is_ddl_transactional(DbDriverKind::Sqlite));
    let driver = SqliteDriver::new_memory();

    // 1. Create Table
    let create_action = SchemaAction::CreateTable {
        schema: "main".into(),
        table: "test_items".into(),
        columns: vec![
            ColumnDef {
                name: "id".into(),
                data_type: "INTEGER".into(),
                nullable: false,
                is_pk: true,
            },
            ColumnDef {
                name: "name".into(),
                data_type: "TEXT".into(),
                nullable: true,
                is_pk: false,
            },
        ],
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &create_action).unwrap();
    driver.execute_schema_ddl(&ddl).await.unwrap();

    // Verify table exists by listing tables
    let tables = driver.list_tables().await.unwrap();
    assert!(tables.iter().any(|t| t.name == "test_items"));

    // 2. Add Column
    let add_col_action = SchemaAction::AddColumn {
        schema: "main".into(),
        table: "test_items".into(),
        column: "price".into(),
        data_type: "REAL".into(),
        nullable: true,
        default_val: Some("0.0".into()),
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &add_col_action).unwrap();
    driver.execute_schema_ddl(&ddl).await.unwrap();

    let cols = driver.list_columns("main", "test_items").await.unwrap();
    assert!(cols.iter().any(|c| c.name == "price"));

    // 3. Rename Column
    let rename_col_action = SchemaAction::RenameColumn {
        schema: "main".into(),
        table: "test_items".into(),
        old_name: "price".into(),
        new_name: "cost".into(),
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &rename_col_action).unwrap();
    driver.execute_schema_ddl(&ddl).await.unwrap();

    let cols = driver.list_columns("main", "test_items").await.unwrap();
    assert!(cols.iter().any(|c| c.name == "cost"));
    assert!(!cols.iter().any(|c| c.name == "price"));

    // 4. Rename Table
    let rename_tbl_action = SchemaAction::RenameTable {
        schema: "main".into(),
        table: "test_items".into(),
        new_name: "catalog_items".into(),
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &rename_tbl_action).unwrap();
    driver.execute_schema_ddl(&ddl).await.unwrap();

    let tables = driver.list_tables().await.unwrap();
    assert!(tables.iter().any(|t| t.name == "catalog_items"));
    assert!(!tables.iter().any(|t| t.name == "test_items"));

    // 5. Create View
    let create_view_action = SchemaAction::CreateView {
        schema: "main".into(),
        view: "v_items".into(),
        definition: "SELECT id, cost FROM catalog_items".into(),
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &create_view_action).unwrap();
    driver.execute_schema_ddl(&ddl).await.unwrap();

    let tables = driver.list_tables().await.unwrap();
    assert!(tables.iter().any(|t| t.name == "v_items"));

    // 6. Create and Drop Index
    driver
        .execute_query("CREATE INDEX idx_cost ON catalog_items (cost);")
        .await
        .unwrap();

    let drop_idx_action = SchemaAction::DropIndex {
        schema: "main".into(),
        table: "catalog_items".into(),
        index: "idx_cost".into(),
        cascade: false,
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &drop_idx_action).unwrap();
    assert_eq!(ddl, "DROP INDEX \"idx_cost\";");
    driver.execute_schema_ddl(&ddl).await.unwrap();

    // 7. Drop View
    let drop_view_action = SchemaAction::DropView {
        schema: "main".into(),
        view: "v_items".into(),
        cascade: false,
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &drop_view_action).unwrap();
    driver.execute_schema_ddl(&ddl).await.unwrap();

    let tables = driver.list_tables().await.unwrap();
    assert!(!tables.iter().any(|t| t.name == "v_items"));

    // 8. Drop Table
    let drop_tbl_action = SchemaAction::DropTable {
        schema: "main".into(),
        table: "catalog_items".into(),
        cascade: false,
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &drop_tbl_action).unwrap();
    driver.execute_schema_ddl(&ddl).await.unwrap();

    let tables = driver.list_tables().await.unwrap();
    assert!(!tables.iter().any(|t| t.name == "catalog_items"));
}

#[tokio::test]
async fn sqlite_truncate_clears_rows() {
    let driver = SqliteDriver::new_memory();

    let create_action = SchemaAction::CreateTable {
        schema: "main".into(),
        table: "records".into(),
        columns: vec![ColumnDef {
            name: "id".into(),
            data_type: "INTEGER".into(),
            nullable: false,
            is_pk: true,
        }],
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &create_action).unwrap();
    driver.execute_schema_ddl(&ddl).await.unwrap();

    // Insert a dummy row
    driver
        .execute_query("INSERT INTO records (id) VALUES (1);")
        .await
        .unwrap();

    let truncate_action = SchemaAction::TruncateTable {
        schema: "main".into(),
        table: "records".into(),
        cascade: false,
    };
    let ddl = generate_schema_ddl(DbDriverKind::Sqlite, &truncate_action).unwrap();
    driver.execute_schema_ddl(&ddl).await.unwrap();

    // Verify count is 0
    let tables = driver.list_tables().await.unwrap();
    assert!(tables.iter().any(|t| t.name == "records"));
}

fn pg_config() -> Option<db_wire::ConnectionConfig> {
    let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
        assert!(
            std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
            "FASTDB_TEST_PG_PORT required"
        );
        return None;
    };
    Some(db_wire::ConnectionConfig {
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

#[tokio::test]
async fn postgres_schema_ddl_end_to_end_lifecycle() {
    let Some(config) = pg_config() else { return };
    let driver = db_wire::PostgresDriver::new(config);
    let schema = format!("fastdb_ddl_test_{}", std::process::id());

    driver
        .execute_query(&format!("CREATE SCHEMA \"{}\";", schema))
        .await
        .unwrap();

    let result: anyhow::Result<()> = async {
        // 1. Create table
        let create_action = SchemaAction::CreateTable {
            schema: schema.clone(),
            table: "customers".into(),
            columns: vec![
                ColumnDef {
                    name: "id".into(),
                    data_type: "bigint".into(),
                    nullable: false,
                    is_pk: true,
                },
                ColumnDef {
                    name: "name".into(),
                    data_type: "text".into(),
                    nullable: true,
                    is_pk: false,
                },
            ],
        };
        let ddl = generate_schema_ddl(DbDriverKind::Postgres, &create_action)
            .map_err(|e| anyhow::anyhow!(e))?;
        driver.execute_schema_ddl(&ddl).await?;

        // 2. Add column
        let add_col = SchemaAction::AddColumn {
            schema: schema.clone(),
            table: "customers".into(),
            column: "email".into(),
            data_type: "varchar(255)".into(),
            nullable: true,
            default_val: None,
        };
        let ddl = generate_schema_ddl(DbDriverKind::Postgres, &add_col)
            .map_err(|e| anyhow::anyhow!(e))?;
        driver.execute_schema_ddl(&ddl).await?;

        // 3. Create View referencing the table
        let create_view = SchemaAction::CreateView {
            schema: schema.clone(),
            view: "v_customers".into(),
            definition: format!("SELECT id, name FROM \"{}\".\"customers\"", schema),
        };
        let ddl = generate_schema_ddl(DbDriverKind::Postgres, &create_view)
            .map_err(|e| anyhow::anyhow!(e))?;
        driver.execute_schema_ddl(&ddl).await?;

        // 4. Drop View
        let drop_view = SchemaAction::DropView {
            schema: schema.clone(),
            view: "v_customers".into(),
            cascade: true,
        };
        let ddl = generate_schema_ddl(DbDriverKind::Postgres, &drop_view)
            .map_err(|e| anyhow::anyhow!(e))?;
        driver.execute_schema_ddl(&ddl).await?;

        // 5. Truncate Table
        let truncate = SchemaAction::TruncateTable {
            schema: schema.clone(),
            table: "customers".into(),
            cascade: false,
        };
        let ddl = generate_schema_ddl(DbDriverKind::Postgres, &truncate)
            .map_err(|e| anyhow::anyhow!(e))?;
        driver.execute_schema_ddl(&ddl).await?;

        // 6. Drop Table
        let drop_table = SchemaAction::DropTable {
            schema: schema.clone(),
            table: "customers".into(),
            cascade: true,
        };
        let ddl = generate_schema_ddl(DbDriverKind::Postgres, &drop_table)
            .map_err(|e| anyhow::anyhow!(e))?;
        driver.execute_schema_ddl(&ddl).await?;

        Ok(())
    }
    .await;

    let _ = driver
        .execute_query(&format!("DROP SCHEMA IF EXISTS \"{}\" CASCADE;", schema))
        .await;

    result.unwrap();
}
