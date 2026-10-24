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
            let val: Option<bool> = row.get(col_idx);
            val.map(|b| if b { b"true".to_vec() } else { b"false".to_vec() })
        }
        Type::INT2 => {
            let val: Option<i16> = row.get(col_idx);
            val.map(|n| n.to_string().into_bytes())
        }
        Type::INT4 => {
            let val: Option<i32> = row.get(col_idx);
            val.map(|n| n.to_string().into_bytes())
        }
        Type::INT8 => {
            let val: Option<i64> = row.get(col_idx);
            val.map(|n| n.to_string().into_bytes())
        }
        Type::FLOAT4 => {
            let val: Option<f32> = row.get(col_idx);
            val.map(|f| f.to_string().into_bytes())
        }
        Type::FLOAT8 => {
            let val: Option<f64> = row.get(col_idx);
            val.map(|f| f.to_string().into_bytes())
        }
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => {
            let val: Option<&str> = row.get(col_idx);
            val.map(|s| s.as_bytes().to_vec())
        }
        Type::BYTEA => {
            let val: Option<&[u8]> = row.get(col_idx);
            val.map(|b| b.to_vec())
        }
        _ => {
            // For other types or custom types, try getting as &str or fallback to string representation
            if let Ok(val) = row.try_get::<_, Option<&str>>(col_idx) {
                return val.map(|s| s.as_bytes().to_vec());
            }
            if let Ok(val) = row.try_get::<_, Option<String>>(col_idx) {
                return val.map(|s| s.into_bytes());
            }
            None
        }
    }
}
