//! Typed column drafts and provider-owned DDL preview.
use crate::{DbDriverKind, sql::SqlDialect};
use anyhow::{Result, bail};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ColumnDefinition {
    pub database: String,
    pub schema: String,
    pub table: String,
    pub relation_oid: String,
    pub attribute: i16,
    pub name: String,
    pub native_type: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub comment: Option<String>,
    pub identity: String,
    pub generated: String,
    pub generation_expression: Option<String>,
    pub collation: Option<String>,
    pub storage: String,
    pub local: bool,
    pub editable: bool,
    pub restriction: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColumnDraft {
    pub name: String,
    pub native_type: String,
    pub using_expression: String,
    pub nullable: bool,
    pub default: Option<String>,
    pub comment: Option<String>,
}
impl From<&ColumnDefinition> for ColumnDraft {
    fn from(c: &ColumnDefinition) -> Self {
        Self {
            name: c.name.clone(),
            native_type: c.native_type.clone(),
            using_expression: String::new(),
            nullable: c.nullable,
            default: c.default.clone(),
            comment: c.comment.clone(),
        }
    }
}
#[derive(Debug)]
pub enum ColumnApplyOutcome {
    Applied(Box<ColumnDefinition>),
    Rejected(String),
    ReadbackRequired(String),
}

/// Restrict fragments to balanced expressions, with SQL-standard doubled quotes.
/// Session-dependent backslash escapes and comments are intentionally unsupported.
pub fn validate_fragment(value: &str, type_declaration: bool) -> Result<()> {
    if value.trim().is_empty() || value.contains('\0') {
        bail!("SQL fragment cannot be empty or contain NUL");
    }
    let chars: Vec<_> = value.chars().collect();
    let mut quote = None;
    let mut depth = 0;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if let Some(q) = quote {
            if c == '\\' {
                bail!("Use doubled SQL quotes instead of backslash escapes");
            }
            if c == q {
                if chars.get(i + 1) == Some(&q) {
                    i += 1;
                } else {
                    quote = None;
                }
            }
        } else {
            match c {
                '"' => quote = Some(c),
                '\'' if !type_declaration => quote = Some(c),
                '(' | '[' => depth += 1,
                ')' | ']' => {
                    depth -= 1;
                    if depth < 0 {
                        bail!("Unbalanced SQL fragment");
                    }
                }
                ';' | '$' | '\\' => bail!(
                    "Statement delimiters, dollar quoting and backslashes are unsupported in fragments"
                ),
                '-' if chars.get(i + 1) == Some(&'-') => {
                    bail!("Comments are unsupported in fragments")
                }
                '/' if chars.get(i + 1) == Some(&'*') => {
                    bail!("Comments are unsupported in fragments")
                }
                ',' if type_declaration && depth == 0 => {
                    bail!("Use a single native type declaration")
                }
                _ if type_declaration
                    && !(c.is_alphanumeric()
                        || c.is_whitespace()
                        || matches!(c, '_' | '.' | ',')) =>
                {
                    bail!("Invalid character in native type declaration")
                }
                _ => {}
            }
        }
        i += 1;
    }
    if quote.is_some() || depth != 0 {
        bail!("Unterminated quote or unbalanced SQL fragment");
    }
    Ok(())
}

