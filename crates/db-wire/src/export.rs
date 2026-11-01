use crate::{DbDriverKind, sql::SqlDialect};
use anyhow::Result;
use mem_engine::QueryResultPager;
use std::io::Write;

/// Streams the entire dataset from QueryResultPager into CSV format without memory overhead
pub fn export_to_csv<W: Write>(pager: &QueryResultPager, writer: &mut W) -> Result<u64> {
    export_to_csv_rows(pager, writer, pager.total_rows())
}

/// Export a fixed prefix of fetched rows, even if more arrive during the export.
pub fn export_to_csv_rows<W: Write>(
    pager: &QueryResultPager,
    writer: &mut W,
    rows: u64,
) -> Result<u64> {
    let num_cols = pager.num_cols() as usize;

    // 1. Write Header
    for (i, col) in pager.columns.iter().enumerate() {
        if i > 0 {
            writer.write_all(b",")?;
        }
        write_csv_field(writer, col.name.as_bytes())?;
    }
    writer.write_all(b"\n")?;

    // 2. Stream Rows
    let total_rows = rows.min(pager.total_rows());
    for r in 0..total_rows {
        for c in 0..num_cols {
            if c > 0 {
                writer.write_all(b",")?;
            }
            if let Some(cell) = pager.get_cell(r, c) {
                write_csv_field(writer, &cell)?;
            }
        }
        writer.write_all(b"\n")?;
    }

    writer.flush()?;
    Ok(total_rows)
}

fn write_csv_field<W: Write>(writer: &mut W, bytes: &[u8]) -> Result<()> {
    let needs_quotes = bytes
        .iter()
        .any(|&b| b == b',' || b == b'"' || b == b'\n' || b == b'\r');
    if needs_quotes {
        writer.write_all(b"\"")?;
        for &b in bytes {
            if b == b'"' {
                writer.write_all(b"\"\"")?;
            } else {
                writer.write_all(&[b])?;
            }
        }
        writer.write_all(b"\"")?;
    } else {
        writer.write_all(bytes)?;
    }
    Ok(())
}

/// Streams data from QueryResultPager into SQL INSERT statements
pub fn export_to_sql_inserts<W: Write>(
    table_name: &str,
    pager: &QueryResultPager,
    writer: &mut W,
    batch_size: usize,
) -> Result<u64> {
    let dialect = SqlDialect(DbDriverKind::Postgres);
    export_to_sql_inserts_for_target(
        &dialect.quote_ident(table_name),
        dialect,
        pager,
        writer,
        batch_size,
        pager.total_rows(),
    )
}

