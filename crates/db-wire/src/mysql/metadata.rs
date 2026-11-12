//! Native column attributes shared by scoped and schema-wide inspection.
use super::*;
use anyhow::Context as _;

const FIELDS: &str = "table_schema, table_name, column_name, data_type, is_nullable, column_key, column_type, column_default, collation_name, column_comment, extra, generation_expression";

pub(super) fn column_query(scoped: bool) -> String {
    let filter = if scoped {
        "table_schema = ? AND table_name = ?"
    } else {
        "table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')"
    };
    format!(
        "SELECT {FIELDS} FROM information_schema.columns WHERE {filter} ORDER BY table_schema, table_name, ordinal_position"
    )
}

fn field<T: FromValue>(row: &mysql_async::Row, index: usize) -> Result<T> {
    Ok(row
        .get_opt(index)
        .context("MySQL metadata field is missing")??)
}

pub(super) fn column(row: &mysql_async::Row) -> Result<ColumnMeta> {
    let mut column = info_schema_column(
        field(row, 2)?,
        &field::<String>(row, 3)?,
        &field::<String>(row, 4)?,
        &field::<String>(row, 5)?,
    );
    column.raw_type = Some(field(row, 6)?);
    // Preserve the server's value; SHOW CREATE remains the authoritative source
    // for distinctions INFORMATION_SCHEMA cannot express (absent vs NULL default).
    column.default_val = field(row, 7)?;
    column.collation = field(row, 8)?;
    column.comment = field(row, 9)?;
    let extra: String = field(row, 10)?;
    column.generates_key_on_null = extra.split_whitespace().any(|v| v == "auto_increment");
    column.is_generated = field::<Option<String>>(row, 11)?.is_some_and(|value| !value.is_empty());
    Ok(column)
}

pub(super) fn grouped(
    rows: Vec<mysql_async::Row>,
) -> Result<Vec<(String, String, Vec<ColumnMeta>)>> {
    let mut tables = std::collections::BTreeMap::<(String, String), Vec<ColumnMeta>>::new();
    for row in rows {
        tables
            .entry((field(&row, 0)?, field(&row, 1)?))
            .or_default()
            .push(column(&row)?);
    }
    Ok(tables
        .into_iter()
        .map(|((s, t), cols)| (s, t, cols))
        .collect())
}

pub(super) const TABLE_STATISTICS: &str = r#"
SELECT table_schema, table_name, table_rows, data_length, index_length, table_comment
FROM information_schema.tables
WHERE table_type = 'BASE TABLE'
  AND table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
LIMIT ?
"#;

pub(super) const MAX_STATISTICS_TABLES: usize = 10_000;
pub(super) type TableStatisticsRow = (
    String,
    String,
    Option<u64>,
    Option<u64>,
    Option<u64>,
    Option<String>,
);

pub(super) fn table_statistics(
    rows: Vec<TableStatisticsRow>,
) -> Result<Vec<(String, String, crate::TableDetails)>> {
    anyhow::ensure!(
        rows.len() <= MAX_STATISTICS_TABLES,
        "MySQL statistics exceed {MAX_STATISTICS_TABLES} tables"
    );
    let signed = |value: Option<u64>| value.and_then(|value| i64::try_from(value).ok());
    let mut details = rows
        .into_iter()
        .map(|(schema, table, rows, data, indexes, comment)| {
            let table_bytes = signed(data);
            let index_bytes = signed(indexes);
            (
                schema,
                table,
                crate::TableDetails {
                    comment,
                    estimated_rows: signed(rows),
                    table_bytes,
                    index_bytes,
                    total_bytes: table_bytes
                        .zip(index_bytes)
                        .and_then(|(data, indexes)| data.checked_add(indexes)),
                    ..Default::default()
                },
            )
        })
        .collect::<Vec<_>>();
    details.sort_unstable_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    Ok(details)
}

pub(super) const MAX_INDEX_KEYS: usize = 20_000;
const MAX_INDEX_EXPRESSION_BYTES: usize = 2_048;

