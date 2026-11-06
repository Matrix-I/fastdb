use crate::{ConstraintDetails, IndexDetails, TableDetails};
use anyhow::Result;
use std::collections::BTreeMap;
use tokio_postgres::Client;

pub(super) async fn columns(
    client: &Client,
    schema: Option<&str>,
    table: Option<&str>,
) -> Result<Vec<(String, String, Vec<crate::ColumnMeta>)>> {
    let rows = async {
        crate::sql_observer::observe!(
            "PostgreSQL",
            r#"
        SELECT col.table_schema, col.table_name, col.column_name, col.data_type,
               col.is_nullable, col.column_default, col.collation_name,
               format_type(a.atttypid, a.atttypmod), col_description(c.oid, a.attnum),
               EXISTS(SELECT 1 FROM pg_constraint k WHERE k.conrelid = c.oid
                      AND k.contype = 'p' AND a.attnum = ANY(k.conkey)),
               EXISTS(SELECT 1 FROM pg_constraint k WHERE k.conrelid = c.oid
                      AND k.contype = 'f' AND a.attnum = ANY(k.conkey))
        FROM information_schema.columns col
        JOIN pg_namespace n ON n.nspname = col.table_schema
        JOIN pg_class c ON c.relnamespace = n.oid AND c.relname = col.table_name
        JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = col.column_name
        WHERE col.table_schema NOT IN ('information_schema', 'pg_catalog')
          AND ($1::text IS NULL OR col.table_schema = $1)
          AND ($2::text IS NULL OR col.table_name = $2)
        ORDER BY col.table_schema, col.table_name, col.ordinal_position
    "#,
            |query| client.query(query, &[&schema, &table],).await,
            |rows| Some(rows.len() as u64)
        )
    }
    .await
    .map_err(|e| {
        let message = super::pg_error_message(&e);
        anyhow::Error::new(e).context(message)
    })?;
    let mut tables = BTreeMap::<(String, String), Vec<crate::ColumnMeta>>::new();
    for row in rows {
        let mut column =
            super::info_schema_column(row.get(2), row.get(3), row.get(4), row.get(5), row.get(6))
                .with_raw_type(row.get::<_, String>(7))
                .with_pk(row.get(9))
                .with_fk(row.get(10));
        column.comment = row.get(8);
        tables
            .entry((row.get(0), row.get(1)))
            .or_default()
            .push(column);
    }
    Ok(tables
        .into_iter()
        .map(|((s, t), columns)| (s, t, columns))
        .collect())
}

// information_schema applies the same visibility rules as list_tables. Catalog
// names are values throughout; user identifiers are never interpolated into SQL.
const TABLES: &str = r#"
SELECT n.nspname::text AS schema, c.relname::text AS name,
       c.oid::text AS oid, pg_get_userbyid(c.relowner)::text AS owner,
       CASE WHEN c.relkind IN ('r', 'p', 'm') THEN ts.spcname::text END AS tablespace,
       obj_description(c.oid, 'pg_class') AS comment,
       CASE c.relpersistence WHEN 'p' THEN 'Permanent'
            WHEN 'u' THEN 'Unlogged' WHEN 't' THEN 'Temporary' END AS persistence,
       CASE WHEN c.relkind IN ('r', 'p') THEN c.relrowsecurity END AS rls,
       CASE WHEN c.relkind IN ('r', 'p') THEN c.relforcerowsecurity END AS force_rls,
       c.relkind = 'p' AS partitioned, pg_get_partkeydef(c.oid) AS partition_key,
       CASE WHEN c.relkind IN ('r', 'p', 'm') AND c.reltuples >= 0
            THEN c.reltuples::bigint END AS estimated_rows,
       CASE WHEN c.relkind IN ('r', 'm') THEN pg_table_size(c.oid) END AS table_bytes,
       CASE WHEN c.relkind IN ('r', 'm') THEN pg_indexes_size(c.oid) END AS index_bytes,
       CASE WHEN c.relkind IN ('r', 'm') THEN pg_total_relation_size(c.oid) END AS total_bytes,
       s.last_vacuum::text, s.last_autovacuum::text,
       s.last_analyze::text, s.last_autoanalyze::text
