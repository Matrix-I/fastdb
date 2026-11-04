//! Checked, bounded parameterized INSERT batches. Values never become SQL text.
use super::{
    csv::{MAX_COLUMNS, MAX_FIELD_BYTES, MAX_RECORD_BYTES},
    mapping::Value,
};
use crate::{DbDriverKind, sql::SqlDialect};
use anyhow::{Result, ensure};
use std::collections::HashSet;

pub const MAX_BATCH_ROWS: usize = 256;
pub const MAX_PARAMETERS: usize = 32_760;
const MAX_IDENTIFIER_BYTES: usize = 4096;

pub struct Batch {
    schema: String,
    table: String,
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}
impl Batch {
    pub fn new(
        schema: impl Into<String>,
        table: impl Into<String>,
        columns: Vec<String>,
        rows: Vec<Vec<Value>>,
    ) -> Result<Self> {
        let schema = schema.into();
        let table = table.into();
        ensure!(
            !columns.is_empty() && columns.len() <= MAX_COLUMNS,
            "Batch requires 1–1,024 destination columns"
        );
        ensure!(
            !rows.is_empty() && rows.len() <= MAX_BATCH_ROWS,
            "Batch requires 1–256 rows"
        );
        ensure!(
            rows.len() * columns.len() <= MAX_PARAMETERS,
            "Batch parameter budget exceeded"
        );
        let mut names = HashSet::new();
        let mut identifier_bytes = 0usize;
        for name in std::iter::once(&schema)
            .chain(std::iter::once(&table))
            .chain(columns.iter())
        {
            ensure!(
                !name.is_empty() && !name.contains('\0') && name.len() <= MAX_IDENTIFIER_BYTES,
                "Invalid or oversized destination identifier"
            );
            identifier_bytes += name.len();
            ensure!(
                identifier_bytes <= MAX_RECORD_BYTES,
                "Destination identifier budget exceeded"
            );
        }
        for name in &columns {
            ensure!(names.insert(name), "Duplicate destination column");
        }
        let mut bytes = 0usize;
        for row in &rows {
            ensure!(
                row.len() == columns.len(),
                "Batch row width does not match destination columns"
            );
            for value in row {
                ensure!(
                    value.byte_len() <= MAX_FIELD_BYTES,
                    "Batch field exceeds 256 KiB"
                );
                bytes = bytes.saturating_add(value.byte_len());
                ensure!(bytes <= MAX_RECORD_BYTES, "Batch values exceed 1 MiB");
            }
        }
        Ok(Self {
            schema,
            table,
            columns,
            rows,
        })
    }
    pub fn row_count(&self) -> u64 {
        self.rows.len() as u64
    }
    pub fn parameter_count(&self) -> usize {
        self.rows.len() * self.columns.len()
    }
    pub fn values(&self) -> impl Iterator<Item = &Value> {
        self.rows.iter().flatten()
    }
    pub fn into_values(self) -> impl Iterator<Item = Value> {
        self.rows.into_iter().flatten()
    }

    pub fn sql(&self, provider: DbDriverKind) -> Result<String> {
        ensure!(
            matches!(
                provider,
                DbDriverKind::Postgres
                    | DbDriverKind::MySql
                    | DbDriverKind::MariaDb
                    | DbDriverKind::Sqlite
            ),
            "Parameterized transfer batches are unavailable for this provider"
        );
        let dialect = SqlDialect(provider);
        let target = format!(
            "{}.{}",
            dialect.quote_ident(&self.schema),
            dialect.quote_ident(&self.table)
        );
        let columns = self
            .columns
            .iter()
            .map(|name| dialect.quote_ident(name))
            .collect::<Vec<_>>()
            .join(", ");
        let mut sql = format!("INSERT INTO {target} ({columns}) VALUES ");
        let mut parameter = 0;
        for row in 0..self.rows.len() {
            if row > 0 {
                sql.push_str(", ");
            }
            sql.push('(');
            for column in 0..self.columns.len() {
                if column > 0 {
                    sql.push_str(", ");
                }
                parameter += 1;
                if provider == DbDriverKind::Postgres {
                    use std::fmt::Write;
                    write!(sql, "${parameter}").expect("writing to String cannot fail");
                } else {
                    sql.push('?');
                }
            }
            sql.push(')');
        }
        Ok(sql)
    }
}

#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
