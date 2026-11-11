use super::*;
use crate::DbDriverKind;
use crate::objects::{
    DATA_PREVIEW_CHARS, DATA_SEARCH_LIMIT, DEFINITION_INPUT_LIMIT, DataRowIdentity, DataRowLocator,
    DataRowPreview, DataSearchHit, PostgresRowLocator,
};
use crate::sql::SqlDialect;

const PREVIEW_COLUMNS: usize = 32;
const SNIPPET_LEAD_CHARS: i32 = 96;
const RELATION_SQL: &str = "
    SELECT c.oid, has_table_privilege(c.oid, 'SELECT')
    FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
    WHERE n.nspname = ($1::text)::name AND n.nspname::text = $1
      AND c.relname = ($2::text)::name AND c.relname::text = $2
      AND c.relkind = 'r' AND c.relpersistence <> 't'
      AND c.relam = (SELECT oid FROM pg_am WHERE amname = 'heap' AND amtype = 't')";
const COLUMN_SQL: &str = "
    SELECT attname::text, atttypid IN (
        'bool'::regtype, 'int2'::regtype, 'int4'::regtype, 'int8'::regtype,
        'float4'::regtype, 'float8'::regtype, 'numeric'::regtype, 'text'::regtype,
        'varchar'::regtype, 'bpchar'::regtype, 'uuid'::regtype, 'date'::regtype,
        'time'::regtype, 'timestamp'::regtype, 'timestamptz'::regtype
    )
    FROM pg_attribute
    WHERE attrelid = $1 AND attnum > 0 AND NOT attisdropped
    ORDER BY attnum";

struct SearchTable {
    oid: u32,
    qualified: String,
    columns: Vec<String>,
    omitted_columns: usize,
}

impl SearchTable {
    async fn load(client: &Client, schema: &str, table: &str) -> Result<Self> {
        let row = client
            .query_opt(RELATION_SQL, &[&schema, &table])
            .await
            .map_err(|error| anyhow::anyhow!("{}", pg_error_message(&error)))?
            .ok_or_else(|| anyhow::anyhow!("Search requires an existing PostgreSQL heap table"))?;
        anyhow::ensure!(
            row.get::<_, bool>(1),
            "Permission denied for table data search"
        );
        let oid = row.get(0);
        let attrs = client
            .query(COLUMN_SQL, &[&oid])
            .await
            .map_err(|error| anyhow::anyhow!("{}", pg_error_message(&error)))?;
        let columns: Vec<String> = attrs
            .iter()
            .filter(|row| row.get::<_, bool>(1))
            .map(|row| row.get(0))
            .collect();
        Ok(Self {
            oid,
            qualified: SqlDialect(DbDriverKind::Postgres).qualified_table(schema, table),
            omitted_columns: attrs.len() - columns.len().min(PREVIEW_COLUMNS),
            columns,
        })
    }
}

