//! Write loaded results as a JSON array of objects keyed by column name.
use db_wire::{DataType, DbDriverKind, QueryResultPager, value};
use std::collections::HashSet;
use std::io::Write;

pub(super) fn write_json(
    pager: &QueryResultPager,
    writer: &mut impl Write,
    rows: u64,
    provider: DbDriverKind,
) -> anyhow::Result<u64> {
    let mut keys = HashSet::with_capacity(pager.columns.len());
    for column in &pager.columns {
        anyhow::ensure!(
            keys.insert(column.name.as_str()),
            "JSON export cannot represent duplicate column name {:?}. Give the columns distinct SQL aliases.",
            column.name
        );
    }

    writer.write_all(b"[")?;
    let rows = rows.min(pager.total_rows());
    for row in 0..rows {
        writer.write_all(if row == 0 { b"\n  {" } else { b",\n  {" })?;
        for (index, column) in pager.columns.iter().enumerate() {
            if index > 0 {
                writer.write_all(b",")?;
            }
            serde_json::to_writer(&mut *writer, &column.name)?;
            writer.write_all(b":")?;
            let bytes = pager.get_cell(row, index);
            write_value(writer, bytes.as_deref(), column.data_type, provider)?;
        }
        writer.write_all(b"}")?;
    }
    if rows > 0 {
        writer.write_all(b"\n")?;
    }
    writer.write_all(b"]\n")?;
    Ok(rows)
}

fn write_value(
    writer: &mut impl Write,
    bytes: Option<&[u8]>,
    kind: DataType,
    provider: DbDriverKind,
) -> anyhow::Result<()> {
    let Some(bytes) = bytes else {
        writer.write_all(b"null")?;
        return Ok(());
    };
    if kind == DataType::Bytea {
        serde_json::to_writer(
            writer,
            &value::hex_text(&value::source_binary(bytes, provider)?),
        )?;
        return Ok(());
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        serde_json::to_writer(writer, &value::hex_text(bytes))?;
        return Ok(());
    };
    match kind {
        DataType::Bool if matches!(text, "true" | "1") => writer.write_all(b"true")?,
        DataType::Bool if matches!(text, "false" | "0") => writer.write_all(b"false")?,
        DataType::Int2
        | DataType::Int4
        | DataType::Int8
        | DataType::Numeric
        | DataType::Float4
        | DataType::Float8
            if json_number(text) =>
        {
            writer.write_all(text.trim().as_bytes())?;
        }
        // JSON columns stay as source text so JSON null differs from SQL NULL
        // and embedded numbers and duplicate keys survive.
        _ => serde_json::to_writer(writer, text)?,
    }
    Ok(())
}

fn json_number(text: &str) -> bool {
    let text = text.trim();
    matches!(text.as_bytes().first(), Some(b'-' | b'0'..=b'9'))
        && value::validate_json_source(text).is_ok()
}
