use super::*;
use mem_engine::{QueryEditColumn, QueryEditOrigin, QueryResultPager};
use tokio_postgres::{Statement, types::ToSql};

const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

fn direct_target(sql: &str) -> Option<&str> {
    let mut words = sql.trim().trim_end_matches(';').split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("select")
        || words.next()? != "*"
        || !words.next()?.eq_ignore_ascii_case("from")
    {
        return None;
    }
    let target = words.next()?;
    words.next().is_none().then_some(target)
}

fn target_matches(source: &str, schema: &str, table: &str) -> bool {
    fn forms(name: &str) -> Vec<String> {
        let mut forms = vec![crate::sql::quote_ident(crate::DbDriverKind::Postgres, name)];
        if name
            .as_bytes()
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || *b == b'_')
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            forms.push(name.into());
        }
        forms
    }
    forms(schema).iter().any(|schema| {
        forms(table)
            .iter()
            .any(|table| source == format!("{schema}.{table}"))
    })
}

async fn inspect(
    client: &impl tokio_postgres::GenericClient,
    oid: u32,
) -> Result<Option<QueryEditOrigin>> {
    let rows = client.query(
        "SELECT pg_catalog.current_database()::text, current_user::text, n.nspname::text, c.relname::text,
                a.attnum, a.attname::text, a.atttypid, a.attgenerated = '' AND a.attidentity = '',
                COALESCE(a.attnum = ANY(i.indkey[0:i.indnkeyatts-1]), false)
           FROM pg_catalog.pg_class c
           JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
           JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid
           LEFT JOIN pg_catalog.pg_index i ON i.indrelid = c.oid AND i.indisprimary AND i.indisvalid
          WHERE c.oid = $1 AND c.relkind = 'r' AND c.relpersistence = 'p' AND NOT c.relrowsecurity
            AND NOT EXISTS (SELECT FROM pg_catalog.pg_trigger t WHERE t.tgrelid = c.oid AND NOT t.tgisinternal)
            AND NOT EXISTS (SELECT FROM pg_catalog.pg_rewrite r WHERE r.ev_class = c.oid)
            AND current_user = session_user AND a.attnum > 0 AND NOT a.attisdropped
            AND NOT EXISTS (SELECT FROM pg_catalog.pg_inherits h WHERE h.inhparent = c.oid OR h.inhrelid = c.oid)
          ORDER BY a.attnum",
        &[&oid],
    ).await?;
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    let columns = rows
        .iter()
        .map(|row| QueryEditColumn {
            attribute: row.get(4),
            name: row.get(5),
            type_oid: row.get(6),
            writable: row.get::<_, bool>(7) && scalar_type(row.get(6)).is_some(),
            key: row.get(8),
        })
        .collect::<Vec<_>>();
    if !columns.iter().any(|column| column.key)
        || columns
            .iter()
            .any(|column| column.key && scalar_type(column.type_oid).is_none())
    {
        return Ok(None);
    }
    Ok(Some(QueryEditOrigin {
        database: first.get(0),
        role: first.get(1),
        schema: first.get(2),
        table: first.get(3),
        oid,
        columns,
    }))
}

pub(super) async fn origin(
    client: &Client,
    sql: &str,
    statement: &Statement,
) -> Option<QueryEditOrigin> {
    let target = direct_target(sql)?;
    let columns = statement.columns();
    let oid = columns.first()?.table_oid()?;
    if columns.iter().any(|column| column.table_oid() != Some(oid)) {
        return None;
    }
    let origin = inspect(client, oid).await.ok()??;
    if !target_matches(target, &origin.schema, &origin.table)
        || columns.len() != origin.columns.len()
        || columns.iter().zip(&origin.columns).any(|(column, meta)| {
            column.name() != meta.name
                || column.column_id() != Some(meta.attribute)
                || column.type_().oid() != meta.type_oid
        })
    {
        return None;
    }
    Some(origin)
}

fn scalar_type(oid: u32) -> Option<&'static str> {
    use tokio_postgres::types::Type;
    [
        (&Type::BOOL, "bool"),
        (&Type::INT2, "int2"),
        (&Type::INT4, "int4"),
        (&Type::INT8, "int8"),
        (&Type::TEXT, "text"),
        (&Type::VARCHAR, "varchar"),
        (&Type::UUID, "uuid"),
    ]
    .into_iter()
    .find_map(|(ty, name)| (oid == ty.oid()).then_some(name))
}

pub(super) async fn load(config: ConnectionConfig, sql: String) -> Result<Arc<QueryResultPager>> {
    anyhow::ensure!(
        direct_target(&sql).is_some(),
        "Use SELECT * FROM schema.table without clauses for editable results"
    );
    tokio::time::timeout(WRITE_TIMEOUT, async move {
        let (client, task) = PostgresDriver::connect_with_task(config).await?;
        let _socket = SocketGuard(task);
        client
            .batch_execute("SET statement_timeout = '10s'")
            .await?;
        let (tx, rx) = flume::unbounded();
        stream_client(
            &client,
            &sql,
            tx,
            Instant::now(),
            &Arc::default(),
            None,
            true,
        )
        .await?;
        let pager = rx
            .drain()
            .find_map(|event| match event {
                StreamEvent::SchemaReady(pager) => Some(pager),
                _ => None,
            })
            .context("Query returned no result")?;
        anyhow::ensure!(
            pager.query_edit_origin.is_some(),
            "This query has no supported writable origin and complete primary key"
        );
        Ok(pager)
    })
    .await
    .context("Editable query deadline expired")?
}

