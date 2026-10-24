use std::io::Write;
use anyhow::Result;
use mem_engine::QueryResultPager;

/// Streams the entire dataset from QueryResultPager into CSV format without memory overhead
pub fn export_to_csv<W: Write>(pager: &QueryResultPager, writer: &mut W) -> Result<u64> {
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
    let total_rows = pager.total_rows();
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
    let needs_quotes = bytes.iter().any(|&b| b == b',' || b == b'"' || b == b'\n' || b == b'\r');
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
    let num_cols = pager.num_cols() as usize;
    let total_rows = pager.total_rows();
    let col_names: Vec<&str> = pager.columns.iter().map(|c| c.name.as_str()).collect();
    let col_list = col_names.join(", ");

    let mut current_batch = 0;

    for r in 0..total_rows {
        if current_batch == 0 {
            write!(writer, "INSERT INTO \"{table_name}\" ({col_list}) VALUES\n")?;
        }

        writer.write_all(b"  (")?;
        for c in 0..num_cols {
            if c > 0 {
                writer.write_all(b", ")?;
            }
            if let Some(cell) = pager.get_cell(r, c) {
                let s = String::from_utf8_lossy(&cell);
                let escaped = s.replace('\'', "''");
                write!(writer, "'{escaped}'")?;
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
    fn test_export_to_csv() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int4),
            ColumnMeta::new("name", DataType::Text),
        ];
        let pager = QueryResultPager::new(cols).unwrap();
        pager.append_row(vec![Some(&b"1"[..]), Some(&b"Alice, Jr."[..])]).unwrap();
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
        pager.append_row(vec![Some(&b"1"[..]), Some(&b"O'Reilly"[..])]).unwrap();

        let mut buf = Vec::new();
        let rows = export_to_sql_inserts("books", &pager, &mut buf, 10).unwrap();
        assert_eq!(rows, 1);

        let output = String::from_utf8(buf).unwrap();
        assert!(output.contains("INSERT INTO \"books\" (id, title) VALUES"));
        assert!(output.contains("('1', 'O''Reilly');"));
    }
}
