use mem_engine::DataType;
use tokio_postgres::types::Type;
use tokio_postgres::Row;

pub fn pg_type_to_data_type(t: &Type) -> DataType {
    match *t {
        Type::BOOL => DataType::Bool,
        Type::INT2 => DataType::Int2,
        Type::INT4 => DataType::Int4,
        Type::INT8 => DataType::Int8,
        Type::FLOAT4 => DataType::Float4,
        Type::FLOAT8 => DataType::Float8,
        Type::NUMERIC => DataType::Numeric,
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => DataType::Text,
        Type::DATE => DataType::Date,
        Type::TIME => DataType::Time,
        Type::TIMESTAMP => DataType::Timestamp,
        Type::TIMESTAMPTZ => DataType::TimestampTz,
        Type::UUID => DataType::Uuid,
        Type::JSON | Type::JSONB => DataType::Json,
        Type::BYTEA => DataType::Bytea,
        _ => DataType::Unknown,
    }
}

/// Decodes a single cell from a tokio-postgres Row into byte representation
pub fn decode_pg_cell(row: &Row, col_idx: usize) -> Option<Vec<u8>> {
    let col_type = row.columns()[col_idx].type_();

    match *col_type {
        Type::BOOL => {
            row.try_get::<_, Option<bool>>(col_idx).ok().flatten()
                .map(|b| if b { b"true".to_vec() } else { b"false".to_vec() })
        }
        Type::INT2 => {
            row.try_get::<_, Option<i16>>(col_idx).ok().flatten()
                .map(|n| n.to_string().into_bytes())
        }
        Type::INT4 => {
            row.try_get::<_, Option<i32>>(col_idx).ok().flatten()
                .map(|n| n.to_string().into_bytes())
        }
        Type::INT8 => {
            row.try_get::<_, Option<i64>>(col_idx).ok().flatten()
                .map(|n| n.to_string().into_bytes())
        }
        Type::FLOAT4 => {
            row.try_get::<_, Option<f32>>(col_idx).ok().flatten()
                .map(|f| f.to_string().into_bytes())
        }
        Type::FLOAT8 => {
            row.try_get::<_, Option<f64>>(col_idx).ok().flatten()
                .map(|f| f.to_string().into_bytes())
        }
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
            row.try_get::<_, Option<&str>>(col_idx).ok().flatten()
                .map(|s| s.as_bytes().to_vec())
        }
        Type::BYTEA => {
            row.try_get::<_, Option<&[u8]>>(col_idx).ok().flatten()
                .map(|b| b.to_vec())
        }
        Type::UUID => {
            row.try_get::<_, Option<uuid::Uuid>>(col_idx).ok().flatten()
                .map(|u| u.to_string().into_bytes())
        }
        Type::TIMESTAMP => {
            row.try_get::<_, Option<chrono::NaiveDateTime>>(col_idx).ok().flatten()
                .map(|dt| dt.to_string().into_bytes())
        }
        Type::TIMESTAMPTZ => {
            row.try_get::<_, Option<chrono::DateTime<chrono::Utc>>>(col_idx).ok().flatten()
                .map(|dt| dt.to_string().into_bytes())
        }
        Type::DATE => {
            row.try_get::<_, Option<chrono::NaiveDate>>(col_idx).ok().flatten()
                .map(|d| d.to_string().into_bytes())
        }
        Type::TIME => {
            row.try_get::<_, Option<chrono::NaiveTime>>(col_idx).ok().flatten()
                .map(|t| t.to_string().into_bytes())
        }
        Type::JSON | Type::JSONB => {
            row.try_get::<_, Option<serde_json::Value>>(col_idx).ok().flatten()
                .map(|j| j.to_string().into_bytes())
        }
        _ => {
            if let Ok(val) = row.try_get::<_, Option<&str>>(col_idx) {
                if let Some(s) = val {
                    return Some(s.as_bytes().to_vec());
                }
            }
            if let Ok(val) = row.try_get::<_, Option<String>>(col_idx) {
                if let Some(s) = val {
                    return Some(s.into_bytes());
                }
            }
            if let Ok(val) = row.try_get::<_, Option<i64>>(col_idx) {
                if let Some(n) = val {
                    return Some(n.to_string().into_bytes());
                }
            }
            if let Ok(val) = row.try_get::<_, Option<f64>>(col_idx) {
                if let Some(f) = val {
                    return Some(f.to_string().into_bytes());
                }
            }
            None
        }
    }
}