/// Export a fixed row prefix. `target` must already be quoted with `SqlDialect`.
pub fn export_to_sql_inserts_for_target<W: Write>(
    target: &str,
    dialect: SqlDialect,
    pager: &QueryResultPager,
    writer: &mut W,
    batch_size: usize,
    rows: u64,
) -> Result<u64> {
    anyhow::ensure!(batch_size > 0, "SQL export batch size must be positive");
    anyhow::ensure!(pager.num_cols() > 0, "SQL export requires result columns");
    let num_cols = pager.num_cols() as usize;
    let total_rows = rows.min(pager.total_rows());
    let col_names: Vec<String> = pager
        .columns
        .iter()
        .map(|c| dialect.quote_ident(&c.name))
        .collect();
    let col_list = col_names.join(", ");

    let mut current_batch = 0;

    for r in 0..total_rows {
        if current_batch == 0 {
            writeln!(writer, "INSERT INTO {target} ({col_list}) VALUES")?;
        }

        writer.write_all(b"  (")?;
        for c in 0..num_cols {
            if c > 0 {
                writer.write_all(b", ")?;
            }
            if let Some(cell) = pager.get_cell(r, c) {
                let s = String::from_utf8_lossy(&cell);
                write!(writer, "{}", dialect.literal(Some(&s)))?;
            } else {
                writer.write_all(b"NULL")?;
            }
        }

        current_batch += 1;
        if current_batch >= batch_size || r + 1 == total_rows {
            writer.write_all(b");\n")?;
            current_batch = 0;
        } else {
            writer.write_all(b"),\n")?;
        }
    }

    writer.flush()?;
    Ok(total_rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mem_engine::{ColumnMeta, DataType};

    #[test]
    fn csv_quotes_headers_and_multiline_values_and_limits_rows() {
        let pager = QueryResultPager::new(vec![
            ColumnMeta::new("a,\"b", DataType::Text),
            ColumnMeta::new("empty", DataType::Text),
        ])
        .unwrap();
        pager
            .append_row([Some("line\r\n\"quoted\""), None])
            .unwrap();
        pager.append_row([Some("later"), Some("")]).unwrap();
        let mut bytes = Vec::new();
        assert_eq!(export_to_csv_rows(&pager, &mut bytes, 1).unwrap(), 1);
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "\"a,\"\"b\",empty\n\"line\r\n\"\"quoted\"\"\",\n"
        );
        let mut bytes = Vec::new();
        assert_eq!(export_to_csv_rows(&pager, &mut bytes, 0).unwrap(), 0);
        assert_eq!(String::from_utf8(bytes).unwrap(), "\"a,\"\"b\",empty\n");
    }

    #[test]
    fn sql_batches_quote_identifiers_preserve_null_and_limit_rows() {
        let pager = QueryResultPager::new(vec![ColumnMeta::new("a\"b", DataType::Text)]).unwrap();
        pager.append_row([Some("O'Reilly")]).unwrap();
        pager.append_row([None::<&str>]).unwrap();
        pager.append_row([Some("")]).unwrap();
        let mut bytes = Vec::new();
        assert_eq!(
            export_to_sql_inserts("t\"x", &pager, &mut bytes, 2).unwrap(),
            3
        );
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "INSERT INTO \"t\"\"x\" (\"a\"\"b\") VALUES\n  ('O''Reilly'),\n  (NULL);\nINSERT INTO \"t\"\"x\" (\"a\"\"b\") VALUES\n  ('');\n"
        );
        let mut bytes = Vec::new();
        let mysql = SqlDialect(DbDriverKind::MySql);
        export_to_sql_inserts_for_target("`db`.`table`", mysql, &pager, &mut bytes, 1, 1).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "INSERT INTO `db`.`table` (`a\"b`) VALUES\n  ('O''Reilly');\n"
        );
        assert!(export_to_sql_inserts("t", &pager, &mut Vec::new(), 0).is_err());
    }

    #[test]
    fn sqlite_sql_export_round_trips_text_and_nulls() {
        let pager = QueryResultPager::new(vec![ColumnMeta::new("select", DataType::Text)]).unwrap();
        pager.append_row([Some("Tiếng Việt\nO'Reilly\\")]).unwrap();
        pager.append_row([None::<&str>]).unwrap();
        pager.append_row([Some("")]).unwrap();
        let mut bytes = Vec::new();
        export_to_sql_inserts("order", &pager, &mut bytes, 2).unwrap();
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE \"order\" (\"select\" TEXT)")
            .unwrap();
        connection
            .execute_batch(std::str::from_utf8(&bytes).unwrap())
            .unwrap();
        let values: Vec<Option<String>> = connection
            .prepare("SELECT \"select\" FROM \"order\"")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            values,
            vec![Some("Tiếng Việt\nO'Reilly\\".into()), None, Some("".into())]
        );
    }

    #[test]
    fn export_propagates_writer_failures() {
        struct FailedWriter;
        impl Write for FailedWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk full"))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let pager = QueryResultPager::new(vec![ColumnMeta::new("x", DataType::Text)]).unwrap();
        pager.append_row([Some("value")]).unwrap();
        assert!(export_to_csv(&pager, &mut FailedWriter).is_err());
        assert!(export_to_sql_inserts("t", &pager, &mut FailedWriter, 1).is_err());
    }

    #[test]
    fn test_export_to_csv() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int4),
            ColumnMeta::new("name", DataType::Text),
        ];
        let pager = QueryResultPager::new(cols).unwrap();
        pager
            .append_row(vec![Some(&b"1"[..]), Some(&b"Alice, Jr."[..])])
            .unwrap();
        pager.append_row(vec![Some(&b"2"[..]), None]).unwrap();

        let mut buf = Vec::new();
        let rows = export_to_csv(&pager, &mut buf).unwrap();
        assert_eq!(rows, 2);

        let output = String::from_utf8(buf).unwrap();
        assert_eq!(output, "id,name\n1,\"Alice, Jr.\"\n2,\n");
    }

    #[test]
    fn test_export_to_sql() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int4),
            ColumnMeta::new("title", DataType::Text),
        ];
        let pager = QueryResultPager::new(cols).unwrap();
        pager
            .append_row(vec![Some(&b"1"[..]), Some(&b"O'Reilly"[..])])
            .unwrap();

        let mut buf = Vec::new();
        let rows = export_to_sql_inserts("books", &pager, &mut buf, 10).unwrap();
        assert_eq!(rows, 1);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("INSERT INTO \"books\" (\"id\", \"title\") VALUES"));
        assert!(output.contains("('1', 'O''Reilly');"));
    }
}
