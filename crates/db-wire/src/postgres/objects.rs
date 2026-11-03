use super::*;
use crate::objects::{OBJECT_PAGE_SIZE, ObjectKind, ObjectPage, ObjectSummary};

struct SocketGuard(tokio::task::JoinHandle<()>);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

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
        client.batch_execute("SET search_path=pg_catalog; SET statement_timeout='10s'; BEGIN READ ONLY ISOLATION LEVEL REPEATABLE READ").await?;
        Ok((client, guard))
    }

    pub(super) async fn object_page(&self, kind: ObjectKind, after: u32) -> Result<ObjectPage> {
        let (client, _guard) = self.object_client().await?;
        let sql = format!(
            "{} AND n.nspname NOT IN ('pg_catalog','information_schema') AND n.nspname NOT LIKE 'pg_toast%' AND c.oid>$1 ORDER BY c.oid LIMIT 101",
            discovery(kind)
        );
        let rows = client.query(&sql, &[&after]).await?;
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
        let row = client
            .query_opt(&sql, &[&object.id])
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
        let row = client
            .query_opt(&bounded, &[&object.id])
            .await?
            .and_then(|r| r.get::<_, Option<String>>(0));
        row.ok_or_else(|| {
            anyhow::anyhow!("Object source exceeds the 256 KiB viewer limit or is unavailable")
        })
    }
}
