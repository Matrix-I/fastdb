use super::SqlDialect;
use crate::{ColumnMeta, DbDriverKind};

/// A partial metadata preview, never an exact recreation of the source table.
pub fn generate_table_ddl(
    kind: DbDriverKind,
    schema: &str,
    table: &str,
    cols: &[ColumnMeta],
    constraints: &[(String, String)],
    indexes: &[String],
) -> String {
    let dialect = SqlDialect(kind);
    if cols.is_empty()
        || cols
            .iter()
            .any(|c| c.raw_type.as_deref().is_none_or(str::is_empty))
    {
        return "-- DDL unavailable: native column type metadata is incomplete.".into();
    }
    let mut ddl = "-- Partial metadata preview; review before execution.\n-- Foreign keys, checks, generated columns and index definitions may be unavailable.\n".to_string();
    ddl.push_str(&format!(
        "CREATE TABLE {} (\n",
        dialect.qualified_table(schema, table)
    ));
    let mut lines = Vec::new();
    for col in cols {
        let default = col
            .default_val
            .as_ref()
            .map(|d| format!(" DEFAULT {d}"))
            .unwrap_or_default();
        lines.push(format!(
            "    {} {}{} {}",
            dialect.quote_ident(&col.name),
            col.display_type(),
            default,
            if col.nullable { "NULL" } else { "NOT NULL" }
        ));
    }
    let pk: Vec<_> = cols
        .iter()
        .filter(|c| c.is_primary_key)
        .map(|c| dialect.quote_ident(&c.name))
        .collect();
    if !pk.is_empty() {
        lines.push(format!("    PRIMARY KEY ({})", pk.join(", ")));
    }
    ddl.push_str(&lines.join(",\n"));
    ddl.push_str("\n);\n");
    for (name, _) in constraints {
        ddl.push_str(&format!(
            "-- Constraint metadata: {} (definition unavailable)\n",
            dialect.quote_ident(name).replace(['\r', '\n'], " ")
        ));
    }
    for name in indexes {
        ddl.push_str(&format!(
            "-- Index metadata: {} (definition unavailable)\n",
            dialect.quote_ident(name).replace(['\r', '\n'], " ")
        ));
    }
    ddl
}
#[cfg(test)]
mod relocated_tests {
    #[test]
    fn test_table_properties_column_meta_and_ddl_generation() {
        use super::generate_table_ddl;
        use crate::{ColumnMeta, DataType, DbDriverKind};

        let cols = vec![
            ColumnMeta::new("key", DataType::Varchar)
                .with_nullable(false)
                .with_pk(true)
                .with_raw_type("varchar")
                .with_collation("default"),
            ColumnMeta::new("value", DataType::Json)
                .with_nullable(true)
                .with_raw_type("jsonb"),
        ];

        assert_eq!(cols[0].display_type(), "varchar");
        assert_eq!(cols[1].display_type(), "jsonb");
        assert!(!cols[0].nullable);
        assert!(cols[1].nullable);
        assert!(cols[0].is_primary_key);

        let ddl = generate_table_ddl(
            DbDriverKind::Postgres,
            "public",
            "additionalInfo",
            &cols,
            &[],
            &[],
        );
        assert!(ddl.contains("CREATE TABLE \"public\".\"additionalInfo\""));
        assert!(ddl.contains("\"key\" varchar NOT NULL"));
        assert!(ddl.contains("\"value\" jsonb NULL"));
        assert!(ddl.contains("PRIMARY KEY (\"key\")"));
    }
}
