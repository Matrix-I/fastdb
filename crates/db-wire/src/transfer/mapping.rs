//! Column selection and deterministic validation before destination binding.
use super::csv::{MAX_COLUMNS, MAX_FIELD_BYTES, MAX_RECORD_BYTES};
use crate::{ColumnMeta, DataType};
use anyhow::{Result, bail, ensure};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Null,
    Boolean(bool),
    Text(String),
    Binary(Vec<u8>),
}
impl Value {
    pub fn byte_len(&self) -> usize {
        match self {
            Self::Null => 0,
            Self::Boolean(_) => 1,
            Self::Text(value) => value.len(),
            Self::Binary(value) => value.len(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Conversion {
    #[default]
    Auto,
    /// Pass through text; explicitly represent binary as canonical hexadecimal text.
    Text,
    Integer,
    Decimal,
    Float,
    Boolean,
    Date,
    Time,
    Timestamp,
    TimestampTz,
    Json,
    Uuid,
    BinaryHex,
}
#[derive(Clone, Debug)]
pub struct ColumnMapping {
    pub source: usize,
    pub target: ColumnMeta,
    pub conversion: Conversion,
}

#[derive(Clone, Debug)]
pub struct Mapping {
    source_width: usize,
    columns: Vec<ColumnMapping>,
}
impl Mapping {
    pub fn new(source_width: usize, columns: Vec<ColumnMapping>) -> Result<Self> {
        ensure!(
            (1..=MAX_COLUMNS).contains(&source_width),
            "Source requires 1–1,024 columns"
        );
        ensure!(
            !columns.is_empty() && columns.len() <= MAX_COLUMNS,
            "Select 1–1,024 destination columns"
        );
        let mut names = HashSet::new();
        for column in &columns {
            ensure!(
                !column.target.is_generated,
                "Destination column is generated: {}",
                column.target.name
            );
            ensure!(
                column.source < source_width,
                "Mapped source column is out of range"
            );
            ensure!(
                !column.target.name.is_empty() && !column.target.name.contains('\0'),
                "Destination column name is invalid"
            );
            ensure!(
                names.insert(&column.target.name),
                "Destination column is mapped more than once"
            );
            if column.target.data_type == DataType::Bool {
                ensure!(
                    matches!(
                        column.conversion,
                        Conversion::Auto | Conversion::Boolean | Conversion::Integer
                    ),
                    "Boolean destinations require Boolean or Integer conversion"
                );
            }
        }
        Ok(Self {
            source_width,
            columns,
        })
    }
    pub fn columns(&self) -> &[ColumnMapping] {
        &self.columns
    }

    /// Unmapped destination columns are omitted, so native defaults remain native.
    pub fn convert(&self, row: &[Value]) -> Result<Vec<Value>> {
        ensure!(
            row.len() == self.source_width,
            "Source row width does not match the mapping"
        );
        let mut input_bytes = 0usize;
        for value in row {
            ensure!(
                value.byte_len() <= MAX_FIELD_BYTES,
                "Source field exceeds 256 KiB"
            );
            input_bytes = input_bytes.saturating_add(value.byte_len());
            ensure!(input_bytes <= MAX_RECORD_BYTES, "Source row exceeds 1 MiB");
        }
        let mut output = Vec::with_capacity(self.columns.len());
        let mut bytes = 0usize;
        for column in &self.columns {
            let value = convert_value(&row[column.source], column).map_err(|error| {
                let message = format!("Column {}: {error}", column.target.name);
                error.context(message)
            })?;
            ensure!(
                value.byte_len() <= MAX_FIELD_BYTES,
                "Converted field exceeds 256 KiB"
            );
            bytes = bytes.saturating_add(value.byte_len());
            ensure!(bytes <= MAX_RECORD_BYTES, "Converted row exceeds 1 MiB");
            output.push(value);
        }
        Ok(output)
    }
}

fn automatic(kind: DataType) -> Conversion {
    match kind {
        DataType::Bool => Conversion::Boolean,
        DataType::Int2 | DataType::Int4 | DataType::Int8 => Conversion::Integer,
        DataType::Float4 | DataType::Float8 => Conversion::Float,
        DataType::Numeric => Conversion::Decimal,
        DataType::Date => Conversion::Date,
        DataType::Time => Conversion::Time,
        DataType::Timestamp => Conversion::Timestamp,
        DataType::TimestampTz => Conversion::TimestampTz,
        DataType::Json => Conversion::Json,
        DataType::Uuid => Conversion::Uuid,
        DataType::Bytea => Conversion::BinaryHex,
        _ => Conversion::Text,
    }
}
fn convert_value(value: &Value, column: &ColumnMapping) -> Result<Value> {
    if *value == Value::Null {
        ensure!(
            column.target.nullable || column.target.generates_key_on_null,
            "Destination does not accept SQL NULL"
        );
        return Ok(Value::Null);
    }
    let conversion = if column.conversion == Conversion::Auto {
        automatic(column.target.data_type)
    } else {
        column.conversion
    };
    if let Value::Boolean(value) = value {
        return match conversion {
            Conversion::Boolean => Ok(boolean(*value, &column.target)),
            Conversion::Integer | Conversion::Decimal | Conversion::Float => {
                Ok(if column.target.data_type == DataType::Bool {
                    Value::Boolean(*value)
                } else {
                    Value::Text(if *value { "1" } else { "0" }.into())
                })
            }
            Conversion::Text | Conversion::Json => Ok(Value::Text(value.to_string())),
            _ => bail!("Boolean source is incompatible with the selected conversion"),
        };
    }
    if let Value::Binary(bytes) = value {
        return match conversion {
            Conversion::BinaryHex => Ok(Value::Binary(bytes.clone())),
            Conversion::Text if column.conversion == Conversion::Text => {
                Ok(Value::Text(crate::value::hex_text(bytes)))
            }
            _ => {
                bail!("Choose Text conversion to represent binary as hex, or a binary destination")
            }
        };
    }
    let Value::Text(text) = value else {
        unreachable!("NULL and binary handled above")
    };
    let valid = match conversion {
        Conversion::Auto => unreachable!("Automatic conversion resolved above"),
        Conversion::Text => true,
        Conversion::BinaryHex => return Ok(Value::Binary(crate::value::parse_hex(text)?)),
        Conversion::Integer => integer(text, &column.target),
        Conversion::Decimal => decimal(text),
        Conversion::Float if column.target.data_type == DataType::Float4 => {
            text.parse::<f32>().is_ok_and(f32::is_finite)
        }
        Conversion::Float => text.parse::<f64>().is_ok_and(f64::is_finite),
        Conversion::Boolean => {
            return match text.to_ascii_lowercase().as_str() {
                "true" | "1" => Ok(boolean(true, &column.target)),
                "false" | "0" => Ok(boolean(false, &column.target)),
                _ => bail!("Expected true, false, 1 or 0"),
            };
        }
        Conversion::Date => chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok(),
        Conversion::Time => chrono::NaiveTime::parse_from_str(text, "%H:%M:%S%.f").is_ok(),
        Conversion::Timestamp => ["%Y-%m-%d %H:%M:%S%.f", "%Y-%m-%dT%H:%M:%S%.f"]
            .iter()
            .any(|format| chrono::NaiveDateTime::parse_from_str(text, format).is_ok()),
        Conversion::TimestampTz => crate::value::parse_timestamp_with_offset(text).is_ok(),
        Conversion::Json => crate::value::validate_json_source(text).is_ok(),
        Conversion::Uuid => uuid::Uuid::parse_str(text).is_ok(),
    };
    ensure!(valid, "Invalid value for the selected conversion");
    if column.target.data_type == DataType::Bool && conversion == Conversion::Integer {
        return match text.parse::<i64>() {
            Ok(0) => Ok(Value::Boolean(false)),
            Ok(1) => Ok(Value::Boolean(true)),
            _ => bail!("Integer Boolean values must be 0 or 1"),
        };
    }
    // Keep numeric/JSON tokens exact; destination types perform their own native rounding.
    Ok(Value::Text(text.clone()))
}

fn boolean(value: bool, target: &ColumnMeta) -> Value {
    match target.data_type {
        DataType::Text | DataType::Varchar | DataType::Json | DataType::Unknown => {
            Value::Text(value.to_string())
        }
        _ => Value::Boolean(value),
    }
}

fn integer(text: &str, target: &ColumnMeta) -> bool {
    let unsigned = target.raw_type.as_ref().is_some_and(|name| {
        name.split_whitespace()
            .any(|word| word.eq_ignore_ascii_case("unsigned"))
    });
    if unsigned {
        match target.data_type {
            DataType::Int2 => text.parse::<u16>().is_ok(),
            DataType::Int4 => text.parse::<u32>().is_ok(),
            _ => text.parse::<u64>().is_ok(),
        }
    } else {
        match target.data_type {
            DataType::Int2 => text.parse::<i16>().is_ok(),
            DataType::Int4 => text.parse::<i32>().is_ok(),
            _ => text.parse::<i64>().is_ok(),
        }
    }
}

fn decimal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut position = usize::from(bytes.first().is_some_and(|byte| b"+-".contains(byte)));
    let mut digits = 0;
    while bytes.get(position).is_some_and(u8::is_ascii_digit) {
        position += 1;
        digits += 1;
    }
    if bytes.get(position) == Some(&b'.') {
        position += 1;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return false;
    }
    if bytes.get(position).is_some_and(|byte| b"eE".contains(byte)) {
        position += 1;
        if bytes.get(position).is_some_and(|byte| b"+-".contains(byte)) {
            position += 1;
        }
        let start = position;
        while bytes.get(position).is_some_and(u8::is_ascii_digit) {
            position += 1;
        }
        if position == start {
            return false;
        }
    }
    position == bytes.len()
}

#[cfg(test)]
#[path = "mapping_tests.rs"]
mod tests;