pub(super) fn index_query(has_expression: bool, has_visibility: bool) -> String {
    let expression = if has_expression {
        format!(
            "CASE WHEN OCTET_LENGTH(expression) <= {MAX_INDEX_EXPRESSION_BYTES} THEN expression END"
        )
    } else {
        "NULL".into()
    };
    let visibility = if has_visibility { "is_visible" } else { "NULL" };
    format!(
        "SELECT table_schema, table_name, index_name, seq_in_index, column_name,
                sub_part, collation, non_unique, index_type, {expression}, {visibility}
         FROM information_schema.statistics
         WHERE table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
         LIMIT ?"
    )
}

pub(super) fn add_indexes(
    tables: &mut [(String, String, crate::TableDetails)],
    rows: Vec<mysql_async::Row>,
) -> Result<()> {
    use crate::{IndexDetails, sql::SqlDialect};
    use std::collections::BTreeMap;
    anyhow::ensure!(
        rows.len() <= MAX_INDEX_KEYS,
        "MySQL index details exceed {MAX_INDEX_KEYS} keys"
    );
    let dialect = SqlDialect(crate::DbDriverKind::MySql);
    let mut indexes = BTreeMap::<(usize, String), (IndexDetails, BTreeMap<u64, String>)>::new();
    for row in rows {
        let schema: String = field(&row, 0)?;
        let table: String = field(&row, 1)?;
        let Ok(table_index) =
            tables.binary_search_by(|item| (&item.0, &item.1).cmp(&(&schema, &table)))
        else {
            continue;
        };
        let name: String = field(&row, 2)?;
        let seq = field(&row, 3)?;
        let mut key = match field::<Option<String>>(&row, 4)? {
            Some(column) => dialect.quote_ident(&column),
            None => field::<Option<String>>(&row, 9)?
                .map(|expression| format!("({expression})"))
                .unwrap_or_else(|| "Expression unavailable".into()),
        };
        if let Some(prefix) = field::<Option<u64>>(&row, 5)? {
            key.push_str(&format!("({prefix})"));
        }
        match field::<Option<String>>(&row, 6)?.as_deref() {
            Some("A") => key.push_str(" ASC"),
            Some("D") => key.push_str(" DESC"),
            _ => {}
        }
        let (_, keys) = match indexes.entry((table_index, name.clone())) {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let visibility = match field::<Option<String>>(&row, 10)?.as_deref() {
                    Some("YES") => "Visible",
                    Some("NO") => "Invisible",
                    _ => "Unavailable",
                };
                entry.insert((
                    IndexDetails {
                        name,
                        method: field(&row, 8)?,
                        columns: Vec::new(),
                        unique: field::<u64>(&row, 7)? == 0,
                        // MySQL optimizer visibility is independent of index validation.
                        valid: false,
                        definition: format!("Optimizer visibility: {visibility}"),
                    },
                    BTreeMap::new(),
                ))
            }
        };
        keys.insert(seq, key);
    }
    for ((table, _), (mut index, keys)) in indexes {
        index.columns = keys.into_values().collect();
        tables[table].2.indexes.push(index);
    }
    Ok(())
}

pub(super) const MAX_CONSTRAINT_ROWS: usize = 20_000;
pub(super) const CONSTRAINT_DETAILS: &str = r#"
SELECT t.table_schema, t.table_name, t.constraint_name, t.constraint_type,
       k.ordinal_position, k.column_name, k.referenced_table_schema,
       k.referenced_table_name, k.referenced_column_name,
       r.update_rule, r.delete_rule,
       CASE WHEN OCTET_LENGTH(c.check_clause) <= 2048 THEN c.check_clause END,
       t.enforced
FROM information_schema.table_constraints t
LEFT JOIN information_schema.key_column_usage k
  ON k.constraint_schema = t.constraint_schema AND k.table_name = t.table_name
 AND k.constraint_name = t.constraint_name
 AND ((t.constraint_type = 'FOREIGN KEY' AND k.referenced_table_name IS NOT NULL)
   OR (t.constraint_type IN ('PRIMARY KEY', 'UNIQUE') AND k.referenced_table_name IS NULL))
LEFT JOIN information_schema.referential_constraints r
  ON t.constraint_type = 'FOREIGN KEY' AND r.constraint_schema = t.constraint_schema
 AND r.table_name = t.table_name AND r.constraint_name = t.constraint_name