pub(super) async fn save(
    config: ConnectionConfig,
    pager: Arc<QueryResultPager>,
    mut edits: Vec<crate::QueryCellUpdate>,
) -> Result<()> {
    anyhow::ensure!(
        !edits.is_empty() && edits.len() <= 1024,
        "Save supports 1–1,024 edited cells"
    );
    anyhow::ensure!(
        edits
            .iter()
            .filter_map(|edit| edit.value.as_ref())
            .map(String::len)
            .sum::<usize>()
            <= 1024 * 1024,
        "Edited values exceed 1 MiB"
    );
    tokio::time::timeout(WRITE_TIMEOUT, async move {
        let origin = pager.query_edit_origin.as_ref().context("Missing native result origin")?;
        let (mut client, task) = PostgresDriver::connect_with_task(config).await?;
        let _socket = SocketGuard(task);
        let transaction = client.transaction().await?;
        transaction.batch_execute("SET LOCAL lock_timeout = '3s'; SET LOCAL statement_timeout = '10s'; SET LOCAL search_path TO pg_catalog").await?;
        let dialect = crate::sql::SqlDialect(crate::DbDriverKind::Postgres);
        let table = dialect.qualified_table(&origin.schema, &origin.table);
        transaction.batch_execute(&format!("LOCK TABLE ONLY {table} IN ROW EXCLUSIVE MODE")).await?;
        anyhow::ensure!(inspect(&transaction, origin.oid).await?.as_ref() == Some(origin),
            "Query result source or primary key changed; rerun the query");
        anyhow::ensure!(!edits.is_empty() && edits.len() <= 1024, "Save supports 1–1,024 edited cells");
        edits.sort_by_key(|edit| (edit.row, edit.column));
        anyhow::ensure!(edits.windows(2).all(|pair| (pair[0].row, pair[0].column) != (pair[1].row, pair[1].column)),
            "Duplicate edited cell");
        let mut bytes = 0usize;
        for edit in edits {
            let column = origin.columns.get(edit.column).filter(|column| column.writable && !column.key)
                .context("Column is read-only")?;
            let (page, row) = pager.locate_row(edit.row).context("Original result row is unavailable")?;
            let page = pager.get_page(page)?;
            let original = |column| -> Result<Option<String>> {
                page.get_cell(row, column).map(|bytes| {
                    anyhow::ensure!(bytes.len() <= 1024 * 1024, "Original cell exceeds 1 MiB");
                    Ok(std::str::from_utf8(bytes)?.to_owned())
                }).transpose()
            };
            let parameter = |index, oid| -> Result<String> {
                Ok(format!("(${index}::pg_catalog.text)::pg_catalog.{}", scalar_type(oid).context("Unsupported edit type")?))
            };
            let mut values = vec![edit.value, original(edit.column)?];
            let name = dialect.quote_ident(&column.name);
            let prior = parameter(2, column.type_oid)?;
            let old_value = if matches!(column.type_oid, 25 | 1043) {
                format!("{name}::pg_catalog.text COLLATE pg_catalog.\"C\" IS NOT DISTINCT FROM {prior}::pg_catalog.text COLLATE pg_catalog.\"C\"")
            } else {
                format!("{name} IS NOT DISTINCT FROM {prior}")
            };
            let mut predicates = vec![old_value];
            for (index, key) in origin.columns.iter().enumerate().filter(|(_, column)| column.key) {
                values.push(Some(original(index)?.context("Primary key was NULL")?));
                predicates.push(format!("{} = {}", dialect.quote_ident(&key.name), parameter(values.len(), key.type_oid)?));
            }
            bytes = bytes.saturating_add(values.iter().flatten().map(String::len).sum::<usize>());
            anyhow::ensure!(bytes <= 1024 * 1024, "Edited values and keys exceed 1 MiB");
            let sql = format!("UPDATE ONLY {table} SET {name} = {} WHERE {}", parameter(1, column.type_oid)?, predicates.join(" AND "));
            let args: Vec<&(dyn ToSql + Sync)> = values.iter().map(|value| value as _).collect();
            anyhow::ensure!(transaction.execute(&sql, &args).await.map_err(|error| anyhow::anyhow!(pg_error_message(&error)))? == 1,
                "Original row changed or is no longer writable; no edits were saved");
        }
        transaction.commit().await.context("Commit was not acknowledged; verify stored values before retrying")
    }).await.context("Query edit deadline expired; verify stored values before retrying")?
}

#[cfg(test)]
mod tests {
    use super::{direct_target, target_matches};
    #[test]
    fn only_plain_qualified_selects_match_the_native_target() {
        assert_eq!(
            direct_target("select * FROM public.items;"),
            Some("public.items")
        );
        for sql in [
            "SELECT id FROM public.items",
            "SELECT * FROM public.items WHERE id=1",
            "SELECT * FROM public.items a",
            "SELECT * FROM public.items UNION SELECT * FROM public.items",
        ] {
            assert_eq!(direct_target(sql), None);
        }
        assert!(target_matches("public.\"Items\"", "public", "Items"));
        assert!(!target_matches("public.items", "public", "Items"));
        assert!(!target_matches("items", "public", "items"));
    }
}
