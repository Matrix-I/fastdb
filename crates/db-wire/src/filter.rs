//! Provider-aware structured table filters. Values are data, never SQL fragments.
use crate::{DbDriverKind, sql::SqlDialect};
use anyhow::{Result, bail};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterOp {
    Equal,
    NotEqual,
    Greater,
    GreaterEqual,
    Less,
    LessEqual,
    Like,
    IsNull,
    IsNotNull,
}
impl FilterOp {
    pub const ALL: [Self; 9] = [
        Self::Equal,
        Self::NotEqual,
        Self::Greater,
        Self::GreaterEqual,
        Self::Less,
        Self::LessEqual,
        Self::Like,
        Self::IsNull,
        Self::IsNotNull,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Equal => "=",
            Self::NotEqual => "<>",
            Self::Greater => ">",
            Self::GreaterEqual => ">=",
            Self::Less => "<",
            Self::LessEqual => "<=",
            Self::Like => "LIKE",
            Self::IsNull => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
        }
    }
    pub fn needs_value(self) -> bool {
        !matches!(self, Self::IsNull | Self::IsNotNull)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Criterion {
    pub column: String,
    pub op: FilterOp,
    pub value: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterOrder {
    pub column: String,
    pub descending: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterSpec {
    pub criteria: Vec<Criterion>,
    pub match_all: bool,
    pub order: Option<FilterOrder>,
}
impl Default for FilterSpec {
    fn default() -> Self {
        Self {
            criteria: Vec::new(),
            match_all: true,
            order: None,
        }
    }
}
pub(crate) fn value_literal(kind: DbDriverKind, value: &str) -> String {
    if matches!(kind, DbDriverKind::MySql | DbDriverKind::MariaDb) {
        // Hex text does not depend on NO_BACKSLASH_ESCAPES or connection encoding.
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut hex = String::with_capacity(value.len() * 2);
        for byte in value.as_bytes() {
            hex.push(char::from(HEX[(byte >> 4) as usize]));
            hex.push(char::from(HEX[(byte & 15) as usize]));
        }
        format!("CONVERT(X'{hex}' USING utf8mb4)")
    } else if kind == DbDriverKind::Postgres {
        format!("E'{}'", value.replace('\\', "\\\\").replace('\'', "''"))
    } else {
        SqlDialect(kind).literal(Some(value))
    }
}
impl FilterSpec {
    pub fn sql(&self, kind: DbDriverKind, columns: &[String]) -> Result<String> {
        if self.criteria.len() > 32 {
            bail!("A structured filter supports at most 32 criteria");
        }
        if self
            .criteria
            .iter()
            .filter(|c| c.op.needs_value())
            .map(|c| c.value.len())
            .sum::<usize>()
            > 65536
        {
            bail!("Filter values exceed 64 KiB");
        }
        let dialect = SqlDialect(kind);
        let mut conditions = Vec::new();
        for c in &self.criteria {
            if !columns.contains(&c.column) {
                bail!("Unknown column {}; refresh and reopen criteria", c.column);
            }
            if c.op.needs_value() && (c.value.len() > 8192 || c.value.contains('\0')) {
                bail!("Filter values must fit within 8 KiB and cannot contain NUL");
            }
            conditions.push(format!(
                "({} {}{})",
                dialect.quote_ident(&c.column),
                c.op.label(),
                if c.op.needs_value() {
                    format!(" {}", value_literal(kind, &c.value))
                } else {
                    String::new()
                }
            ));
        }
        let mut sql = if conditions.is_empty() {
            String::new()
        } else {
            format!(
                "WHERE {}",
                conditions.join(if self.match_all { " AND " } else { " OR " })
            )
        };
        if let Some(order) = &self.order {
            if !columns.contains(&order.column) {
                bail!("Unknown ordering column; refresh and reopen criteria");
            }
            if !sql.is_empty() {
                sql.push(' ');
            }
            sql.push_str(&format!(
                "ORDER BY {} {}",
                dialect.quote_ident(&order.column),
                if order.descending { "DESC" } else { "ASC" }
            ));
        }
        Ok(sql)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn criteria_quote_values_and_keep_null_distinct_from_text() {
        let spec = FilterSpec {
            criteria: vec![
                Criterion {
                    column: "odd\"name".into(),
                    op: FilterOp::Equal,
                    value: "x' OR 1=1 --".into(),
                },
                Criterion {
                    column: "nullable".into(),
                    op: FilterOp::IsNull,
                    value: "ignored".into(),
                },
            ],
            ..Default::default()
        };
        let columns = vec!["odd\"name".into(), "nullable".into()];
        let sql = spec.sql(DbDriverKind::Postgres, &columns).unwrap();
        assert_eq!(
            sql,
            "WHERE (\"odd\"\"name\" = E'x'' OR 1=1 --') AND (\"nullable\" IS NULL)"
        );
        let q = char::from(96);
        assert!(
            spec.sql(DbDriverKind::MySql, &columns)
                .unwrap()
                .contains(&format!("{q}odd\"name{q}"))
        );
        assert!(
            spec.sql(DbDriverKind::SqlServer, &columns)
                .unwrap()
                .contains("N'x'' OR")
        );
        assert!(spec.sql(DbDriverKind::Oracle, &columns).is_ok());
        assert!(spec.sql(DbDriverKind::Postgres, &[]).is_err());
    }
}
