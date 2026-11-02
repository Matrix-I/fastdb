//! Conservative SQL script boundaries and whitespace formatting. Literals and comments
//! are opaque tokens: semicolons inside them never terminate a statement.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Quoted,
    Comment,
    Symbol,
}
#[derive(Debug)]
struct Token<'a> {
    text: &'a str,
    start: usize,
    end: usize,
    kind: Kind,
}

fn tokens(sql: &str) -> Result<Vec<Token<'_>>, String> {
    tokens_policy(sql, false)
}
fn tokens_policy(sql: &str, escaped_e_strings: bool) -> Result<Vec<Token<'_>>, String> {
    let b = sql.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let kind;
        if b[i] == b'#' || b[i..].starts_with(b"/*!") {
            return Err("MySQL hash comments and executable comments are not supported by script splitting/formatting".into());
        }
        if b[i..].starts_with(b"--") {
            kind = Kind::Comment;
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            kind = Kind::Comment;
            let mut depth = 1;
            i += 2;
            while i < b.len() && depth > 0 {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth != 0 {
                return Err("Unterminated SQL comment".into());
            }
        } else if matches!(b[i], b'\'' | b'"' | b'`' | b'[') {
            kind = Kind::Quoted;
            let quote = if b[i] == b'[' { b']' } else { b[i] };
            let explicit_e = escaped_e_strings
                && quote == b'\''
                && out.last().is_some_and(|t: &Token<'_>| {
                    t.kind == Kind::Word && t.text.eq_ignore_ascii_case("E") && t.end == start
                });
            i += 1;
            loop {
                if i >= b.len() {
                    return Err("Unterminated SQL literal or identifier".into());
                }
                if b[i] == b'\\' && matches!(quote, b'\'' | b'`' | b'"') {
                    if explicit_e {
                        if i + 1 >= b.len() {
                            return Err("Unterminated E string escape".into());
                        }
                        i += 2;
                        continue;
                    }
                    // Backslash quoting depends on the provider/session SQL mode. Reject rather
                    // than accidentally execute a different statement boundary.
                    return Err("Script splitting/formatting does not support backslashes in string literals; execute the selected statement instead".into());
                }
                if b[i] == quote {
                    i += 1;
                    if i < b.len() && b[i] == quote {
                        i += 1;
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if b[i] == b'$' && dollar_tag(&sql[i..]).is_some() {
            kind = Kind::Quoted;
            let tag = dollar_tag(&sql[i..]).unwrap();
            i += tag.len();
            let end = sql[i..]
                .find(tag)
                .ok_or("Unterminated dollar-quoted SQL literal")?;
            i += end + tag.len();
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' || b[i] >= 128 {
            kind = Kind::Word;
            i += 1;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'$') || b[i] >= 128)
            {
                i += 1;
            }
        } else {
            kind = Kind::Symbol;
            i += 1;
            // Preserve contiguous operators, numeric literals and punctuation spacing through
            // the formatter's original-gap policy.
        }
        out.push(Token {
            text: &sql[start..i],
            start,
            end: i,
            kind,
        });
    }
    Ok(out)
}

