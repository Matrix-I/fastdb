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
            i += 1;
            loop {
                if i >= b.len() {
                    return Err("Unterminated SQL literal or identifier".into());
                }
                if b[i] == b'\\' && matches!(quote, b'\'' | b'`' | b'"') {
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
    let tokens = tokens(sql)?;
    let mut start = 0;
    let mut has_code = false;
    let mut out = Vec::new();
    for token in &tokens {
        if token.kind == Kind::Word {
            let upper = token.text.to_ascii_uppercase();
            if matches!(
                upper.as_str(),
                "DELIMITER"
                    | "GO"
                    | "BEGIN"
                    | "COMMIT"
                    | "ROLLBACK"
                    | "SAVEPOINT"
                    | "TRANSACTION"
                    | "PROCEDURE"
                    | "TRIGGER"
                    | "DECLARE"
            ) {
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
