use super::*;
use crate::objects::{
    DEFINITION_INPUT_LIMIT, DEFINITION_SEARCH_LIMIT, DEFINITION_SOURCE_LIMIT, DefinitionHit,
    DefinitionKind, OBJECT_PAGE_SIZE, ObjectKind, ObjectPage, ObjectSummary,
};
use std::time::Duration;

const DEFINITION_SQL: &str = include_str!("definition_search.sql");
const SEARCH_DEADLINE: Duration = Duration::from_secs(15);
const CANCEL_DEADLINE: Duration = Duration::from_secs(2);

fn discovery(kind: ObjectKind) -> &'static str {
    match kind {
        ObjectKind::Sequence => {
            "SELECT c.oid,n.nspname::text,c.relname::text,''::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relkind='S'"
        }
        ObjectKind::Routine => {
            "SELECT c.oid,n.nspname::text,c.proname::text,pg_get_function_identity_arguments(c.oid) FROM pg_proc c JOIN pg_namespace n ON n.oid=c.pronamespace WHERE c.prokind IN ('f','p')"
        }
        ObjectKind::Trigger => {
            "SELECT c.oid,n.nspname::text,c.tgname::text,r.relname::text FROM pg_trigger c JOIN pg_class r ON r.oid=c.tgrelid JOIN pg_namespace n ON n.oid=r.relnamespace WHERE NOT c.tgisinternal"
        }
        ObjectKind::Type => {
            "SELECT c.oid,n.nspname::text,c.typname::text,c.typtype::text FROM pg_type c JOIN pg_namespace n ON n.oid=c.typnamespace WHERE c.typtype IN ('e','d','c','r','m') AND (c.typrelid=0 OR EXISTS(SELECT 1 FROM pg_class r WHERE r.oid=c.typrelid AND r.relkind='c'))"
        }
    }
}

impl PostgresDriver {
    async fn object_client(&self) -> Result<(Client, SocketGuard)> {
        let (client, task) = Self::connect_with_task(self.config.clone()).await?;
        let guard = SocketGuard(task);
        configure_catalog_read(&client).await?;
        Ok((client, guard))
    }

    pub(super) async fn find_definitions(
        &self,
        schema: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Vec<DefinitionHit>> {
        anyhow::ensure!(
            !schema.is_empty() && schema.len() <= DEFINITION_INPUT_LIMIT,
            "Enter an exact schema name up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes"
        );
        anyhow::ensure!(
            !needle.trim().is_empty() && needle.len() <= DEFINITION_INPUT_LIMIT,
            "Enter search text up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes"
        );
        anyhow::ensure!(
            !schema.contains('\0') && !needle.contains('\0'),
            "Schema and search text cannot contain a NUL character"
        );

        self.run_catalog_search("Definition search", cancel, async |client| {
            query_definitions(client, schema, needle).await
        })
        .await
    }

    pub(super) async fn run_catalog_search<T>(
        &self,
        label: &str,
        cancel: flume::Receiver<()>,
        work: impl for<'a> AsyncFnOnce(&'a Client) -> Result<T>,
    ) -> Result<T> {
        let deadline = tokio::time::sleep(SEARCH_DEADLINE);
        tokio::pin!(deadline);
        let EstablishedConnection {
            client,
            task,
            cancellation,
        } = tokio::select! {
            biased;
            _ = cancel.recv_async() => anyhow::bail!("{label} cancelled"),
            _ = &mut deadline => anyhow::bail!("{label} timed out"),
            result = Self::connect_with_notices(self.config.clone(), Arc::default()) => result?,
        };
        let _socket = SocketGuard(task);
        let reason = tokio::select! {
            biased;
            _ = cancel.recv_async() => "cancelled",
            _ = &mut deadline => "timed out",
            result = async {
                configure_catalog_read(&client).await?;
                work(&client).await
            } => return result,
        };
        match tokio::time::timeout(CANCEL_DEADLINE, cancellation.cancel()).await {
            Ok(Ok(())) => anyhow::bail!("{label} {reason}"),
            Ok(Err(error)) => {
                anyhow::bail!("{label} {reason}. Server cancellation failed: {error}")
            }
            Err(_) => anyhow::bail!("{label} {reason}. Server cancellation timed out"),
        }
    }

    pub(super) async fn object_page(&self, kind: ObjectKind, after: u32) -> Result<ObjectPage> {
        let (client, _guard) = self.object_client().await?;
        let sql = format!(
            "{} AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' AND c.oid>$1 ORDER BY c.oid LIMIT 101",
            discovery(kind)
        );
        let rows = async {
            crate::sql_observer::observe!(
                "PostgreSQL",
                &sql,
                |query| client.query(query, &[&after]).await,
                |rows| Some(rows.len() as u64)
            )
        }
        .await?;
        let more = rows.len() > OBJECT_PAGE_SIZE;
        let objects: Vec<_> = rows
            .into_iter()
            .take(OBJECT_PAGE_SIZE)
            .map(|r| ObjectSummary {
                id: r.get(0),
                schema: r.get(1),
                name: r.get(2),
                qualifier: r.get(3),
            })
            .collect();
        let next = more.then(|| objects.last().unwrap().id);
        Ok(ObjectPage { objects, next })
    }

