use super::*;
use crate::objects::{
    DATA_PREVIEW_CHARS, DATA_SEARCH_LIMIT, DEFINITION_INPUT_LIMIT, DataRowIdentity, DataRowLocator,
    DataRowPreview, DataSearchHit,
};
use crate::{DbDriverKind, sql::SqlDialect};
use anyhow::Context as _;

const PREVIEW_COLUMNS: usize = 32;
const SNIPPET_LEAD_CHARS: i32 = 96;

impl MySqlDriver {
    pub(super) async fn find_table_data(
        &self,
        schema: &str,
        table: &str,
        needle: &str,
        cancel: flume::Receiver<()>,
    ) -> Result<Option<Vec<DataSearchHit>>> {
        anyhow::ensure!(
            !needle.trim().is_empty()
                && [schema, table, needle].iter().all(|value| !value.is_empty()
                    && value.len() <= DEFINITION_INPUT_LIMIT
                    && !value.contains('\0')),
            "Enter an exact database and table name and search text up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes"
        );
        let (schema, table, needle) = (schema.to_owned(), table.to_owned(), needle.to_owned());
        self.run_catalog_search("Data search", cancel, move |conn| {
            Box::pin(async move {
                let mut options = TxOpts::default();
                options.with_readonly(Some(true));
                let mut tx = conn.start_transaction(options).await?;
                let result = query_table(&mut tx, &schema, &table, &needle).await;
                tx.rollback().await?;
                result.map(Some)
            })
        })
        .await
    }
}

async fn query_table(
    tx: &mut mysql_async::Transaction<'_>,
    schema: &str,
    table: &str,
    needle: &str,
) -> Result<Vec<DataSearchHit>> {
    let qualified = SqlDialect(DbDriverKind::MySql).qualified_table(schema, table);
    // Resolve the relation before inspecting columns so DDL cannot replace it
    // during the scan. LIMIT 0 reads no user rows.
    tx.query_drop(format!("SELECT 1 FROM {qualified} LIMIT 0"))
        .await?;
    let metadata: Vec<(String, String)> = tx.exec(
        "SELECT c.COLUMN_NAME, c.DATA_TYPE
         FROM information_schema.COLUMNS c
         JOIN information_schema.TABLES t ON c.TABLE_SCHEMA = t.TABLE_SCHEMA AND c.TABLE_NAME = t.TABLE_NAME
           AND CAST(c.TABLE_SCHEMA AS BINARY) = CAST(t.TABLE_SCHEMA AS BINARY)
           AND CAST(c.TABLE_NAME AS BINARY) = CAST(t.TABLE_NAME AS BINARY)
         WHERE c.TABLE_SCHEMA = ? AND c.TABLE_NAME = ?
           AND CAST(c.TABLE_SCHEMA AS BINARY) = CAST(? AS BINARY)
           AND CAST(c.TABLE_NAME AS BINARY) = CAST(? AS BINARY)
           AND t.TABLE_TYPE = 'BASE TABLE'
         ORDER BY c.ORDINAL_POSITION",
        (schema, table, schema, table),
    ).await?;
    anyhow::ensure!(
        !metadata.is_empty(),
        "Search requires an existing MySQL base table"
    );
    let total = metadata.len();
    let columns: Vec<String> = metadata
        .into_iter()
        .filter_map(|(name, kind)| {
            matches!(
                kind.as_str(),
                "char"
                    | "varchar"
                    | "tinytext"
                    | "text"
                    | "mediumtext"
                    | "longtext"
                    | "tinyint"
                    | "smallint"
                    | "mediumint"
                    | "int"
                    | "bigint"
                    | "decimal"
                    | "float"
                    | "double"
                    | "date"
                    | "datetime"
                    | "timestamp"
                    | "time"
                    | "year"
                    | "enum"
                    | "set"
                    | "json"
            )
            .then_some(name)
        })
        .collect();
    anyhow::ensure!(
        !columns.is_empty(),
        "No supported scalar columns in this table"
    );
    let sql = search_sql(&qualified, &columns);
    let mut rows = tx
        .exec_iter(
            sql,
            (
                needle,
                SNIPPET_LEAD_CHARS,
                DATA_PREVIEW_CHARS,
                (DATA_SEARCH_LIMIT + 1) as u64,
            ),
        )
        .await?;
    let mut hits = Vec::new();
    while let Some(mut row) = rows.next().await? {
        let index = row
            .take::<u64, _>(0)
            .context("MySQL match column missing")? as usize;
        let preview = row
            .take::<String, _>(1)
            .context("MySQL match preview missing")?;
        let mut snapshot = columns
            .iter()
            .take(PREVIEW_COLUMNS)
            .enumerate()
            .map(|(i, name)| {
                Ok((
                    name.clone(),
                    row.take::<Option<String>, _>(i + 2)
                        .context("MySQL preview column missing")?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        let column = columns
            .get(index)
            .context("MySQL match column is invalid")?
            .clone();
        if index >= PREVIEW_COLUMNS {
            snapshot[PREVIEW_COLUMNS - 1] = (column.clone(), Some(preview.clone()));
        }
        hits.push(DataSearchHit {
            column,
            preview,
            locator: DataRowLocator(DataRowIdentity::MySql(DataRowPreview {
                omitted_columns: total - snapshot.len(),
                columns: snapshot,
            })),
        });
    }
    rows.drop_result().await?;
    Ok(hits)
}

fn search_sql(qualified: &str, columns: &[String]) -> String {
    let dialect = SqlDialect(DbDriverKind::MySql);
    let mut match_arms = Vec::new();
    let mut value_arms = Vec::new();
    let mut previews = Vec::new();
    for (index, name) in columns.iter().enumerate() {
        let value = format!("CONVERT(t.{} USING utf8mb4)", dialect.quote_ident(name));
        match_arms.push(format!(
            "WHEN LOCATE(needle, LOWER({value}) COLLATE utf8mb4_bin) > 0 THEN {index}"
        ));
        value_arms.push(format!("WHEN {index} THEN {value}"));
        if index < PREVIEW_COLUMNS {
            previews.push(format!(
                "LEFT({value}, {DATA_PREVIEW_CHARS}) AS preview_{index}"
            ));
        }
    }
    let projection = (0..previews.len())
        .map(|index| format!("preview_{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "WITH term AS (SELECT LOWER(CONVERT(? USING utf8mb4)) COLLATE utf8mb4_bin AS needle)
         SELECT /*+ MAX_EXECUTION_TIME(10000) */ matched_index,
             SUBSTRING(matched_value, GREATEST(1, LOCATE(needle, LOWER(matched_value) COLLATE utf8mb4_bin) - ?), ?),
             {projection}
         FROM (
             SELECT needle, CASE {} END AS matched_index,
                 CASE (CASE {} END) {} END AS matched_value, {}
             FROM {qualified} AS t CROSS JOIN term
         ) AS matched WHERE matched_index IS NOT NULL LIMIT ?",
        match_arms.join(" "), match_arms.join(" "), value_arms.join(" "), previews.join(", "),
    )
}
