//! Real-time syntax diagnostics and error detection for SQL scripts.
//!
//! Checks for:
//! - Unterminated string literals (`'...'`)
//! - Unterminated block comments (`/* ... */`)
//! - Unterminated dollar quotes (`$$...$$`)
//! - Unbalanced parentheses and brackets
//! - Tree-sitter syntax parse error and missing token nodes

use tree_sitter::{Language, Parser};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlDiagnostic {
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub is_error: bool,
}

pub fn diagnose_sql(sql: &str) -> Vec<SqlDiagnostic> {
    let mut diagnostics = Vec::new();

    if sql.trim().is_empty() {
        return diagnostics;
    }

    // 1. Lexical checks for unterminated tokens and bracket balance
    check_lexical_structure(sql, &mut diagnostics);

    // 2. Tree-sitter AST error node check (if no fatal lexical error already found)
    if diagnostics.is_empty() {
        check_ast_errors(sql, &mut diagnostics);
    }

    diagnostics
}

fn byte_to_line_col(sql: &str, byte_offset: usize) -> (usize, usize) {
    let clamped = byte_offset.min(sql.len());
    let prefix = &sql[..clamped];
    let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
    let last_newline = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let column = clamped.saturating_sub(last_newline) + 1;
    (line, column)
}

fn check_lexical_structure(sql: &str, diagnostics: &mut Vec<SqlDiagnostic>) {
    let b = sql.as_bytes();
    let len = b.len();
    let mut i = 0;

    let mut paren_stack: Vec<(char, usize)> = Vec::new();

    while i < len {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comment
        if i + 1 < len && b[i] == b'-' && b[i + 1] == b'-' {
            i += 2;
            while i < len && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }

        // Block comment
        if i + 1 < len && b[i] == b'/' && b[i + 1] == b'*' {
            let start = i;
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
            if depth > 0 {
                let (line, col) = byte_to_line_col(sql, start);
                diagnostics.push(SqlDiagnostic {
                    line,
                    column: col,
                    message: "Unterminated block comment (/* ... */)".into(),
                    is_error: true,
                });
                return;
            }
            continue;
        }

        // Single-quoted string
        if b[i] == b'\'' {
            let start = i;
            i += 1;
            let mut terminated = false;
            while i < len {
                if b[i] == b'\'' {
                    i += 1;
                    if i < len && b[i] == b'\'' {
                        i += 1;
                    } else {
                        terminated = true;
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            if !terminated {
                let (line, col) = byte_to_line_col(sql, start);
                diagnostics.push(SqlDiagnostic {
                    line,
                    column: col,
                    message: "Unterminated string literal".into(),
                    is_error: true,
                });
                return;
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

        // Dollar-quoted string ($$...$$ or $tag$...$tag$)
        if b[i] == b'$'
            && i + 1 < len
            && let Some(tag_len) = scan_dollar_quote_tag(&b[i..])
        {
            let start = i;
            let tag = &sql[i..i + tag_len];
            i += tag_len;
            if let Some(pos) = sql[i..].find(tag) {
                i += pos + tag_len;
            } else {
                let (line, col) = byte_to_line_col(sql, start);
                diagnostics.push(SqlDiagnostic {
                    line,
                    column: col,
                    message: format!("Unterminated dollar-quoted string ({tag})"),
                    is_error: true,
                });
                return;
            }
            continue;
        }

        // Parentheses and brackets
        if b[i] == b'(' || b[i] == b'[' {
            paren_stack.push((b[i] as char, i));
        } else if b[i] == b')' {
            match paren_stack.pop() {
                Some(('(', _)) => {}
                Some((other, start)) => {
                    let (line, col) = byte_to_line_col(sql, i);
                    diagnostics.push(SqlDiagnostic {
                        line,
                        column: col,
                        message: format!("Mismatched bracket: expected matching for '{other}' (opened at offset {start}), found ')'"),
                        is_error: true,
                    });
                    return;
                }
                None => {
                    let (line, col) = byte_to_line_col(sql, i);
                    diagnostics.push(SqlDiagnostic {
                        line,
                        column: col,
                        message: "Unexpected closing parenthesis ')' without matching '('".into(),
                        is_error: true,
                    });
                    return;
                }
            }
        } else if b[i] == b']' {
            match paren_stack.pop() {
                Some(('[', _)) => {}
                Some((other, start)) => {
                    let (line, col) = byte_to_line_col(sql, i);
                    diagnostics.push(SqlDiagnostic {
                        line,
                        column: col,
                        message: format!("Mismatched bracket: expected matching for '{other}' (opened at offset {start}), found ']'"),
                        is_error: true,
                    });
                    return;
                }
                None => {
                    let (line, col) = byte_to_line_col(sql, i);
                    diagnostics.push(SqlDiagnostic {
                        line,
                        column: col,
                        message: "Unexpected closing bracket ']' without matching '['".into(),
                        is_error: true,
                    });
                    return;
                }
            }
        }

        i += 1;
    }

    if let Some((unclosed, start)) = paren_stack.pop() {
        let (line, col) = byte_to_line_col(sql, start);
        diagnostics.push(SqlDiagnostic {
            line,
            column: col,
            message: format!("Unclosed bracket '{unclosed}'"),
            is_error: true,
        });
    }
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

fn check_ast_errors(sql: &str, diagnostics: &mut Vec<SqlDiagnostic>) {
    let mut parser = Parser::new();
    let lang: Language = tree_sitter_sequel::LANGUAGE.into();
    if parser.set_language(&lang).is_err() {
        return;
    }

    // Normalize MySQL backticks to ANSI double quotes for tree-sitter compatibility
    // (exact 1:1 ASCII byte mapping preserves all character and column offsets)
    let normalized = if sql.contains('`') {
        sql.replace('`', "\"")
    } else {
        sql.to_string()
    };

    let Some(tree) = parser.parse(&normalized, None) else {
        return;
    };

    let root = tree.root_node();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.is_error() || node.is_missing() {
            let start_pos = node.start_position();
            let line = start_pos.row + 1;
            let col = start_pos.column + 1;
            let snippet = &sql[node.start_byte()..node.end_byte().min(sql.len())];
            let msg = if node.is_missing() {
                format!("Missing expected SQL syntax near line {line}, col {col}")
            } else if snippet.trim().is_empty() {
                format!("Syntax error at line {line}, col {col}")
            } else {
                format!("Syntax error near '{snippet}' at line {line}, col {col}")
            };

            diagnostics.push(SqlDiagnostic {
                line,
                column: col,
                message: msg,
                is_error: true,
            });
            break; // Surface the first syntax error
        }

        let mut child_cursor = node.walk();
        let children: Vec<_> = node.children(&mut child_cursor).collect();
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_sql_produces_no_diagnostics() {
        let sql = "SELECT id, name FROM users WHERE active = TRUE AND age > 21 ORDER BY id ASC;";
        let diags = diagnose_sql(sql);
        assert!(diags.is_empty(), "Expected no diagnostics for valid SQL");
    }

    #[test]
    fn quoted_identifiers_with_parentheses_do_not_produce_false_positives() {
        let sql = r#"SELECT "total (usd)", `sub (count)` FROM my_table WHERE id = 1;"#;
        let diags = diagnose_sql(sql);
        assert!(
            diags.is_empty(),
            "Quoted identifiers with parentheses should not trigger bracket errors"
        );
    }

    #[test]
    fn unclosed_string_detected() {
        let sql = "SELECT * FROM users WHERE name = 'John Doe";
        let diags = diagnose_sql(sql);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Unterminated string literal"));
    }

    #[test]
    fn unclosed_comment_detected() {
        let sql = "SELECT 1;\n/* unclosed comment";
        let diags = diagnose_sql(sql);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Unterminated block comment"));
        assert_eq!(diags[0].line, 2);
    }

    #[test]
    fn unclosed_parenthesis_detected() {
        let sql = "SELECT (1 + 2 FROM users;";
        let diags = diagnose_sql(sql);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Unclosed bracket '('"));
    }

    #[test]
    fn unmatched_closing_parenthesis_detected() {
        let sql = "SELECT 1 + 2) FROM users;";
        let diags = diagnose_sql(sql);
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("Unexpected closing parenthesis"));
    }
}
