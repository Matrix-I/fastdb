use tree_sitter::{Language, Parser};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Keyword,
    StringLiteral,
    NumberLiteral,
    Identifier,
    Comment,
    Operator,
    Punctuation,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub kind: TokenKind,
}

pub struct SqlParser {
    parser: Parser,
}

impl SqlParser {
    pub fn new() -> Option<Self> {
        let mut parser = Parser::new();
        let lang: Language = tree_sitter_sequel::LANGUAGE.into();
        parser.set_language(&lang).ok()?;
        Some(Self { parser })
    }

    /// Finds the SQL statement at cursor byte offset (start_byte, end_byte, statement_sql).
    /// Allows executing only the current statement under the cursor (Cmd+Enter).
    pub fn statement_at_offset<'a>(
        &mut self,
        source: &'a str,
        offset: usize,
    ) -> Option<(usize, usize, &'a str)> {
        let trimmed_source = source.trim();
        if trimmed_source.is_empty() {
            return None;
        }

        let clamped_offset = offset.min(source.len());
        let mut statements: Vec<(usize, usize, &'a str)> = Vec::new();

        if let Some(tree) = self.parser.parse(source, None) {
            let root = tree.root_node();
            let mut cursor = root.walk();

            for child in root.children(&mut cursor) {
                let text = &source[child.start_byte()..child.end_byte()];
                let trimmed = text.trim();

                if trimmed == ";" {
                    // Attach semicolon to the previous statement if present
                    if let Some(last) = statements.last_mut() {
                        last.1 = child.end_byte();
                        last.2 = &source[last.0..last.1];
                    }
                    continue;
                }

                if trimmed.starts_with("--") || trimmed.is_empty() {
                    // Skip top-level comments and blank spaces
                    continue;
                }

                statements.push((child.start_byte(), child.end_byte(), text));
            }
        }

        // Fallback: if tree-sitter couldn't split any statements, fallback to semicolon splitting
        if statements.is_empty() {
            let mut cur_offset = 0;
            for part in source.split(';') {
                let part_len = part.len();
                let trimmed = part.trim();
                let end = cur_offset + part_len;
                if !trimmed.is_empty() && !trimmed.starts_with("--") {
                    statements.push((cur_offset, end, part));
                }
                cur_offset = end + 1;
            }
        }

        if statements.is_empty() {
            return Some((0, source.len(), source.trim()));
        }

        // If only 1 statement exists, return it unconditionally
        if statements.len() == 1 {
            let (s, e, text) = statements[0];
            return Some((s, e, text.trim()));
        }

        // If multiple statements exist:
        // 1. Direct hit: offset falls within [start, end]
        for (start, end, text) in &statements {
            if clamped_offset >= *start && clamped_offset <= *end {
                return Some((*start, *end, text.trim()));
            }
        }

        // 2. Cursor is in whitespace after a statement: pick the preceding statement
        let mut best = None;
        for (start, end, text) in &statements {
            if clamped_offset >= *start {
                best = Some((*start, *end, text.trim()));
            }
        }

        if let Some(stmt) = best {
            return Some(stmt);
        }

        // 3. Cursor is before the first statement: pick the first statement
        let (s, e, text) = statements[0];
        Some((s, e, text.trim()))
    }

    /// Tokenizes the SQL code into highlighted spans
    pub fn highlight(&mut self, source: &str) -> Vec<HighlightSpan> {
        let mut spans = Vec::new();
        let tree = match self.parser.parse(source, None) {
            Some(t) => t,
            None => return spans,
        };

        let mut cursor = tree.walk();
        let mut reached_root = false;

        while !reached_root {
            let node = cursor.node();
            let kind = node.kind();
            let start = node.start_byte();
            let end = node.end_byte();

            let token_kind = match kind {
                "keyword" | "select" | "from" | "where" | "insert" | "update" | "delete"
                | "create" | "table" | "join" | "left" | "right" | "inner" | "outer" | "on"
                | "group" | "by" | "order" | "limit" | "having" | "as" | "distinct" | "into"
                | "values" | "set" | "and" | "or" | "not" | "null" | "is" | "in" => {
                    Some(TokenKind::Keyword)
                }
                "string_literal" | "literal_string" => Some(TokenKind::StringLiteral),
                "numeric_literal" | "number" => Some(TokenKind::NumberLiteral),
                "comment" | "line_comment" | "block_comment" => Some(TokenKind::Comment),
                "identifier" | "object_reference" => Some(TokenKind::Identifier),
                "+" | "-" | "*" | "/" | "=" | "<" | ">" | "<=" | ">=" | "!=" | "<>" => {
                    Some(TokenKind::Operator)
                }
                ";" | "," | "(" | ")" | "." => Some(TokenKind::Punctuation),
                _ => None,
            };

            if let Some(tk) = token_kind {
                if start < end {
                    spans.push(HighlightSpan {
                        start_byte: start,
                        end_byte: end,
                        kind: tk,
                    });
                }
            }

            if cursor.goto_first_child() {
                continue;
            }
            if cursor.goto_next_sibling() {
                continue;
            }

            let mut retracing = true;
            while retracing {
                if !cursor.goto_parent() {
                    reached_root = true;
                    retracing = false;
                } else if cursor.goto_next_sibling() {
                    retracing = false;
                }
            }
        }

        spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sql_parser_statement_extraction() {
        let mut parser = SqlParser::new().unwrap();
        let sql = "SELECT * FROM users;\nSELECT 42;";

        let stmt1 = parser.statement_at_offset(sql, 5);
        assert!(stmt1.is_some());
        let (_s, _e, text) = stmt1.unwrap();
        assert!(text.contains("SELECT * FROM users"));

        let stmt2 = parser.statement_at_offset(sql, 25);
        assert!(stmt2.is_some());
        let (_, _, text2) = stmt2.unwrap();
        assert!(text2.contains("SELECT 42"));

        // Single statement with cursor at the end after semicolon
        let single = "SELECT * FROM \"public\".\"additionalInfo\" LIMIT 100;";
        let stmt3 = parser.statement_at_offset(single, single.len());
        assert!(stmt3.is_some());
        let (_, _, text3) = stmt3.unwrap();
        assert_eq!(text3, single);

        // Single statement with cursor at 0
        let stmt4 = parser.statement_at_offset(single, 0);
        assert!(stmt4.is_some());
        assert_eq!(stmt4.unwrap().2, single);
    }

    #[test]
    fn test_sql_parser_highlighting() {
        let mut parser = SqlParser::new().unwrap();
        let sql = "SELECT id, 'hello' FROM accounts WHERE id = 10;";
        let spans = parser.highlight(sql);
        assert!(!spans.is_empty());
    }
}
