use super::*;
use crate::catalog::{CatalogRow, CatalogSection};

struct SocketGuard(tokio::task::JoinHandle<()>);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}
impl PostgresDriver {
    pub(super) async fn catalog_section(
        &self,
        schema: &str,
        table: &str,
        section: CatalogSection,
    ) -> Result<Option<Vec<CatalogRow>>> {
        let (client, task) = Self::connect_with_task(self.config.clone()).await?;
        let _guard = SocketGuard(task);
        async { crate::sql_observer::observe!("PostgreSQL", "SET search_path=pg_catalog; SET statement_timeout='10s'; BEGIN READ ONLY ISOLATION LEVEL REPEATABLE READ", |query| client.batch_execute(query).await) }.await?;
        let row=async { crate::sql_observer::observe!("PostgreSQL", "SELECT c.oid FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=$1 AND c.relname=$2 AND c.relkind IN ('r','p','v','m','f')", |query| client.query_opt(query,&[&schema,&table]).await, |row| Some(u64::from(row.is_some()))) }.await?
            .ok_or_else(||anyhow::anyhow!("Table/view no longer exists; refresh the connection catalog"))?;
        let oid: u32 = row.get(0);
        let sql = match section {
            CatalogSection::Dependencies => DEPENDENCIES,
            CatalogSection::References => REFERENCES,
            CatalogSection::Partitions => PARTITIONS,
            CatalogSection::Triggers => TRIGGERS,
            CatalogSection::Rules => RULES,
            CatalogSection::Policies => POLICIES,
            CatalogSection::Permissions => PERMISSIONS,
        };
        let rows = query_catalog_rows(&client, sql, oid).await?;
        Ok(Some(
            rows.into_iter()
                .map(|row| {
                    let schema: Option<String> = row.get(3);
                    let table: Option<String> = row.get(4);
                    CatalogRow {
                        name: row.get(0),
                        kind: row.get(1),
                        definition: row.get(2),
                        relation: schema.zip(table),
                    }
                })
                .collect(),
        ))
    }
}
const DEPENDENCIES: &str = r#"
SELECT DISTINCT pg_describe_object(d.refclassid,d.refobjid,d.refobjsubid),
       ('Direct dependency: '||d.deptype::text)::text,
       pg_describe_object(d.classid,d.objid,d.objsubid)||' -> '||
         pg_describe_object(d.refclassid,d.refobjid,d.refobjsubid),
       CASE WHEN c.relkind IN ('r','p','v','m','f') THEN n.nspname::text END,
       CASE WHEN c.relkind IN ('r','p','v','m','f') THEN c.relname::text END
FROM pg_depend d
LEFT JOIN pg_class c ON d.refclassid='pg_class'::regclass AND c.oid=d.refobjid
LEFT JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE ((d.classid='pg_class'::regclass AND d.objid=$1)
 OR (d.classid='pg_constraint'::regclass AND d.objid IN(SELECT oid FROM pg_constraint WHERE conrelid=$1))
 OR (d.classid='pg_attrdef'::regclass AND d.objid IN(SELECT oid FROM pg_attrdef WHERE adrelid=$1))
 OR (d.classid='pg_rewrite'::regclass AND d.objid IN(SELECT oid FROM pg_rewrite WHERE ev_class=$1)))
 AND NOT(d.refclassid='pg_class'::regclass AND d.refobjid=$1)
ORDER BY 1,2,3
"#;
const REFERENCES: &str = r#"
SELECT DISTINCT pg_describe_object(d.classid,d.objid,d.objsubid),
       ('Direct dependent: '||d.deptype::text)::text,
       CASE WHEN k.oid IS NOT NULL THEN pg_get_constraintdef(k.oid,true)
            WHEN rw.oid IS NOT NULL THEN pg_get_ruledef(rw.oid,true)
            ELSE pg_describe_object(d.classid,d.objid,d.objsubid) END,
       CASE WHEN c.relkind IN ('r','p','v','m','f') AND c.oid<>$1 THEN n.nspname::text END,
       CASE WHEN c.relkind IN ('r','p','v','m','f') AND c.oid<>$1 THEN c.relname::text END
FROM pg_depend d
LEFT JOIN pg_rewrite rw ON d.classid='pg_rewrite'::regclass AND rw.oid=d.objid
LEFT JOIN pg_constraint k ON d.classid='pg_constraint'::regclass AND k.oid=d.objid
LEFT JOIN pg_class c ON c.oid=CASE WHEN d.classid='pg_class'::regclass THEN d.objid
                                  WHEN rw.oid IS NOT NULL THEN rw.ev_class
                                  WHEN k.oid IS NOT NULL THEN k.conrelid END
