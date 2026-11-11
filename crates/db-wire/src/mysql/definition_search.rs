use super::*;
use crate::objects::{
    DEFINITION_INPUT_LIMIT, DEFINITION_SEARCH_LIMIT, DEFINITION_SOURCE_LIMIT, DefinitionHit,
    DefinitionKind, ObjectSummary,
};

const QUERY_TIMEOUT: Duration = Duration::from_secs(15);

// These identifiers are fixed catalog fields, never user input.
type DefinitionFamily = (
    DefinitionKind,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
);
const FAMILIES: [DefinitionFamily; 3] = [
    (
        DefinitionKind::Routine,
        "ROUTINES",
        "ROUTINE_SCHEMA",
        "ROUTINE_NAME",
        "ROUTINE_TYPE",
        "ROUTINE_DEFINITION",
    ),
    (
        DefinitionKind::Trigger,
        "TRIGGERS",
        "TRIGGER_SCHEMA",
        "TRIGGER_NAME",
        "EVENT_OBJECT_TABLE",
        "ACTION_STATEMENT",
    ),
    (
        DefinitionKind::View,
        "VIEWS",
        "TABLE_SCHEMA",
        "TABLE_NAME",
        "''",
        "VIEW_DEFINITION",
    ),
];

impl MySqlDriver {
    pub(super) async fn find_definitions(
        &self,
        schema: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<Vec<DefinitionHit>>> {
        anyhow::ensure!(
            !schema.is_empty()
                && !needle.trim().is_empty()
                && [schema, needle]
                    .iter()
                    .all(|value| value.len() <= DEFINITION_INPUT_LIMIT && !value.contains('\0')),
            "Enter an exact database name and search text up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes"
        );
        let (schema, needle) = (schema.to_owned(), needle.to_owned());
        self.run_catalog_search("Definition search", cancel, move |conn| {
            Box::pin(async move { query_definitions(conn, &schema, &needle).await.map(Some) })
        })
        .await
    }

    pub(super) async fn run_catalog_search<T: Send + 'static>(
        &self,
        label: &'static str,
        cancel: flume::Receiver<()>,
        query: impl for<'a> FnOnce(
            &'a mut mysql_async::Conn,
        )
            -> std::pin::Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>
        + Send
        + 'static,
    ) -> Result<T> {
        let deadline = tokio::time::sleep(QUERY_TIMEOUT);
        tokio::pin!(deadline);
        let pool = tokio::select! {
            biased;
            _ = cancel.recv_async() => anyhow::bail!("{label} cancelled"),
            _ = &mut deadline => anyhow::bail!("{label} timed out"),
            result = self.get_pool() => result?,
        };
        let deadline_at = deadline.deadline();
        crate::run_on_shared_runtime(async move {
            let timeout = tokio::time::sleep_until(deadline_at);
            tokio::pin!(timeout);
            let mut conn = tokio::select! {
                biased;
                _ = cancel.recv_async() => anyhow::bail!("{label} cancelled"),
                _ = &mut timeout => anyhow::bail!("{label} timed out"),
                result = pool.get_conn() => result?,
            };
            let reason = tokio::select! {
                biased;
                _ = cancel.recv_async() => "cancelled",
                _ = &mut timeout => "timed out",
                result = query(&mut conn) => return result,
            };
            // Keep the server ID reserved and bypass the pool so cancellation
            // can proceed even when every pooled connection is occupied.
            let kill = script::kill_connection(conn.opts().clone(), conn.id()).await;
            // Mark the interrupted connection terminal even when the server
            // could not confirm the kill. Recycling could drain the blocked query.
            let _ = tokio::time::timeout(script::DISCONNECT_TIMEOUT, conn.disconnect()).await;
            match kill {
                Ok(()) => anyhow::bail!("{label} {reason}"),
                Err(error) => {
                    anyhow::bail!("{label} {reason}. Server cancellation failed: {error}")
                }
            }
        })
        .await
    }
}

async fn query_definitions(
    conn: &mut mysql_async::Conn,
    schema: &str,
    needle: &str,
) -> Result<Vec<DefinitionHit>> {
    let mut hits = Vec::new();
    for (kind, catalog, schema_field, name, qualifier, source) in FAMILIES {
        let remaining = DEFINITION_SEARCH_LIMIT + 1 - hits.len();
        if remaining == 0 {
            break;
        }
        let sql = format!(
            "SELECT /*+ MAX_EXECUTION_TIME(10000) */ {name}, {qualifier},
                    CASE WHEN OCTET_LENGTH({source}) <= ? THEN {source} END
             FROM information_schema.{catalog}
             WHERE {schema_field} = ?
               AND CAST({schema_field} AS BINARY) = CAST(? AS BINARY)
               AND LOCATE(
                   CAST(LOWER(CONVERT(? USING utf8mb4)) AS BINARY),
                   CAST(LOWER(CONVERT({source} USING utf8mb4)) AS BINARY)
               ) > 0
             ORDER BY CAST({name} AS BINARY), 2 LIMIT ?"
        );
        let rows: Vec<(String, String, Option<String>)> = conn
            .exec(
                sql,
                (
                    DEFINITION_SOURCE_LIMIT,
                    schema,
                    schema,
                    needle,
                    remaining as u64,
                ),
            )
            .await?;
        for (name, qualifier, source) in rows {
            hits.push(DefinitionHit {
                kind,
                object: ObjectSummary {
                    id: (hits.len() + 1) as u32,
                    schema: schema.into(),
                    name,
                    qualifier,
                },
                source,
            });
        }
    }
    Ok(hits)
}