LEFT JOIN (
    SELECT constraint_schema, constraint_name,
           CASE WHEN OCTET_LENGTH(check_clause) <= 2048 THEN check_clause END AS check_clause
    FROM information_schema.check_constraints
    WHERE constraint_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
    LIMIT 20001
) c
  ON t.constraint_type = 'CHECK' AND c.constraint_schema = t.constraint_schema
 AND c.constraint_name = t.constraint_name
WHERE t.table_schema NOT IN ('information_schema', 'mysql', 'performance_schema', 'sys')
LIMIT ?
"#;

pub(super) fn constraint_query(modern: bool) -> String {
    if modern {
        return CONSTRAINT_DETAILS.to_owned();
    }
    let check_join = CONSTRAINT_DETAILS
        .find("LEFT JOIN (")
        .expect("CHECK catalog join");
    let filters = CONSTRAINT_DETAILS
        .find("WHERE t.table_schema")
        .expect("constraint catalog filter");
    format!("{}{}", &CONSTRAINT_DETAILS[..check_join], &CONSTRAINT_DETAILS[filters..])
        .replace("CASE WHEN OCTET_LENGTH(c.check_clause) <= 2048 THEN c.check_clause END,\n       t.enforced", "NULL, NULL")
}

struct ConstraintEntry {
    detail: crate::ConstraintDetails,
    columns: std::collections::BTreeMap<u64, (String, Option<String>)>,
    reference: Option<String>,
    actions: Option<String>,
}

impl ConstraintEntry {
    fn from_row(row: &mysql_async::Row) -> Result<Self> {
        let kind: String = field(row, 3)?;
        let mut definition = "See native relation DDL for full definition".into();
        if kind == "CHECK" {
            let expression = field::<Option<String>>(row, 11)?;
            let enforced: Option<String> = field(row, 12)?;
            let enforcement = match enforced.as_deref() {
                Some("YES") => "ENFORCED",
                Some("NO") => "NOT ENFORCED",
                _ => "enforcement unavailable",
            };
            definition = format!(
                "CHECK ({}) {enforcement}",
                expression.as_deref().unwrap_or("expression unavailable")
            );
        }
        let (reference, actions) = if kind == "FOREIGN KEY" {
            let schema: Option<String> = field(row, 6)?;
            let table: Option<String> = field(row, 7)?;
            let update: Option<String> = field(row, 9)?;
            let delete: Option<String> = field(row, 10)?;
            let reference = schema.zip(table).map(|(schema, table)| {
                crate::sql::SqlDialect(crate::DbDriverKind::MySql).qualified_table(&schema, &table)
            });
            let actions = update
                .zip(delete)
                .map(|(update, delete)| format!("ON UPDATE {update} ON DELETE {delete}"));
            (reference, actions)
        } else {
            (None, None)
        };
        Ok(Self {
            detail: crate::ConstraintDetails {
                name: field(row, 2)?,
                kind,
                columns: Vec::new(),
                definition,
                validated: false,
            },
            columns: Default::default(),
            reference,
            actions,
        })
    }

    fn finish(mut self) -> crate::ConstraintDetails {
        let dialect = crate::sql::SqlDialect(crate::DbDriverKind::MySql);
        let (source, target): (Vec<_>, Vec<_>) = self.columns.into_values().unzip();
        self.detail.columns = source;
        if self.detail.kind == "FOREIGN KEY" {
            self.detail.definition = match (
                self.reference,
                self.actions,
                target.into_iter().collect::<Option<Vec<_>>>(),
            ) {
                (Some(reference), Some(actions), Some(target)) if !target.is_empty() => {
                    let target = target
                        .iter()
                        .map(|column| dialect.quote_ident(column))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("REFERENCES {reference} ({target}) {actions}")
                }
                _ => "Reference details unavailable".into(),
            };
        }
        self.detail
    }
}

