//! Bounded local validation before opening a destination session.
use super::{
    csv::{CsvOptions, CsvReader},
    mapping::{Mapping, Value},
};
use anyhow::Result;
use std::io::Read;

pub const PREVIEW_ROWS: usize = 8;

#[derive(Debug)]
pub struct PreviewRow {
    pub record: u64,
    pub converted: Result<Vec<Value>>,
}

#[derive(Debug)]
pub struct Preview {
    pub headers: Option<Vec<String>>,
    pub rows: Vec<PreviewRow>,
    /// False if sampling stopped at the limit, even if the file ends there.
    pub reached_end: bool,
}

impl Preview {
    pub fn read(input: impl Read, options: CsvOptions, mapping: &Mapping) -> Result<Self> {
        let mut csv = CsvReader::new(input, options)?;
        let mut preview = Self {
            headers: csv.headers().map(<[String]>::to_vec),
            rows: Vec::with_capacity(PREVIEW_ROWS),
            reached_end: false,
        };
        for _ in 0..PREVIEW_ROWS {
            let Some(row) = csv.next_record()? else {
                preview.reached_end = true;
                break;
            };
            preview.rows.push(PreviewRow {
                record: csv.record_number(),
                converted: mapping.convert(&csv_values(row)),
            });
        }
        Ok(preview)
    }
}

pub(super) fn csv_values(row: Vec<Option<String>>) -> Vec<Value> {
    row.into_iter()
        .map(|field| field.map_or(Value::Null, Value::Text))
        .collect()
}
