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
