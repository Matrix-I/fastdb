use super::*;

pub(super) fn render_dbeaver_col_badge(dtype_str: &str) -> Div {
    let lower = dtype_str.to_lowercase();
    if lower.contains("json") {
        div()
            .w(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(10.5))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0x38bdf8))
            .child("{ }")
    } else if lower.contains("varchar") || lower.contains("text") || lower.contains("char") {
        div()
            .w(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.0))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0x4daafc))
            .child("A-Z")
    } else if lower.contains("int")
        || lower.contains("num")
        || lower.contains("float")
        || lower.contains("dec")
        || lower.contains("double")
    {
        div()
            .w(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.5))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0xa78bfa))
            .child("123")
    } else if lower.contains("date") || lower.contains("time") {
        div()
            .w(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.5))
            .child("📅")
    } else if lower.contains("bool") {
        div()
            .w(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.5))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0xf43f5e))
            .child("✓")
    } else if lower.contains("uuid") {
        div()
            .w(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(8.0))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0xfbbf24))
            .child("UUID")
    } else {
        div()
            .w(px(20.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.0))
            .font_weight(FontWeight::BOLD)
            .text_color(rgb(0x4daafc))
            .child("A-Z")
    }
}

pub fn generate_table_ddl(
    schema: &str,
    table: &str,
    cols: &[ColumnMeta],
    constraints: &[(String, String)],
    indexes: &[String],
) -> String {
    let mut ddl = String::new();
    ddl.push_str("-- Drop table\n\n");
    ddl.push_str(&format!("-- DROP TABLE {}.\"{}\";\n\n", schema, table));
    ddl.push_str(&format!("CREATE TABLE {}.\"{}\" (\n", schema, table));

    let mut lines = Vec::new();
    let mut pk_cols = Vec::new();

    for c in cols {
        let nullability = if c.nullable { "NULL" } else { "NOT NULL" };
        let def = if let Some(ref d) = c.default_val {
            format!(" DEFAULT {}", d)
        } else {
            String::new()
        };
        let coll = if let Some(ref cl) = c.collation {
            if cl != "default" {
                format!(" COLLATE \"{}\"", cl)
            } else {
                String::new()
            }
        } else {
            String::new()
        };
        lines.push(format!(
            "\t\"{}\" {}{}{} {}",
            c.name,
            c.display_type(),
            coll,
            def,
            nullability
        ));
        if c.is_primary_key {
            pk_cols.push(format!("\"{}\"", c.name));
        }
    }

    if !pk_cols.is_empty() {
        lines.push(format!(
            "\tCONSTRAINT {}_pkey PRIMARY KEY ({})",
            table,
            pk_cols.join(", ")
        ));
    } else {
        for (cname, ctype) in constraints {
            if ctype.contains("PRIMARY KEY") || ctype.contains("UNIQUE") {
                lines.push(format!("\tCONSTRAINT {} {}", cname, ctype));
            }
        }
    }

    ddl.push_str(&lines.join(",\n"));
    ddl.push_str("\n);\n");

    if !indexes.is_empty() {
        ddl.push_str("\n-- Indexes\n");
        for idx in indexes {
            if !idx.ends_with("_pkey") {
                ddl.push_str(&format!(
                    "CREATE INDEX {} ON {}.\"{}\" USING btree (...);\n",
                    idx, schema, table
                ));
            }
        }
    }

    ddl
}

#[cfg(test)]
mod relocated_tests {
    #[test]
    fn test_table_properties_column_meta_and_ddl_generation() {
        use crate::workspace::generate_table_ddl;
        use db_wire::{ColumnMeta, DataType};

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

        let ddl = generate_table_ddl("public", "additionalInfo", &cols, &[], &[]);
        assert!(ddl.contains("CREATE TABLE public.\"additionalInfo\""));
        assert!(ddl.contains("\"key\" varchar NOT NULL"));
        assert!(ddl.contains("\"value\" jsonb NULL"));
        assert!(ddl.contains("CONSTRAINT additionalInfo_pkey PRIMARY KEY (\"key\")"));
    }
}