FROM pg_class c
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN information_schema.tables t ON t.table_schema = n.nspname AND t.table_name = c.relname
JOIN pg_database d ON d.datname = current_database()
LEFT JOIN pg_tablespace ts ON ts.oid = COALESCE(NULLIF(c.reltablespace, 0), d.dattablespace)
LEFT JOIN pg_stat_all_tables s ON s.relid = c.oid
WHERE n.nspname NOT IN ('information_schema', 'pg_catalog')
ORDER BY n.nspname, c.relname
"#;

const INDEXES: &str = r#"
SELECT n.nspname::text, c.relname::text, ic.relname::text, am.amname::text,
       ARRAY(SELECT pg_get_indexdef(i.indexrelid, k, true)
             FROM generate_series(1, i.indnatts) k ORDER BY k),
       i.indisunique, i.indisvalid, pg_get_indexdef(i.indexrelid)
FROM pg_index i
JOIN pg_class c ON c.oid = i.indrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN pg_class ic ON ic.oid = i.indexrelid
JOIN pg_am am ON am.oid = ic.relam
JOIN information_schema.tables t ON t.table_schema = n.nspname AND t.table_name = c.relname
WHERE n.nspname NOT IN ('information_schema', 'pg_catalog')
ORDER BY n.nspname, c.relname, ic.relname
"#;

const CONSTRAINTS: &str = r#"
SELECT n.nspname::text, c.relname::text, con.conname::text,
       CASE con.contype WHEN 'p' THEN 'PRIMARY KEY' WHEN 'f' THEN 'FOREIGN KEY'
            WHEN 'u' THEN 'UNIQUE' WHEN 'c' THEN 'CHECK' WHEN 'x' THEN 'EXCLUDE'
            WHEN 'n' THEN 'NOT NULL' ELSE con.contype::text END,
       ARRAY(SELECT a.attname::text FROM unnest(con.conkey) WITH ORDINALITY k(attnum, pos)
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum ORDER BY k.pos),
       pg_get_constraintdef(con.oid, true), con.convalidated
FROM pg_constraint con
JOIN pg_class c ON c.oid = con.conrelid
JOIN pg_namespace n ON n.oid = c.relnamespace
JOIN information_schema.tables t ON t.table_schema = n.nspname AND t.table_name = c.relname
WHERE n.nspname NOT IN ('information_schema', 'pg_catalog')
ORDER BY n.nspname, c.relname, con.conname
"#;

pub(super) async fn load(client: &Client) -> Result<Vec<(String, String, TableDetails)>> {
    let mut tables = BTreeMap::new();
    for row in async {
        crate::sql_observer::observe!(
            "PostgreSQL",
            TABLES,
            |query| client.query(query, &[]).await,
            |rows| Some(rows.len() as u64)
        )
    }
    .await
    .map_err(|e| {
        let message = super::pg_error_message(&e);
        anyhow::Error::new(e).context(message)
    })? {
        tables.insert(
            (row.get::<_, String>("schema"), row.get::<_, String>("name")),
            TableDetails {
                object_id: row.get("oid"),
                owner: row.get("owner"),
                tablespace: row.get("tablespace"),
                comment: row.get("comment"),
                persistence: row.get("persistence"),
                row_security: row.get("rls"),
                force_row_security: row.get("force_rls"),
                partitioned: row.get("partitioned"),
                partition_key: row.get("partition_key"),
                estimated_rows: row.get("estimated_rows"),
                table_bytes: row.get("table_bytes"),
                index_bytes: row.get("index_bytes"),
                total_bytes: row.get("total_bytes"),
                last_vacuum: row.get("last_vacuum"),
                last_autovacuum: row.get("last_autovacuum"),
                last_analyze: row.get("last_analyze"),
                last_autoanalyze: row.get("last_autoanalyze"),
                ..Default::default()
            },
        );
    }
    for row in async {
        crate::sql_observer::observe!(
            "PostgreSQL",
            INDEXES,
            |query| client.query(query, &[]).await,
            |rows| Some(rows.len() as u64)
        )
    }
    .await
    .map_err(|e| {
        let message = super::pg_error_message(&e);
        anyhow::Error::new(e).context(message)
    })? {
        if let Some(table) = tables.get_mut(&(row.get::<_, String>(0), row.get::<_, String>(1))) {
            table.indexes.push(IndexDetails {
                name: row.get(2),
                method: row.get(3),
                columns: row.get(4),
                unique: row.get(5),
                valid: row.get(6),
                definition: row.get(7),
            });
        }
    }
    for row in async {
        crate::sql_observer::observe!(
            "PostgreSQL",
            CONSTRAINTS,
            |query| client.query(query, &[]).await,
            |rows| Some(rows.len() as u64)
        )
    }
    .await
    .map_err(|e| {
        let message = super::pg_error_message(&e);
        anyhow::Error::new(e).context(message)
    })? {
        if let Some(table) = tables.get_mut(&(row.get::<_, String>(0), row.get::<_, String>(1))) {
            table.constraints.push(ConstraintDetails {
                name: row.get(2),
                kind: row.get(3),
                columns: row.get(4),
                definition: row.get(5),
                validated: row.get(6),
            });
        }
    }
    match async {
        crate::sql_observer::observe!(
            "PostgreSQL",
            include_str!("ddl.sql"),
            |query| client.query(query, &[]).await,
            |rows| Some(rows.len() as u64)
        )
    }
    .await
    {
        Ok(rows) => {
            for row in rows {
                if let Some(table) =
                    tables.get_mut(&(row.get::<_, String>(0), row.get::<_, String>(1)))
                {
                    table.ddl = row.get(2);
                }
            }
        }
        Err(error) => {
            // An older catalog or concurrent schema change must not discard the
            // independently loaded Properties metadata or expose a partial script.
            let message = super::pg_error_message(&error).replace(['\r', '\n'], " ");
            for table in tables.values_mut() {
                table.ddl = Some(format!("-- PostgreSQL DDL unavailable: {message}\n"));
            }
        }
    }
    Ok(tables.into_iter().map(|((s, t), d)| (s, t, d)).collect())
}