    pub(super) async fn read_object_source(
        &self,
        kind: ObjectKind,
        object: &ObjectSummary,
    ) -> Result<String> {
        let (client, _guard) = self.object_client().await?;
        let sql = format!("{} AND c.oid=$1", discovery(kind));
        let row = async {
            crate::sql_observer::observe!(
                "PostgreSQL",
                &sql,
                |query| client.query_opt(query, &[&object.id]).await,
                |row| Some(u64::from(row.is_some()))
            )
        }
        .await?
        .ok_or_else(|| anyhow::anyhow!("Object no longer exists; refresh the catalog"))?;
        let current = ObjectSummary {
            id: row.get(0),
            schema: row.get(1),
            name: row.get(2),
            qualifier: row.get(3),
        };
        anyhow::ensure!(current == *object, "Object changed; refresh the catalog");
        let sql = match kind {
            ObjectKind::Routine => "SELECT pg_get_functiondef($1)",
            ObjectKind::Trigger => "SELECT pg_get_triggerdef($1,true)",
            ObjectKind::Sequence => {
                "SELECT format('Type: %s\nStart: %s\nIncrement: %s\nMinimum: %s\nMaximum: %s\nCache: %s\nCycle: %s',format_type(seqtypid,NULL),seqstart,seqincrement,seqmin,seqmax,seqcache,seqcycle) FROM pg_sequence WHERE seqrelid=$1"
            }
            ObjectKind::Type => {
                "SELECT format('Type: %s\nKind: %s\nBase: %s\nNot null: %s\nDefault: %s\nEnum labels: %s',format_type(t.oid,NULL),t.typtype,format_type(NULLIF(t.typbasetype,0),t.typtypmod),t.typnotnull,t.typdefault,(SELECT string_agg(quote_literal(e.enumlabel),', ' ORDER BY e.enumsortorder) FROM pg_enum e WHERE e.enumtypid=t.oid)) FROM pg_type t WHERE t.oid=$1"
            }
        };
        let bounded = format!(
            "SELECT CASE WHEN octet_length(source)<=262144 THEN source END FROM ({sql}) AS definition(source)"
        );
        let row = async {
            crate::sql_observer::observe!(
                "PostgreSQL",
                &bounded,
                |query| client.query_opt(query, &[&object.id]).await,
                |row| Some(u64::from(row.is_some()))
            )
        }
        .await?
        .and_then(|r| r.get::<_, Option<String>>(0));
        row.ok_or_else(|| {
            anyhow::anyhow!("Object source exceeds the 256 KiB viewer limit or is unavailable")
        })
    }
}

async fn configure_catalog_read(client: &Client) -> Result<()> {
    async {
        crate::sql_observer::observe!(
            "PostgreSQL",
            "SET search_path=pg_catalog; SET statement_timeout='10s'; BEGIN READ ONLY ISOLATION LEVEL REPEATABLE READ",
            |query| client.batch_execute(query).await
        )
    }
    .await?;
    Ok(())
}

async fn query_definitions(
    client: &Client,
    schema: &str,
    needle: &str,
) -> Result<Vec<DefinitionHit>> {
    let namespace = client
        .query_opt(
            "SELECT oid, has_schema_privilege(oid, 'USAGE') FROM pg_namespace WHERE nspname = ($1::text)::name AND nspname::text = $1",
            &[&schema],
        )
        .await
        .map_err(|error| anyhow::anyhow!("{}", pg_error_message(&error)))?
        .ok_or_else(|| anyhow::anyhow!("Schema does not exist"))?;
    anyhow::ensure!(namespace.get::<_, bool>(1), "Permission denied for schema");
    let oid: u32 = namespace.get(0);
    let limit = DEFINITION_SEARCH_LIMIT as i64 + 1;
    let rows = client
        .query(
            DEFINITION_SQL,
            &[&oid, &needle, &limit, &DEFINITION_SOURCE_LIMIT],
        )
        .await
        .map_err(|error| anyhow::anyhow!("{}", pg_error_message(&error)))?;

    rows.into_iter()
        .map(|row| {
            let kind = match row.get::<_, &str>(2) {
                "view" => DefinitionKind::View,
                "materialized view" => DefinitionKind::MaterializedView,
                "routine" => DefinitionKind::Routine,
                "trigger" => DefinitionKind::Trigger,
                "rule" => DefinitionKind::Rule,
                "index" => DefinitionKind::Index,
                "constraint" => DefinitionKind::Constraint,
                "policy" => DefinitionKind::Policy,
                "statistics" => DefinitionKind::Statistics,
                _ => anyhow::bail!("Unexpected PostgreSQL definition kind"),
            };
            Ok(DefinitionHit {
                kind,
                object: ObjectSummary {
                    id: row.get(0),
                    schema: schema.into(),
                    name: row.get(1),
                    qualifier: row.get(3),
                },
                source: row.get(4),
            })
        })
        .collect()
}