fn dollar_tag(sql: &str) -> Option<&str> {
    let end = sql[1..].find('$')? + 1;
    let tag = &sql[1..end];
    (tag.is_empty()
        || (tag.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')))
    .then_some(&sql[..=end])
}

/// Semicolon-delimited statements; client directives and procedural blocks without
/// dollar quoting are rejected. This deliberately does not guess at vendor batch syntax.
pub fn split_script(sql: &str) -> Result<Vec<String>, String> {
    split_script_policy(sql, false)
}

/// PostgreSQL run-scoped sessions: validate all explicit transaction boundaries
/// before executing any statement. Other providers keep the conservative policy.
pub fn split_script_with_transactions(sql: &str) -> Result<Vec<String>, String> {
    let statements = split_script_policy(sql, true)?;
    let mut in_transaction = false;
    for statement in &statements {
        let scanned = tokens(statement)?;
        let significant: Vec<_> = scanned
            .iter()
            .filter(|t| t.kind != Kind::Comment && t.text != ";")
            .collect();
        let words: Vec<_> = significant
            .iter()
            .map(|t| t.text.to_ascii_uppercase())
            .collect();
        let first = words.first().map(String::as_str).unwrap_or("");
        let starts = matches!(first, "BEGIN" | "START");
        let ends = matches!(first, "COMMIT" | "END" | "ROLLBACK" | "ABORT");
        if starts || ends {
            let ordinary = significant.iter().all(|t| t.kind == Kind::Word);
            let valid_form = if first == "START" {
                words == ["START", "TRANSACTION"]
            } else {
                words.len() == 1
                    || (words.len() == 2 && matches!(words[1].as_str(), "WORK" | "TRANSACTION"))
            };
            if !ordinary || !valid_form {
                return Err("Unsupported transaction form: use BEGIN / START TRANSACTION, COMMIT or ROLLBACK (optional WORK / TRANSACTION); options, chaining, savepoints and prepared transactions are not supported".into());
            }
            if starts {
                if in_transaction {
                    return Err("Nested BEGIN is not supported in Run script".into());
                }
                in_transaction = true;
            } else {
                if !in_transaction {
                    return Err(
                        "COMMIT / ROLLBACK requires a preceding BEGIN in this script".into(),
                    );
                }
                in_transaction = false;
            }
        } else if matches!(first, "SAVEPOINT" | "RELEASE")
            || words
                .get(0..2)
                .is_some_and(|w| w == ["PREPARE", "TRANSACTION"])
        {
            return Err(
                "Savepoints and prepared transactions are not supported by Run script".into(),
            );
        } else if significant.iter().any(|t| {
            t.kind == Kind::Word
                && matches!(
                    t.text.to_ascii_uppercase().as_str(),
                    "BEGIN" | "COMMIT" | "ROLLBACK"
                )
        }) {
            return Err("Unquoted procedural blocks are not supported by Run script".into());
        }
    }
    if in_transaction {
        return Err(
            "Transaction left open: end each BEGIN with COMMIT or ROLLBACK within the script"
                .into(),
        );
    }
    Ok(statements)
}

/// Persistent PostgreSQL sessions allow transaction boundaries across executions.
/// Keep the conservative lexer and reject transaction options whose state cannot
/// be represented by the toolbar. PostgreSQL validates savepoint existence.
pub fn split_session_script(sql: &str) -> Result<Vec<String>, String> {
    let statements = split_script_policy(sql, true)?;
    for statement in &statements {
        let scanned = tokens(statement)?;
        let significant: Vec<_> = scanned
            .iter()
            .filter(|t| t.kind != Kind::Comment && t.text != ";")
            .collect();
        let words: Vec<_> = significant
            .iter()
            .map(|t| t.text.to_ascii_uppercase())
            .collect();
        let first = words.first().map(String::as_str).unwrap_or("");
        let identifier = |i: usize| {
            significant.get(i).is_some_and(|t| {
                t.kind == Kind::Word || (t.kind == Kind::Quoted && t.text.starts_with('"'))
            })
        };
        let ordinary = significant.iter().all(|t| t.kind == Kind::Word);
        let valid = match first {
            "BEGIN" | "COMMIT" | "END" | "ABORT" => {
                ordinary
                    && (words.len() == 1
                        || (words.len() == 2
                            && matches!(words[1].as_str(), "WORK" | "TRANSACTION")))
            }
            "START" => words == ["START", "TRANSACTION"],
            "ROLLBACK" => {
                (ordinary
                    && (words.len() == 1
                        || (words.len() == 2
                            && matches!(words[1].as_str(), "WORK" | "TRANSACTION"))))
                    || (words.get(1).is_some_and(|w| w == "TO")
                        && ((words.len() == 3 && identifier(2))
                            || (words.len() == 4 && words[2] == "SAVEPOINT" && identifier(3))))
            }
            "SAVEPOINT" => words.len() == 2 && identifier(1),
            "RELEASE" => {
                (words.len() == 2 && identifier(1))
                    || (words.len() == 3 && words[1] == "SAVEPOINT" && identifier(2))
            }
            "PREPARE" if words.get(1).is_some_and(|w| w == "TRANSACTION") => false,
            _ => !significant.iter().any(|t| {
                t.kind == Kind::Word
                    && matches!(
                        t.text.to_ascii_uppercase().as_str(),
                        "BEGIN" | "COMMIT" | "ROLLBACK"
                    )
            }),
        };
        if !valid {
            return Err("Unsupported transaction or procedural form in tab session".into());
        }
    }
    Ok(statements)
}

/// State changes apply only after PostgreSQL acknowledges successful execution.
pub fn session_transaction_transition(sql: &str) -> Option<&'static str> {
    let scanned = tokens(sql).ok()?;
    let words: Vec<_> = scanned
        .iter()
        .filter(|t| t.kind != Kind::Comment)
        .map(|t| t.text.to_ascii_uppercase())
        .collect();
    match words.first()?.as_str() {
        "BEGIN" | "START" => Some("In transaction"),
        "ROLLBACK" if words.get(1).is_some_and(|w| w == "TO") => Some("In transaction"),
        "COMMIT" | "END" | "ROLLBACK" | "ABORT" => Some("Idle"),
        _ => None,
    }
}

/// Clause keywords outside literals, quoted identifiers, comments and subqueries.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct FilterClauses {
    pub order_by: bool,
    pub limit: bool,
    pub offset: bool,
    pub fetch: bool,
}
pub fn filter_clauses(sql: &str) -> Option<FilterClauses> {
    let scanned = tokens_policy(sql, true).ok()?;
    let mut clauses = FilterClauses::default();
    let mut depth = 0;
    let mut previous = String::new();
    for token in scanned {
        if token.kind == Kind::Comment {
            continue;
        }
        if token.kind == Kind::Symbol && token.text == "(" {
            depth += 1;
        }
        if token.kind == Kind::Symbol && token.text == ")" {
            depth -= 1;
        }
        if depth != 0 {
            continue;
        }
        if token.kind == Kind::Word {
            let word = token.text.to_ascii_uppercase();
            match word.as_str() {
                "BY" if previous == "ORDER" => clauses.order_by = true,
                "LIMIT" => clauses.limit = true,
                "OFFSET" => clauses.offset = true,
                "FETCH" => clauses.fetch = true,
                _ => {}
            }
            previous = word;
        } else {
            previous.clear();
        }
    }
    Some(clauses)
}