LEFT JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE d.refclassid='pg_class'::regclass AND d.refobjid=$1
ORDER BY 1,2,3
"#;
const PARTITIONS: &str = r#"
SELECT n.nspname::text||'.'||c.relname::text,
       CASE WHEN c.relispartition THEN 'Direct partition' ELSE 'Inheritance child' END,
       COALESCE(pg_get_expr(c.relpartbound,c.oid,true),'Inheritance child (no partition bound)'),
       n.nspname::text,c.relname::text
FROM pg_inherits i JOIN pg_class c ON c.oid=i.inhrelid JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE i.inhparent=$1
UNION ALL
SELECT n.nspname::text||'.'||c.relname::text,'Parent relation','Parent of the current relation',
       n.nspname::text,c.relname::text
FROM pg_inherits i JOIN pg_class c ON c.oid=i.inhparent JOIN pg_namespace n ON n.oid=c.relnamespace
WHERE i.inhrelid=$1 ORDER BY 1,2
"#;
pub(super) const TRIGGERS: &str = r#"
SELECT t.tgname::text,
       CASE t.tgenabled WHEN 'O' THEN 'Enabled (origin)' WHEN 'D' THEN 'Disabled'
                       WHEN 'R' THEN 'Enabled (replica)' ELSE 'Enabled (always)' END,
       pg_get_triggerdef(t.oid,true),NULL::text,NULL::text
FROM pg_trigger t WHERE t.tgrelid=$1 AND NOT t.tgisinternal ORDER BY t.tgname
"#;
const RULES: &str = r#"
SELECT r.rulename::text,
       CASE r.ev_enabled WHEN 'O' THEN 'Enabled (origin)' WHEN 'D' THEN 'Disabled'
                         WHEN 'R' THEN 'Enabled (replica)' ELSE 'Enabled (always)' END,
       pg_get_ruledef(r.oid,true),NULL::text,NULL::text
FROM pg_rewrite r WHERE r.ev_class=$1 ORDER BY r.rulename
"#;
const POLICIES: &str = r#"
SELECT p.polname::text,
       (CASE WHEN p.polpermissive THEN 'Permissive' ELSE 'Restrictive' END)||' / '||
       CASE p.polcmd WHEN 'r' THEN 'SELECT' WHEN 'a' THEN 'INSERT' WHEN 'w' THEN 'UPDATE'
                     WHEN 'd' THEN 'DELETE' ELSE 'ALL' END,
       'Roles: '||array_to_string(ARRAY(SELECT CASE WHEN role=0 THEN 'PUBLIC' ELSE pg_get_userbyid(role)::text END FROM unnest(p.polroles) role),', ')||
       '; RLS enabled: '||c.relrowsecurity::text||'; forced: '||c.relforcerowsecurity::text||
       '; USING: '||COALESCE(pg_get_expr(p.polqual,p.polrelid,true),'Not specified')||
       '; WITH CHECK: '||COALESCE(pg_get_expr(p.polwithcheck,p.polrelid,true),'Not specified'),
       NULL::text,NULL::text
FROM pg_policy p JOIN pg_class c ON c.oid=p.polrelid WHERE p.polrelid=$1 ORDER BY p.polname
"#;
const PERMISSIONS: &str = r#"
SELECT CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee)::text END,
       'TABLE '||a.privilege_type,
       'Grantor: '||pg_get_userbyid(a.grantor)::text||'; grant option: '||a.is_grantable::text||
       CASE WHEN c.relacl IS NULL THEN '; default owner ACL' ELSE '; stored direct ACL' END,
       NULL::text,NULL::text
FROM pg_class c CROSS JOIN LATERAL aclexplode(COALESCE(c.relacl,acldefault('r',c.relowner))) a
WHERE c.oid=$1
UNION ALL
SELECT CASE WHEN a.grantee=0 THEN 'PUBLIC' ELSE pg_get_userbyid(a.grantee)::text END,
       'COLUMN '||quote_ident(col.attname)||' '||a.privilege_type,
       'Grantor: '||pg_get_userbyid(a.grantor)::text||'; grant option: '||a.is_grantable::text||'; stored direct column ACL',
       NULL::text,NULL::text
FROM pg_attribute col CROSS JOIN LATERAL aclexplode(col.attacl) a
WHERE col.attrelid=$1 AND col.attnum>0 AND NOT col.attisdropped
ORDER BY 1,2,3
"#;

pub(super) async fn query_catalog_rows(
    client: &Client,
    sql: &str,
    oid: u32,
) -> Result<Vec<tokio_postgres::Row>> {
    async {
        crate::sql_observer::observe!(
            "PostgreSQL",
            sql,
            |query| client.query(query, &[&oid]).await,
            |rows| Some(rows.len() as u64)
        )
    }
    .await
    .map_err(|e| {
        let message = pg_error_message(&e);
        anyhow::Error::new(e).context(message)
    })
}