pub(super) fn add_constraints(
    tables: &mut [(String, String, crate::TableDetails)],
    rows: Vec<mysql_async::Row>,
) -> Result<()> {
    use std::collections::{BTreeMap, btree_map::Entry};
    anyhow::ensure!(
        rows.len() <= MAX_CONSTRAINT_ROWS,
        "MySQL constraint details exceed {MAX_CONSTRAINT_ROWS} rows"
    );
    let mut constraints = BTreeMap::<(usize, String, String), ConstraintEntry>::new();
    for row in rows {
        let schema: String = field(&row, 0)?;
        let table: String = field(&row, 1)?;
        let Ok(table_index) =
            tables.binary_search_by(|item| (&item.0, &item.1).cmp(&(&schema, &table)))
        else {
            continue;
        };
        let name: String = field(&row, 2)?;
        let entry = match constraints.entry((table_index, field(&row, 3)?, name)) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(ConstraintEntry::from_row(&row)?),
        };
        if let Some(column) = field::<Option<String>>(&row, 5)? {
            entry
                .columns
                .insert(field(&row, 4)?, (column, field(&row, 8)?));
        }
    }
    for ((table, _, _), constraint) in constraints {
        tables[table].2.constraints.push(constraint.finish());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_constraint_projection_reads_real_ordered_keys() {
        let Ok(port) = std::env::var("FASTDB_TEST_MYSQL_PORT") else {
            assert!(
                std::env::var_os("FASTDB_REQUIRE_DB_TESTS").is_none(),
                "MySQL fixture port required"
            );
            return;
        };
        crate::get_runtime().block_on(async {
            let driver = MySqlDriver::new(ConnectionConfig {
                driver: crate::DbDriverKind::MySql,
                host: std::env::var("FASTDB_TEST_MYSQL_HOST")
                    .unwrap_or_else(|_| "127.0.0.1".into()),
                port: port.parse().unwrap(),
                user: std::env::var("FASTDB_TEST_MYSQL_USER").unwrap_or_else(|_| "root".into()),
                password: std::env::var("FASTDB_TEST_MYSQL_PASSWORD").ok(),
                database: std::env::var("FASTDB_TEST_MYSQL_DATABASE")
                    .unwrap_or_else(|_| "mysql".into()),
                use_unix_socket: false,
                ..Default::default()
            });
            let schema = format!(
                "fastdb_legacy_constraints_{}",
                uuid::Uuid::new_v4().simple()
            );
            driver
                .execute_query(&format!("CREATE DATABASE {schema}"))
                .await
                .unwrap();
            let result: Result<_> = async {
                driver
                    .execute_query(&format!(
                        "CREATE TABLE {schema}.items (a INT, b INT, PRIMARY KEY(b,a))"
                    ))
                    .await?;
                let rows = driver
                    .query_rows(constraint_query(false), (MAX_CONSTRAINT_ROWS + 1,))
                    .await?;
                let mut tables = vec![(
                    schema.clone(),
                    "items".into(),
                    crate::TableDetails::default(),
                )];
                add_constraints(&mut tables, rows)?;
                Ok(tables)
            }
            .await;
            driver
                .execute_query(&format!("DROP DATABASE {schema}"))
                .await
                .unwrap();
            let tables = result.unwrap();
            assert_eq!(tables[0].2.constraints.len(), 1);
            assert_eq!(tables[0].2.constraints[0].columns, ["b", "a"]);
        });
    }

    #[test]
    fn index_catalog_queries_keep_visibility_without_functional_keys() {
        let modern = index_query(true, true);
        assert!(modern.contains("OCTET_LENGTH(expression)"));
        assert!(modern.contains("is_visible"));
        let visibility_only = index_query(false, true);
        assert!(visibility_only.contains("index_type, NULL, is_visible"));
        let legacy = index_query(false, false);
        assert!(legacy.contains("index_type, NULL, NULL"));
        assert!(legacy.ends_with("LIMIT ?"));
    }

    #[test]
    fn statistics_preserve_zero_and_keep_missing_or_overflowing_values_unavailable() {
        let details = table_statistics(vec![
            ("db".into(), "empty".into(), Some(0), Some(0), None, None),
            (
                "db".into(),
                "large".into(),
                Some(u64::MAX),
                Some(i64::MAX as u64),
                Some(1),
                None,
            ),
        ])
        .unwrap();
        assert_eq!(details[0].2.estimated_rows, Some(0));
        assert_eq!(details[0].2.index_bytes, None);
        assert_eq!(details[0].2.total_bytes, None);
        assert_eq!(details[1].2.estimated_rows, None);
        assert_eq!(details[1].2.table_bytes, Some(i64::MAX));
        assert_eq!(details[1].2.total_bytes, None);
        assert!(table_statistics(vec![Default::default(); MAX_STATISTICS_TABLES + 1]).is_err());
    }
}
