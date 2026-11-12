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
        .context("MySQL column metadata field is missing")??)
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

#[cfg(test)]
mod tests {
    use super::*;

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
