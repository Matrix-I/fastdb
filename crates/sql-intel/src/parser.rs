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
        let statements = crate::script::execute_statement_ranges(source);
        let offset = offset.min(source.len());
        let &(start, end) = statements
            .iter()
            .find(|&&(start, end)| offset >= start && offset < end)
            .or_else(|| statements.iter().rev().find(|&&(start, _)| start <= offset))
            .or_else(|| statements.first())?;
        Some((start, end, source[start..end].trim()))
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
                _ if kind.starts_with("keyword_") => Some(TokenKind::Keyword),
                "marginalia" => Some(TokenKind::Comment),
                "literal" => {
                    let text = &source[start..end];
                    if matches!(
                        text.to_ascii_uppercase().as_str(),
                        "TRUE" | "FALSE" | "NULL"
                    ) {
                        Some(TokenKind::Keyword)
                    } else if text
                        .starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | '.'))
                    {
                        Some(TokenKind::NumberLiteral)
                    } else {
                        Some(TokenKind::StringLiteral)
                    }
                }

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

            if let Some(tk) = token_kind
                && start < end
            {
                spans.push(HighlightSpan {
                    start_byte: start,
                    end_byte: end,
                    kind: tk,
                });
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
    fn execute_keeps_postgres_strings_and_splits_independent_selects() {
        let mut parser = SqlParser::new().unwrap();
        let sql = "INSERT INTO people(note) VALUES (E'O''Reilly\\nDòng hai');\nSELECT 2";
        assert_eq!(
            parser.statement_at_offset(sql, 40).unwrap().2,
            sql.split(";\n").next().unwrap().to_owned() + ";"
        );
        let sql = "SELECT 1\nSELECT 2";
        assert_eq!(parser.statement_at_offset(sql, 2).unwrap().2, "SELECT 1");
        assert_eq!(
            parser.statement_at_offset(sql, sql.len()).unwrap().2,
            "SELECT 2"
        );
    }

    #[test]
    fn execute_preserves_nested_queries_ctes_and_union() {
        let mut parser = SqlParser::new().unwrap();
        for statement in [
            "WITH x AS (SELECT 1) SELECT * FROM x",
            "SELECT * FROM (SELECT 1) x UNION ALL SELECT 2",
            "INSERT INTO t SELECT * FROM s",
            "CREATE TABLE t AS SELECT 1",
            "ALTER TABLE child ADD CONSTRAINT fk FOREIGN KEY (parent_id) REFERENCES parent(id) ON DELETE CASCADE ON UPDATE CASCADE;",
            "CREATE PROCEDURE p() BEGIN IF 1 THEN SELECT 1; END IF; DELETE FROM t; END;",
            "BEGIN SELECT 1; SELECT 2; END;",
            "GRANT SELECT, INSERT, UPDATE, DELETE ON t TO app;",
            "EXPLAIN WITH x AS (SELECT 1) SELECT * FROM x;",
            "CREATE VIEW v AS WITH x AS (SELECT 1) SELECT * FROM x;",
            "SELECT now()::timestamp(3) WITH TIME ZONE;",
            "CREATE TRIGGER tr AFTER INSERT ON source BEGIN INSERT INTO audit VALUES (1); END;",
            "CREATE TABLE t(id int)",
            "CREATE TEMP TABLE t(id int) ON COMMIT DROP;",
            "EXPLAIN UPDATE t SET x=1;",
            "SELECT 1::timestamp WITH TIME ZONE;",
            "WITH x AS (SELECT 1) INSERT INTO t SELECT * FROM x",
            "SELECT * FROM t FOR NO KEY UPDATE",
            "INSERT INTO t VALUES (1) ON CONFLICT DO UPDATE SET id = 2",
            "SELECT ';SELECT' /* ; SELECT */ FROM t -- SELECT\nWHERE id = 1",
        ] {
            let sql = format!("{statement}\nSELECT 42");
            for (offset, _) in statement.char_indices() {
                assert_eq!(
                    parser.statement_at_offset(&sql, offset).unwrap().2,
                    statement
                );
            }
            assert_eq!(
                parser.statement_at_offset(&sql, sql.len()).unwrap().2,
                "SELECT 42"
            );
        }
    }

    #[test]
    fn execute_never_includes_other_statements_when_later_sql_is_incomplete() {
        let mut parser = SqlParser::new().unwrap();
        for suffix in ["SELECT 'unfinished", "SELECT $$unfinished", "/* unfinished"] {
            let sql = format!("DELETE FROM t; SELECT 1; {suffix}");
            assert_eq!(parser.statement_at_offset(&sql, 18).unwrap().2, "SELECT 1;");
        }
        let sql = "SELECT 1\nCREATE TABLE t(id int)";
        assert_eq!(parser.statement_at_offset(sql, 2).unwrap().2, "SELECT 1");
        assert_eq!(
            parser.statement_at_offset(sql, sql.len()).unwrap().2,
            "CREATE TABLE t(id int)"
        );
    }

    #[test]
    fn execute_inside_a_transaction_never_includes_other_commands() {
        let mut parser = SqlParser::new().unwrap();
        for begin in ["BEGIN", "BEGIN TRANSACTION", "BEGIN WORK"] {
            let sql = format!("{begin}; DELETE FROM t; SELECT 1; COMMIT;");
            let offset = sql.find("SELECT").unwrap();
            for position in offset..offset + "SELECT 1;".len() {
                assert_eq!(
                    parser.statement_at_offset(&sql, position).unwrap().2,
                    "SELECT 1;"
                );
            }
        }
    }

    #[test]
    fn execute_caret_only_applies_the_target_statement_to_sqlite() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE t(id INTEGER); INSERT INTO t VALUES (1);")
            .unwrap();
        let sql = "DELETE FROM t;\nINSERT INTO t VALUES (2)\nSELECT * FROM t";
        let mut parser = SqlParser::new().unwrap();
        let statement = parser
            .statement_at_offset(sql, sql.find("VALUES").unwrap())
            .unwrap()
            .2;
        db.execute(statement, []).unwrap();
        let ids: Vec<i64> = db
            .prepare("SELECT id FROM t ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn test_sql_parser_highlighting() {
        let mut parser = SqlParser::new().unwrap();
        let sql = "SELECT id, 'hello' FROM accounts WHERE id = 10;";
        let spans = parser.highlight(sql);
        assert!(!spans.is_empty());
    }
    #[test]
    fn highlighting_recognizes_native_grammar_keywords_literals_and_comments() {
        let source = "CREATE TABLE t (n INTEGER DEFAULT 42, s TEXT DEFAULT 'value'); /* note */";
        let mut parser = SqlParser::new().unwrap();
        let spans = parser.highlight(source);
        for (text, kind) in [
            ("CREATE", TokenKind::Keyword),
            ("42", TokenKind::NumberLiteral),
            ("'value'", TokenKind::StringLiteral),
            ("/* note */", TokenKind::Comment),
        ] {
            assert!(
                spans.iter().any(
                    |span| &source[span.start_byte..span.end_byte] == text && span.kind == kind
                ),
                "missing {text}"
            );
        }
    }
}
