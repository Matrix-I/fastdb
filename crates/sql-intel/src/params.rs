//! Parameter detection, validation, and safe SQL substitution.
//!
//! Handles:
//! - Named parameters: `:param_name`, `@param_name` (excluding PostgreSQL type casts `::type`)
//! - Positional parameters: `?`, `$1`, `$2`
//! - Safe parameter typing (String, Integer, Float, Boolean, Date, Null, Raw)
//! - SQL injection-proof substitution with type validation

use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamKind {
    Named,
    PositionalDollar,
    PositionalQuestion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlParameter {
    pub name: String,
    pub kind: ParamKind,
    pub start_byte: usize,
    pub end_byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    String,
    Integer,
    Float,
    Boolean,
    Date,
    Null,
    Raw,
}

impl ParamType {
    pub fn all() -> &'static [ParamType] {
        &[
            ParamType::String,
            ParamType::Integer,
            ParamType::Float,
            ParamType::Boolean,
            ParamType::Date,
            ParamType::Null,
            ParamType::Raw,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            ParamType::String => "String",
            ParamType::Integer => "Integer",
            ParamType::Float => "Float",
            ParamType::Boolean => "Boolean",
            ParamType::Date => "Date/Time",
            ParamType::Null => "NULL",
            ParamType::Raw => "Raw SQL",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamBinding {
    pub name: String,
    pub param_type: ParamType,
    pub value: String,
}

/// Scans SQL text and returns all parameter occurrences in order of appearance.
/// String literals (`'...'`, `$$...$$`) and comments (`--...`, `/*...*/`) are ignored.
/// PostgreSQL type casts (`::type`) are not treated as parameters.
pub fn extract_parameters(sql: &str) -> Vec<SqlParameter> {
    let mut params = Vec::new();
    let b = sql.as_bytes();
    let len = b.len();
    let mut i = 0;

    let mut question_index = 1;

    while i < len {
        // Skip whitespace
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comment: `--`
        if i + 1 < len && b[i] == b'-' && b[i + 1] == b'-' {
            i += 2;
            while i < len && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Block comment: `/* ... */`
        if i + 1 < len && b[i] == b'/' && b[i + 1] == b'*' {
            i += 2;
            let mut depth = 1;
            while i < len && depth > 0 {
                if i + 1 < len && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < len && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }

        // Quoted string literal: `'...'`
        if b[i] == b'\'' {
            i += 1;
            while i < len {
                if b[i] == b'\'' {
                    i += 1;
                    if i < len && b[i] == b'\'' {
                        i += 1; // Escaped quote ''
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            continue;
        }

        // Quoted identifier: `"..."` or `` `...` ``
        if b[i] == b'"' || b[i] == b'`' {
            let q = b[i];
            i += 1;
            while i < len {
                if b[i] == q {
                    i += 1;
                    if i < len && b[i] == q {
                        i += 1;
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            continue;
        }

        // Dollar-quoted string: `$$...$$` or `$tag$...$tag$`
        if b[i] == b'$'
            && i + 1 < len
            && let Some(tag_len) = scan_dollar_quote_tag(&b[i..])
        {
            let tag = &sql[i..i + tag_len];
            i += tag_len;
            if let Some(pos) = sql[i..].find(tag) {
                i += pos + tag_len;
            } else {
                i = len;
            }
            continue;
        }

        // PostgreSQL type cast: `::`
        if i + 1 < len && b[i] == b':' && b[i + 1] == b':' {
            i += 2;
            // Skip the type identifier
            while i < len && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            continue;
        }

        // Named parameter: `:name` or `@name`
        if (b[i] == b':' || b[i] == b'@') && i + 1 < len {
            let prev_byte = if i > 0 { Some(b[i - 1]) } else { None };
            let prev_is_boundary = prev_byte
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != b'_' && c != b':' && c != b'@');

            if prev_is_boundary {
                let start = i;
                let next = b[i + 1];
                // Must begin with an ASCII letter or underscore, not another @ or :
                if next.is_ascii_alphabetic() || next == b'_' {
                    i += 2;
                    while i < len && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                        i += 1;
                    }
                    let name = sql[start..i].to_string();
                    params.push(SqlParameter {
                        name,
                        kind: ParamKind::Named,
                        start_byte: start,
                        end_byte: i,
                    });
                    continue;
                }
            }
        }

        // Positional parameter: `$1`, `$2`
        if b[i] == b'$' && i + 1 < len && b[i + 1].is_ascii_digit() {
            let start = i;
            i += 1;
            while i < len && b[i].is_ascii_digit() {
                i += 1;
            }
            let name = sql[start..i].to_string();
            params.push(SqlParameter {
                name,
                kind: ParamKind::PositionalDollar,
                start_byte: start,
                end_byte: i,
            });
            continue;
        }

        // Positional parameter: `?`
        if b[i] == b'?' {
            let start = i;
            let name = format!("?{question_index}");
            question_index += 1;
            i += 1;
            params.push(SqlParameter {
                name,
                kind: ParamKind::PositionalQuestion,
                start_byte: start,
                end_byte: i,
            });
            continue;
        }

        i += 1;
    }

    params
}

fn scan_dollar_quote_tag(b: &[u8]) -> Option<usize> {
    if b.is_empty() || b[0] != b'$' {
        return None;
    }
    let mut i = 1;
    while i < b.len() {
        if b[i] == b'$' {
            return Some(i + 1);
        }
        if !b[i].is_ascii_alphanumeric() && b[i] != b'_' {
            return None;
        }
        i += 1;
    }
    None
}

/// Returns unique parameter names in their first-appearance order.
pub fn distinct_parameter_names(params: &[SqlParameter]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for p in params {
        if seen.insert(p.name.clone()) {
            names.push(p.name.clone());
        }
    }
    names
}

/// Formats a single parameter value safely according to its requested type.
pub fn format_param_value(pt: ParamType, raw_val: &str) -> Result<String, String> {
    match pt {
        ParamType::Null => Ok("NULL".to_string()),
        ParamType::String => {
            let escaped = raw_val.replace('\\', "\\\\").replace('\'', "''");
            Ok(format!("'{escaped}'"))
        }
        ParamType::Integer => {
            let trimmed = raw_val.trim();
            if trimmed.is_empty() {
                return Err("Integer value cannot be empty (use NULL for missing values)".into());
            }
            match trimmed.parse::<i64>() {
                Ok(n) => Ok(n.to_string()),
                Err(_) => Err(format!("Invalid integer format: '{trimmed}'")),
            }
        }
        ParamType::Float => {
            let trimmed = raw_val.trim();
            if trimmed.is_empty() {
                return Err("Float value cannot be empty (use NULL for missing values)".into());
            }
            match trimmed.parse::<f64>() {
                Ok(f) if f.is_finite() => Ok(f.to_string()),
                _ => Err(format!("Invalid finite float format: '{trimmed}'")),
            }
        }
        ParamType::Boolean => {
            let trimmed = raw_val.trim().to_lowercase();
            match trimmed.as_str() {
                "true" | "t" | "1" | "yes" => Ok("TRUE".to_string()),
                "false" | "f" | "0" | "no" => Ok("FALSE".to_string()),
                _ => Err(format!(
                    "Invalid boolean value: '{raw_val}' (expected true/false, 1/0, yes/no)"
                )),
            }
        }
        ParamType::Date => {
            let trimmed = raw_val.trim();
            if trimmed.is_empty() {
                return Err("Date value cannot be empty (use NULL for missing values)".into());
            }
            // Basic SQL date/timestamp safety check: disallow semicolons, comments, or quotes
            if trimmed.contains([';', '\'', '"']) {
                return Err("Date value cannot contain quotes or semicolons".into());
            }
            let escaped = trimmed.replace('\'', "''");
            Ok(format!("'{escaped}'"))
        }
        ParamType::Raw => Ok(raw_val.to_string()),
    }
}

/// Safely substitutes parameters in SQL with typed and formatted values.
pub fn substitute_parameters(sql: &str, bindings: &[ParamBinding]) -> Result<String, String> {
    let params = extract_parameters(sql);
    if params.is_empty() {
        return Ok(sql.to_string());
    }

    // Map bindings by name
    use std::collections::HashMap;
    let mut map = HashMap::new();
    for b in bindings {
        let formatted = format_param_value(b.param_type, &b.value)?;
        map.insert(b.name.as_str(), formatted);
    }

    // Check that all parameters have bindings
    for p in &params {
        if !map.contains_key(p.name.as_str()) {
            return Err(format!("Missing binding for parameter '{}'", p.name));
        }
    }

    // Replace in reverse order of start_byte to preserve character offsets
    let mut result = sql.to_string();
    let mut sorted_params = params;
    sorted_params.sort_by_key(|p| std::cmp::Reverse(p.start_byte));

    for p in sorted_params {
        let replacement = map.get(p.name.as_str()).unwrap();
        result.replace_range(p.start_byte..p.end_byte, replacement);
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_named_and_positional_parameters() {
        let sql = "SELECT * FROM users WHERE id = :user_id AND age > @min_age AND active = $1 AND role = ?;";
        let params = extract_parameters(sql);
        assert_eq!(params.len(), 4);
        assert_eq!(params[0].name, ":user_id");
        assert_eq!(params[0].kind, ParamKind::Named);
        assert_eq!(params[1].name, "@min_age");
        assert_eq!(params[1].kind, ParamKind::Named);
        assert_eq!(params[2].name, "$1");
        assert_eq!(params[2].kind, ParamKind::PositionalDollar);
        assert_eq!(params[3].name, "?1");
        assert_eq!(params[3].kind, ParamKind::PositionalQuestion);
    }

    #[test]
    fn skips_postgres_type_casts_and_strings_and_comments() {
        let sql = r#"
            -- Here is a comment with :ignored_param
            /* Block comment with $1 and ? */
            SELECT 'string with :fake_param and ?' AS str,
                   val::text AS casted_val,
                   $$dollar quote with :another_fake$$ AS dq,
                   @@version AS mysql_var,
                   'email@domain.com' AS email
            FROM my_table
            WHERE real_id = :real_id AND date_val > ?;
        "#;
        let params = extract_parameters(sql);
        assert_eq!(params.len(), 2);
        assert_eq!(params[0].name, ":real_id");
        assert_eq!(params[1].name, "?1");
    }

    #[test]
    fn escapes_backslashes_and_single_quotes_in_string_params() {
        assert_eq!(
            format_param_value(ParamType::String, r"C:\path\to\file O'Connor").unwrap(),
            r"'C:\\path\\to\\file O''Connor'"
        );
    }

    #[test]
    fn distinct_parameters_deduplicates_while_preserving_order() {
        let sql = "SELECT * FROM t WHERE a = :val AND b = :val AND c = :other;";
        let params = extract_parameters(sql);
        let names = distinct_parameter_names(&params);
        assert_eq!(names, vec![":val".to_string(), ":other".to_string()]);
    }

    #[test]
    fn formats_param_values_safely() {
        assert_eq!(
            format_param_value(ParamType::String, "O'Reilly").unwrap(),
            "'O''Reilly'"
        );
        assert_eq!(
            format_param_value(ParamType::Integer, " 42 ").unwrap(),
            "42"
        );
        assert!(format_param_value(ParamType::Integer, "not_a_num").is_err());
        assert_eq!(
            format_param_value(ParamType::Float, " 3.1415 ").unwrap(),
            "3.1415"
        );
        assert_eq!(
            format_param_value(ParamType::Boolean, "yes").unwrap(),
            "TRUE"
        );
        assert_eq!(
            format_param_value(ParamType::Boolean, "0").unwrap(),
            "FALSE"
        );
        assert_eq!(
            format_param_value(ParamType::Null, "anything").unwrap(),
            "NULL"
        );
        assert_eq!(
            format_param_value(ParamType::Date, "2026-11-03 16:00:00").unwrap(),
            "'2026-11-03 16:00:00'"
        );
        assert!(format_param_value(ParamType::Date, "2026'; DROP TABLE users;--").is_err());
    }

    #[test]
    fn safe_substitution_replaces_all_occurrences() {
        let sql = "SELECT * FROM users WHERE name = :name AND age >= :age AND status = :name;";
        let bindings = vec![
            ParamBinding {
                name: ":name".into(),
                param_type: ParamType::String,
                value: "Alice O'Connor".into(),
            },
            ParamBinding {
                name: ":age".into(),
                param_type: ParamType::Integer,
                value: "25".into(),
            },
        ];
        let substituted = substitute_parameters(sql, &bindings).unwrap();
        assert_eq!(
            substituted,
            "SELECT * FROM users WHERE name = 'Alice O''Connor' AND age >= 25 AND status = 'Alice O''Connor';"
        );
    }

    #[test]
    fn substitution_fails_when_parameter_unbound() {
        let sql = "SELECT * FROM users WHERE id = :id AND code = :code;";
        let bindings = vec![ParamBinding {
            name: ":id".into(),
            param_type: ParamType::Integer,
            value: "10".into(),
        }];
        assert!(substitute_parameters(sql, &bindings).is_err());
    }
}