impl PostgresDriver {
    pub(super) async fn find_table_data(
        &self,
        schema: &str,
        table: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Vec<DataSearchHit>> {
        anyhow::ensure!(
            !needle.trim().is_empty()
                && [schema, table, needle].iter().all(|value| !value.is_empty()
                    && value.len() <= DEFINITION_INPUT_LIMIT
                    && !value.contains('\0')),
            "Enter an exact schema and table name and search text up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes"
        );
        self.run_catalog_search("Data search", cancel, async |client| {
            let relation = SearchTable::load(client, schema, table).await?;
            search_rows(client, relation, schema, table, needle).await
        })
        .await
    }

    pub(super) async fn read_search_match(
        &self,
        locator: &DataRowLocator,
        cancel: flume::Receiver<()>,
    ) -> Result<DataRowPreview> {
        let DataRowIdentity::Postgres(locator) = &locator.0 else {
            anyhow::bail!("Search result belongs to another provider");
        };
        self.run_catalog_search("Open search match", cancel, async |client| {
            let table = SearchTable::load(client, &locator.schema, &locator.table).await?;
            anyhow::ensure!(
                table.oid == locator.relation,
                "Search result table changed; search again"
            );
            read_row(client, table, locator).await
        })
        .await
    }
}

fn search_sql(table: &SearchTable) -> String {
    let dialect = SqlDialect(DbDriverKind::Postgres);
    let columns = table
        .columns
        .iter()
        .enumerate()
        .map(|(index, name)| {
            format!(
                r#"({index}, t.{}::text COLLATE pg_catalog."default")"#,
                dialect.quote_ident(name)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "SELECT hit.ordinal, substring(hit.value FROM greatest(1, hit.position - $4) FOR $5),
                t.ctid::text, t.xmin::text, pg_relation_filenode(t.tableoid)
         FROM ONLY {} AS t
         CROSS JOIN LATERAL (
             SELECT ordinal, value, strpos(lower(value), lower($1)) AS position
             FROM (VALUES {columns}) AS cells(ordinal, value)
             WHERE strpos(lower(value), lower($1)) > 0
             LIMIT 1
         ) AS hit
         WHERE t.tableoid = $2
         LIMIT $3",
        table.qualified,
    )
}

async fn search_rows(
    client: &Client,
    table: SearchTable,
    schema: &str,
    name: &str,
    needle: &str,
) -> Result<Vec<DataSearchHit>> {
    anyhow::ensure!(
        !table.columns.is_empty(),
        "No supported scalar columns in this table"
    );
    let sql = search_sql(&table);
    let limit = DATA_SEARCH_LIMIT as i64 + 1;
    let rows = client
        .query(
            &sql,
            &[
                &needle,
                &table.oid,
                &limit,
                &SNIPPET_LEAD_CHARS,
                &DATA_PREVIEW_CHARS,
            ],
        )
        .await
        .map_err(|error| anyhow::anyhow!("{}", pg_error_message(&error)))?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let column = table.columns[row.get::<_, i32>(0) as usize].clone();
            DataSearchHit {
                column: column.clone(),
                preview: row.get(1),
                locator: DataRowLocator(DataRowIdentity::Postgres(PostgresRowLocator {
                    schema: schema.into(),
                    table: name.into(),
                    relation: table.oid,
                    tuple: row.get(2),
                    version: row.get(3),
                    file: row.get(4),
                    column,
                })),
            }
        })
        .collect())
}

async fn read_row(
    client: &Client,
    table: SearchTable,
    locator: &PostgresRowLocator,
) -> Result<DataRowPreview> {
    let matched = table
        .columns
        .iter()
        .position(|name| name == &locator.column)
        .ok_or_else(|| anyhow::anyhow!("Search result columns changed; search again"))?;
    let mut names = table
        .columns
        .into_iter()
        .take(PREVIEW_COLUMNS)
        .collect::<Vec<_>>();
    if matched >= PREVIEW_COLUMNS {
        names[PREVIEW_COLUMNS - 1] = locator.column.clone();
    }
    let dialect = SqlDialect(DbDriverKind::Postgres);
    let projection = names
        .iter()
        .map(|name| format!("left(t.{}::text, $4)", dialect.quote_ident(name)))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {projection} FROM ONLY {} AS t
         WHERE t.tableoid = $1 AND t.ctid = ($2::text)::tid AND t.xmin = ($3::text)::xid
           AND pg_relation_filenode(t.tableoid) = $5",
        table.qualified,
    );
    let row = client
        .query_opt(
            &sql,
            &[
                &table.oid,
                &locator.tuple,
                &locator.version,
                &DATA_PREVIEW_CHARS,
                &locator.file,
            ],
        )
        .await
        .map_err(|error| anyhow::anyhow!("{}", pg_error_message(&error)))?
        .ok_or_else(|| {
            anyhow::anyhow!("Matching row changed or is no longer visible; search again")
        })?;
    let columns = names
        .into_iter()
        .enumerate()
        .map(|(index, name)| (name, row.get(index)))
        .collect();
    Ok(DataRowPreview {
        columns,
        omitted_columns: table.omitted_columns,
    })
}