fn split_script_policy(sql: &str, transactions: bool) -> Result<Vec<String>, String> {
    let tokens = tokens(sql)?;
    let mut start = 0;
    let mut has_code = false;
    let mut out = Vec::new();
    for token in &tokens {
        if token.kind == Kind::Word {
            let upper = token.text.to_ascii_uppercase();
            if matches!(
                upper.as_str(),
                "DELIMITER" | "GO" | "PROCEDURE" | "TRIGGER" | "DECLARE"
            ) || (!transactions
                && matches!(
                    upper.as_str(),
                    "BEGIN" | "COMMIT" | "ROLLBACK" | "SAVEPOINT" | "TRANSACTION"
                ))
            {
                return Err("Run script supports ordinary statements and dollar-quoted function bodies. Execute transaction control, procedural blocks and vendor batch directives as individual statements.".into());
            }
        }
        if token.text == ";" && token.kind == Kind::Symbol {
            if has_code {
                out.push(sql[start..token.end].trim().to_owned());
            }
            start = token.end;
            has_code = false;
        } else if token.kind != Kind::Comment {
            has_code = true;
        }
    }
    if has_code {
        out.push(sql[start..].trim().to_owned());
    }
    Ok(out)
}

/// Layout-only formatting: never changes token bytes or joins adjacent tokens. Only
/// existing whitespace is replaced, which also preserves operators and literal prefixes.
pub fn format_sql(sql: &str) -> Result<String, String> {
    let tokens = tokens(sql)?;
    let mut out = String::new();
    let mut end = 0;
    let mut depth: usize = 0;
    let mut previous = "";
    for (index, token) in tokens.into_iter().enumerate() {
        let gap = &sql[end..token.start];
        let clause = token.kind == Kind::Word
            && depth == 0
            && matches!(
                token.text.to_ascii_uppercase().as_str(),
                "SELECT"
                    | "FROM"
                    | "WHERE"
                    | "JOIN"
                    | "LEFT"
                    | "RIGHT"
                    | "INNER"
                    | "GROUP"
                    | "ORDER"
                    | "HAVING"
                    | "LIMIT"
                    | "RETURNING"
                    | "UNION"
                    | "VALUES"
                    | "SET"
            );
        if index == 0 {
            out.push_str(gap);
        } else if !gap.is_empty() {
            if previous.starts_with("--")
                || previous == ";"
                || (clause
                    && !matches!(
                        previous.to_ascii_uppercase().as_str(),
                        "UNION" | "ALL" | "LEFT" | "RIGHT" | "INNER"
                    ))
            {
                out.push('\n');
            } else {
                out.push(' ');
            }
        }
        out.push_str(token.text);
        if token.kind == Kind::Symbol {
            if token.text == "(" {
                depth += 1;
            }
            if token.text == ")" {
                depth = depth.saturating_sub(1);
            }
        }
        previous = token.text;
        end = token.end;
    }
    out.push_str(&sql[end..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_preserve_literals_comments_and_function_bodies() {
        let sql = "-- start\nselect 'a;''b', \"semi;colon\"; /* ; /* nested */ */ select $$a;b$$; select $body$hello; world$body$; -- last";
        let statements = split_script(sql).unwrap();
        assert_eq!(statements.len(), 3);
        assert!(statements[0].contains("'a;''b'"));
        assert!(statements[1].contains("$$a;b$$"));
        assert_eq!(split_script("-- only\n/* comment */;").unwrap().len(), 0);
        assert!(split_script("select 'unfinished").is_err());
        assert!(split_script("BEGIN; select 1; COMMIT;").is_err());
        assert!(split_script("select 'a\\'; select 2").is_err());
        assert!(split_script("create trigger t begin select 1; end;").is_err());
    }
    #[test]
    fn formatting_keeps_all_tokens_and_is_idempotent() {
        let sql = "select  a::int, 'hello  world' from t -- keep this\nwhere a>=1 and name=E'hello' order by a; select 2;";
        let formatted = format_sql(sql).unwrap();
        assert!(formatted.contains("\nfrom t -- keep this\nwhere"));
        assert!(formatted.contains("'hello  world'"));
        assert_eq!(
            tokens(sql)
                .unwrap()
                .iter()
                .map(|t| t.text)
                .collect::<Vec<_>>(),
            tokens(&formatted)
                .unwrap()
                .iter()
                .map(|t| t.text)
                .collect::<Vec<_>>()
        );
        assert_eq!(format_sql(&formatted).unwrap(), formatted);
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::{split_script, split_script_with_transactions as split};

    #[test]
    fn closed_blocks_aliases_and_session_sql_are_validated_together() {
        let sql = "SET application_name = 'BEGIN; COMMIT'; -- boundary\n BEGIN WORK; CREATE TEMP TABLE t (v text); INSERT INTO t VALUES ($q$ROLLBACK; BEGIN$q$); COMMIT TRANSACTION; START TRANSACTION; SET LOCAL statement_timeout = '1s'; ROLLBACK WORK; BEGIN TRANSACTION; SELECT 1; END; BEGIN; ABORT;";
        assert_eq!(split(sql).unwrap().len(), 13);
        assert!(split_script(sql).is_err());
        assert!(split("SELECT 'BEGIN;'; SELECT 1 AS \"ROLLBACK\";").is_ok());
    }

    #[test]
    fn all_boundaries_are_rejected_before_any_script_execution() {
        for sql in [
            "INSERT INTO t VALUES (1); BEGIN;",
            "BEGIN; BEGIN; COMMIT; COMMIT;",
            "COMMIT;",
            "ROLLBACK;",
            "START;",
            "BEGIN ISOLATION LEVEL SERIALIZABLE; COMMIT;",
            "BEGIN; COMMIT AND CHAIN;",
            "BEGIN; ROLLBACK TO point;",
            "BEGIN; SAVEPOINT p; COMMIT;",
            "RELEASE p;",
            "PREPARE TRANSACTION 'id';",
            "COMMIT PREPARED 'id';",
            "BEGIN; GO; COMMIT;",
            "BEGIN; SELECT 'unterminated",
            "BEGIN; SELECT 'a\\b'; COMMIT;",
        ] {
            assert!(split(sql).is_err(), "accepted unsupported script: {sql}");
        }
    }
}

#[cfg(test)]
mod persistent_tests {
    use super::*;
    #[test]
    fn boundaries_and_savepoints_can_span_runs() {
        for sql in [
            "BEGIN",
            "COMMIT",
            "SAVEPOINT s",
            "ROLLBACK TO s",
            "ROLLBACK TO SAVEPOINT \"quoted name\"",
            "RELEASE SAVEPOINT s",
            "BEGIN; SELECT 1",
        ] {
            assert!(split_session_script(sql).is_ok(), "{sql}");
        }
        for sql in [
            "COMMIT AND CHAIN",
            "PREPARE TRANSACTION 'x'",
            "BEGIN SELECT 1 END",
            "SAVEPOINT s extra",
            "ROLLBACK TO",
        ] {
            assert!(split_session_script(sql).is_err(), "{sql}");
        }
        assert_eq!(
            session_transaction_transition("/* comment */ ROLLBACK TO s"),
            Some("In transaction")
        );
        assert_eq!(session_transaction_transition("SELECT 'BEGIN'"), None);
        assert!(split_session_script("SELECT ';'; SELECT $$BEGIN; END$$").is_ok());
    }
}

#[cfg(test)]
mod filter_clause_tests {
    use super::filter_clauses;
    #[test]
    fn quoted_and_nested_keywords_do_not_change_outer_paging() {
        let c =
            filter_clauses("WHERE x = 'a LIMIT b ORDER BY c' AND y IN (SELECT x FROM t LIMIT 1)")
                .unwrap();
        assert!(!c.limit && !c.order_by);
        let c = filter_clauses("WHERE x='ORDER BY' ORDER /*comment*/ BY id LIMIT 5").unwrap();
        assert!(c.order_by && c.limit);
        assert!(!filter_clauses("WHERE \"LIMIT\" = 1").unwrap().limit);
        let c = filter_clauses(r#"WHERE x=E'alpha\' LIMIT' ORDER BY id"#).unwrap();
        assert!(!c.limit && c.order_by);
    }
}