#[cfg(test)]
mod permission_tests {
    #[test]
    fn metadata_readers_keep_native_permission_errors() {
        let Ok(port) = std::env::var("FASTDB_TEST_PG_PORT") else {
            assert!(
                std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
                "FASTDB_TEST_PG_PORT required"
            );
            return;
        };
        crate::get_runtime().block_on(async {
            let mut config=tokio_postgres::Config::new();
            config.host(std::env::var("FASTDB_TEST_PG_HOST").unwrap_or_else(|_|"127.0.0.1".into()))
                .port(port.parse().unwrap()).dbname("postgres")
                .user(std::env::var("FASTDB_TEST_PG_USER").unwrap_or_else(|_|"postgres".into()));
            if let Ok(password)=std::env::var("FASTDB_TEST_PG_PASSWORD") { config.password(password); }
            config.connect_timeout(std::time::Duration::from_secs(5));
            let (client,connection)=config.connect(tokio_postgres::NoTls).await.unwrap();
            let task=tokio::spawn(connection);
            let role=format!("fastdb_metadata_denied_{}", std::process::id());
            // Transaction-local catalog ACL changes are invisible to other test sessions.
            client.batch_execute(&format!("BEGIN; SET LOCAL statement_timeout='5s'; CREATE ROLE {role}; REVOKE SELECT ON pg_catalog.pg_attribute, pg_catalog.pg_trigger FROM PUBLIC; SET LOCAL ROLE {role}")).await.unwrap();
            let columns=super::columns(&client,None,None).await;
            client.batch_execute("ROLLBACK").await.unwrap();
            client.batch_execute(&format!("BEGIN; SET LOCAL statement_timeout='5s'; CREATE ROLE {role}; REVOKE SELECT ON pg_catalog.pg_attribute FROM PUBLIC; SET LOCAL ROLE {role}")).await.unwrap();
            let details=super::load(&client).await;
            client.batch_execute("ROLLBACK").await.unwrap();
            client.batch_execute(&format!("BEGIN; SET LOCAL statement_timeout='5s'; CREATE ROLE {role}; REVOKE SELECT ON pg_catalog.pg_trigger FROM PUBLIC; SET LOCAL ROLE {role}")).await.unwrap();
            let catalog=super::super::catalog::query_catalog_rows(&client, super::super::catalog::TRIGGERS, 0).await;
            client.batch_execute("ROLLBACK").await.unwrap();
            drop(client);
            task.abort();
            let error=columns.expect_err("columns must fail without catalog SELECT");
            assert!(crate::metadata::MetadataFailure::from_error(&error).permission_denied,"{error:#}");
            let error=details.expect_err("native details must fail without catalog SELECT");
            assert!(crate::metadata::MetadataFailure::from_error(&error).permission_denied,"{error:#}");
            let error=catalog.expect_err("catalog must fail without catalog SELECT");
            assert!(crate::metadata::MetadataFailure::from_error(&error).permission_denied,"{error:#}");
        });
    }
}
