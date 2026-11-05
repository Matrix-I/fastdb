//! Bind native PostgreSQL types in their text wire format without client numeric conversion.
use crate::transfer::{batch::Batch, mapping::Value};
use anyhow::{Context, Result};
use bytes::BytesMut;
use postgres_types::{Format, IsNull, Kind, ToSql, Type};
use std::{error::Error, fmt};

struct Parameter<'a>(&'a Value);
impl fmt::Debug for Parameter<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TransferParameter(<redacted>)")
    }
}
impl ToSql for Parameter<'_> {
    fn to_sql(
        &self,
        ty: &Type,
        output: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        match self.0 {
            Value::Null => return Ok(IsNull::Yes),
            Value::Boolean(value) => output.extend_from_slice(if *value { b"1" } else { b"0" }),
            Value::Text(text) => {
                if text.contains('\0') {
                    return Err("PostgreSQL text parameters cannot contain NUL".into());
                }
                output.extend_from_slice(text.as_bytes());
            }
            Value::Binary(bytes) => {
                let mut base = ty;
                while let Kind::Domain(inner) = base.kind() {
                    base = inner;
                }
                if *base != Type::BYTEA {
                    return Err("Binary transfer value requires a bytea destination".into());
                }
                const HEX: &[u8; 16] = b"0123456789abcdef";
                output.extend_from_slice(b"\\x");
                for byte in bytes {
                    output
                        .extend_from_slice(&[HEX[(byte >> 4) as usize], HEX[(byte & 15) as usize]]);
                }
            }
        }
        Ok(IsNull::No)
    }
    fn accepts(_: &Type) -> bool {
        true
    }
    fn encode_format(&self, _: &Type) -> Format {
        Format::Text
    }
    postgres_types::to_sql_checked!();
}

pub(super) async fn insert(
    client: &tokio_postgres::Client,
    batch: Batch,
    expected_relation: Option<u32>,
) -> Result<u64> {
    let mut sql = batch.sql(crate::DbDriverKind::Postgres)?;
    if expected_relation.is_some() {
        sql.push_str(" RETURNING tableoid");
    }
    let statement = client
        .prepare(&sql)
        .await
        .context("Cannot prepare transfer batch")?;
    if let Some(expected) = expected_relation {
        anyhow::ensure!(
            statement
                .columns()
                .first()
                .and_then(|column| column.table_oid())
                == Some(expected),
            "PostgreSQL destination identity changed before batch execution"
        );
    }
    let values: Vec<_> = batch.values().map(Parameter).collect();
    let parameters: Vec<&(dyn ToSql + Sync)> = values
        .iter()
        .map(|value| value as &(dyn ToSql + Sync))
        .collect();
    client
        .execute(&statement, &parameters)
        .await
        .context("PostgreSQL transfer batch failed")
}

#[cfg(test)]
mod tests {
    use super::{BytesMut, Parameter, ToSql, Type, Value};
    #[test]
    fn parameters_are_redacted_and_reject_nul_or_binary_to_text_coercion() {
        let value = Value::Text("private value".into());
        assert!(!format!("{:?}", Parameter(&value)).contains("private"));
        assert!(
            Parameter(&Value::Text("bad\0text".into()))
                .to_sql(&Type::TEXT, &mut BytesMut::new())
                .is_err()
        );
        assert!(
            Parameter(&Value::Binary(vec![0, 255]))
                .to_sql(&Type::TEXT, &mut BytesMut::new())
                .is_err()
        );
        let mut output = BytesMut::new();
        Parameter(&Value::Binary(vec![0, 255]))
            .to_sql(&Type::BYTEA, &mut output)
            .unwrap();
        assert_eq!(output.as_ref(), b"\\x00ff");
    }
}