pub fn column_change_sql(original: &ColumnDefinition, draft: &ColumnDraft) -> Result<Vec<String>> {
    if !original.editable {
        bail!("{}", original.restriction);
    }
    if draft.name.trim().is_empty() || draft.name.contains('\0') || draft.name.len() > 63 {
        bail!("Column name must contain 1–63 UTF-8 bytes");
    }
    let d = SqlDialect(DbDriverKind::Postgres);
    let table = d.qualified_table(&original.schema, &original.table);
    let name = d.quote_ident(&original.name);
    let prefix = format!("ALTER TABLE ONLY {table} ALTER COLUMN {name}");
    let type_changed = original.native_type != draft.native_type;
    let mut sql = Vec::new();
    if type_changed {
        validate_fragment(&draft.native_type, true)?;
        if !draft.using_expression.trim().is_empty() {
            validate_fragment(&draft.using_expression, false)?;
        }
        if original.default.is_some() {
            sql.push(format!("{prefix} DROP DEFAULT;"));
        }
        sql.push(format!(
            "{prefix} TYPE {}{};",
            draft.native_type,
            if draft.using_expression.trim().is_empty() {
                String::new()
            } else {
                format!(" USING ({})", draft.using_expression)
            }
        ));
    } else if !draft.using_expression.trim().is_empty() {
        bail!("USING is only applicable when changing the type");
    }
    if original.default != draft.default || (type_changed && draft.default.is_some()) {
        sql.push(match &draft.default {
            Some(value) => {
                // Unchanged catalog defaults may contain syntax outside the draft subset.
                if original.default.as_ref() != Some(value) {
                    validate_fragment(value, false)?;
                }
                format!("{prefix} SET DEFAULT ({value});")
            }
            None => format!("{prefix} DROP DEFAULT;"),
        });
    }
    if original.nullable != draft.nullable {
        sql.push(format!(
            "{prefix} {} NOT NULL;",
            if draft.nullable { "DROP" } else { "SET" }
        ));
    }
    if original.comment != draft.comment {
        if draft.comment.as_ref().is_some_and(|c| c.contains('\0')) {
            bail!("Comments cannot contain NUL");
        }
        // E literals preserve Unicode/newlines and backslashes under any server setting.
        let comment = draft
            .comment
            .as_ref()
            .map(|c| format!("E'{}'", c.replace('\\', "\\\\").replace('\'', "''")))
            .unwrap_or_else(|| "NULL".into());
        sql.push(format!("COMMENT ON COLUMN {table}.{name} IS {comment};"));
    }
    if original.name != draft.name {
        sql.push(format!(
            "ALTER TABLE ONLY {table} RENAME COLUMN {name} TO {};",
            d.quote_ident(&draft.name)
        ));
    }
    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_quotes_identifiers_and_distinguishes_null_default_from_removal() {
        let original = ColumnDefinition {
            schema: "a\"schema".into(),
            table: "table".into(),
            name: "col".into(),
            native_type: "text".into(),
            nullable: true,
            editable: true,
            ..Default::default()
        };
        let mut draft = ColumnDraft::from(&original);
        draft.name = "renamed\"column".into();
        draft.default = Some("NULL".into());
        draft.comment = Some("line\\path 'x'\nTiếng Việt".into());
        let sql = column_change_sql(&original, &draft).unwrap();
        assert!(sql[0].contains("SET DEFAULT (NULL)"));
        assert!(sql[1].contains("E'line\\\\path ''x''"));
        assert!(sql.last().unwrap().contains("TO \"renamed\"\"column\""));
        assert!(sql[0].contains("\"a\"\"schema\".\"table\""));
        let with_default = ColumnDefinition {
            default: Some("NULL".into()),
            ..original
        };
        let mut removed = ColumnDraft::from(&with_default);
        removed.default = None;
        assert!(column_change_sql(&with_default, &removed).unwrap()[0].contains("DROP DEFAULT"));
    }
    #[test]
    fn fragments_reject_extra_statements_and_actions() {
        for v in [
            "text, DROP COLUMN other",
            "int; DROP TABLE t",
            "text -- comment",
            "text/*c*/",
            "int)",
        ] {
            assert!(validate_fragment(v, true).is_err(), "{v}");
        }
        for v in [
            "numeric(10,2)",
            "timestamp without time zone",
            "\"custom schema\".\"enum type\"[]",
        ] {
            assert!(validate_fragment(v, true).is_ok(), "{v}");
        }
        assert!(validate_fragment("'a;b'::text", false).is_ok());
        assert!(validate_fragment("NULL", false).is_ok());
        assert!(validate_fragment("1); DROP TABLE t; (1", false).is_err());
    }
}
