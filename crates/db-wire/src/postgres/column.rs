use super::*;
use crate::DbDriverKind;
use crate::column::{ColumnApplyOutcome, ColumnDefinition, ColumnDraft, column_change_sql};
use crate::sql::SqlDialect;

struct SocketGuard(tokio::task::JoinHandle<()>);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn read(
    client: &Client,
    schema: &str,
    table: &str,
    column: &str,
) -> Result<ColumnDefinition> {
    let row = client.query_opt(
        "SELECT current_database()::text, c.oid::text, a.attnum, a.attname::text,
                format_type(a.atttypid,a.atttypmod), NOT a.attnotnull,
                CASE WHEN a.attgenerated = '' THEN pg_get_expr(d.adbin,d.adrelid) END,
                col_description(c.oid,a.attnum), a.attidentity::text, a.attgenerated::text,
                CASE WHEN a.attgenerated <> '' THEN pg_get_expr(d.adbin,d.adrelid) END,
                CASE WHEN a.attcollation <> 0 THEN quote_ident(cn.nspname)||'.'||quote_ident(co.collname) END,
                a.attstorage::text, a.attislocal,
                (c.relkind = 'r' AND NOT c.relispartition AND a.attinhcount = 0
                 AND NOT EXISTS(SELECT 1 FROM pg_inherits i WHERE i.inhparent=c.oid OR i.inhrelid=c.oid)
                 AND a.attidentity='' AND a.attgenerated=''
                 AND pg_has_role(c.relowner,'USAGE'))
         FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
         JOIN pg_attribute a ON a.attrelid=c.oid
         LEFT JOIN pg_attrdef d ON d.adrelid=c.oid AND d.adnum=a.attnum
         LEFT JOIN pg_collation co ON co.oid=a.attcollation
         LEFT JOIN pg_namespace cn ON cn.oid=co.collnamespace
         WHERE n.nspname=$1 AND c.relname=$2 AND a.attname=$3
           AND a.attnum>0 AND NOT a.attisdropped", &[&schema,&table,&column]
    ).await.map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?
        .ok_or_else(|| anyhow::anyhow!("Column no longer exists; refresh the catalog"))?;
    Ok(ColumnDefinition {
        database: row.get(0), schema: schema.into(), table: table.into(),
        relation_oid: row.get(1), attribute: row.get(2), name: row.get(3),
        native_type: row.get(4), nullable: row.get(5), default: row.get(6),
        comment: row.get(7), identity: row.get(8), generated: row.get(9),
        generation_expression: row.get(10), collation: row.get(11), storage: row.get(12),
        local: row.get(13), editable: row.get(14),
        restriction: "Editing requires an owned ordinary local column outside inheritance/partitions; identity/generated columns and specialized properties are read-only".into(),
    })
}
impl PostgresDriver {
    pub(super) async fn read_column_definition(
        &self,
        schema: &str,
        table: &str,
        name: &str,
    ) -> Result<ColumnDefinition> {
        let (client, task) = Self::connect_with_task(self.config.clone()).await?;
        let _guard = SocketGuard(task);
        client
            .batch_execute("SET search_path = pg_catalog; SET statement_timeout = '15s'")
            .await?;
        read(&client, schema, table, name).await
    }
    pub(super) async fn reload_column_identity(
        &self,
        original: &ColumnDefinition,
    ) -> Result<ColumnDefinition> {
        let (client, task) = Self::connect_with_task(self.config.clone()).await?;
        let _guard = SocketGuard(task);
        client
            .batch_execute("SET search_path=pg_catalog; SET statement_timeout='15s'")
            .await?;
        let row = client.query_opt(
            "SELECT n.nspname::text,c.relname::text,a.attname::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace JOIN pg_attribute a ON a.attrelid=c.oid WHERE c.oid::text=$1 AND a.attnum=$2 AND NOT a.attisdropped AND current_database()::text=$3",
            &[&original.relation_oid, &original.attribute, &original.database]).await?
            .ok_or_else(|| anyhow::anyhow!("Original column was dropped or database identity changed"))?;
        read(
            &client,
            &row.get::<_, String>(0),
            &row.get::<_, String>(1),
            &row.get::<_, String>(2),
        )
        .await
    }
    pub(super) async fn column_type_choices(&self) -> Result<Vec<String>> {
        let client = self.get_or_connect().await?;
        Ok(client
            .query(
                "SELECT quote_ident(n.nspname)||'.'||quote_ident(t.typname)
             FROM pg_type t JOIN pg_namespace n ON n.oid=t.typnamespace
             WHERE t.typtype IN ('b','d','e') AND t.typelem=0
             AND n.nspname NOT IN ('information_schema') ORDER BY n.nspname,t.typname LIMIT 2000",
                &[],
            )
            .await?
            .into_iter()
            .map(|r| r.get(0))
            .collect())
    }
    pub(super) async fn persist_column(
        &self,
        original: &ColumnDefinition,
        draft: &ColumnDraft,
    ) -> ColumnApplyOutcome {
        let statements = match column_change_sql(original, draft) {
            Ok(sql) if !sql.is_empty() => sql,
            Ok(_) => return ColumnApplyOutcome::Rejected("No column changes".into()),
            Err(e) => return ColumnApplyOutcome::Rejected(e.to_string()),
        };
        let (client, task) = match Self::connect_with_task(self.config.clone()).await {
            Ok(value) => value,
            Err(e) => return ColumnApplyOutcome::Rejected(format!("{e:#}")),
        };
        let _guard = SocketGuard(task);
        let applied = async {
            client.batch_execute("SET search_path=pg_catalog; SET standard_conforming_strings=on; SET lock_timeout='3s'; SET statement_timeout='15s'; BEGIN").await?;
            let table = SqlDialect(DbDriverKind::Postgres)
                .qualified_table(&original.schema, &original.table);
            client
                .batch_execute(&format!("LOCK TABLE ONLY {table} IN ACCESS EXCLUSIVE MODE"))
                .await?;
            let current = read(&client, &original.schema, &original.table, &original.name).await?;
            if &current != original {
                anyhow::bail!("Column changed concurrently. Refresh and review a new draft");
            }
            // Ask PostgreSQL to parse native declarations/expressions, never execute a
            // SELECT merely to validate a default or conversion expression.
            if original.native_type != draft.native_type {
                client
                    .prepare(&format!("SELECT NULL::{}", draft.native_type))
                    .await?;
            }
            for sql in &statements {
                client
                    .batch_execute(sql)
                    .await
                    .map_err(|e| anyhow::anyhow!(pg_error_message(&e)))?;
            }
            Ok::<_, anyhow::Error>(())
        };
        match tokio::time::timeout(std::time::Duration::from_secs(30), applied).await {
            Ok(Ok(())) => {}
            result => {
                let error = match result {
                    Ok(Err(e)) => format!("{e:#}"),
                    _ => "Schema change timed out before commit".into(),
                };
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    client.batch_execute("ROLLBACK"),
                )
                .await;
                return ColumnApplyOutcome::Rejected(error);
            }
        }
        if let Err(e) = client.batch_execute("COMMIT").await {
            return ColumnApplyOutcome::ReadbackRequired(format!(
                "Commit acknowledgement failed; Refresh before retrying: {}",
                pg_error_message(&e)
            ));
        }
        match read(&client, &original.schema, &original.table, &draft.name).await {
            Ok(definition) => ColumnApplyOutcome::Applied(Box::new(definition)),
            Err(e) => ColumnApplyOutcome::ReadbackRequired(format!(
                "DDL committed, but read-back failed. Refresh required: {e:#}"
            )),
        }
    }
}
