//! Client batch boundaries. Directives never reach the server and are never replayed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptDialect {
    Postgres,
    MySql,
    Sqlite,
    SqlServer,
    Oracle,
    Db2,
}

/// Split a complete script before executing any part. Ambiguous backslash quoting
/// is rejected because its meaning depends on server session settings.
pub fn split(sql: &str, dialect: ScriptDialect) -> Result<Vec<String>, String> {
    split_impl(sql, dialect, false)
}

/// An explicitly selected Oracle block can use the end of selection as its boundary.
pub fn split_selection(sql: &str, dialect: ScriptDialect) -> Result<Vec<String>, String> {
    split_impl(sql, dialect, true)
}

fn split_impl(sql: &str, dialect: ScriptDialect, selected: bool) -> Result<Vec<String>, String> {
    if dialect == ScriptDialect::Postgres {
        return crate::split_session_script(sql);
    }
    let bytes = sql.as_bytes();
    let mut i = 0;
    let mut start = 0;
    let mut has_code = false;
    let mut delimiter = ";".to_string();
    let mut words = Vec::new();
    let mut trigger_depth: usize = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        if i == 0 || bytes[i - 1] == b'\n' {
            let end = sql[i..].find('\n').map_or(sql.len(), |n| i + n);
            let line = sql[i..end].trim();
            let command = line.split_once("--").map_or(line, |(code, _)| code).trim();
            let first = command.split_whitespace().next().unwrap_or("");
            let is_go = dialect == ScriptDialect::SqlServer && first.eq_ignore_ascii_case("GO");
            let is_slash = dialect == ScriptDialect::Oracle && command == "/";
            let directive =
                if dialect == ScriptDialect::MySql && first.eq_ignore_ascii_case("DELIMITER") {
                    Some(command.get(first.len()..).unwrap_or("").trim())
                } else if dialect == ScriptDialect::Db2
                    && line.to_ascii_uppercase().starts_with("--#SET TERMINATOR")
                {
                    Some(line["--#SET TERMINATOR".len()..].trim())
                } else {
                    None
                };
            if let Some(new_delimiter) = directive {
                if has_code {
                    return Err("A delimiter directive cannot interrupt a statement".into());
                }
                if new_delimiter.is_empty()
                    || new_delimiter.len() > 16
                    || !new_delimiter.bytes().all(|b| {
                        b.is_ascii_punctuation() && !matches!(b, b'\'' | b'"' | b'`' | b'\\')
                    })
                    || ["--", "/*", "#"]
                        .iter()
                        .any(|prefix| new_delimiter.starts_with(prefix))
                {
                    return Err("Use a delimiter of 1–16 punctuation bytes without quotes, backslashes or comment prefixes".into());
                }
                delimiter = new_delimiter.into();
                i = end;
                start = end;
                continue;
            }
            if is_go || is_slash {
                if is_go && !command.eq_ignore_ascii_case("GO") {
                    return Err(
                        "GO repetition counts/options are unsupported; batches are never replayed"
                            .into(),
                    );
                }
                if has_code {
                    out.push(sql[start..i].trim().to_owned());
                } else if is_slash {
                    return Err("Slash without a pending PL/SQL block is unsupported; statements are never replayed".into());
                }
                has_code = false;
                words.clear();
                i = end;
                start = end;
                continue;
            }
        }
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if (bytes[i..].starts_with(b"--")
            && (dialect != ScriptDialect::MySql
                || bytes
                    .get(i + 2)
                    .is_none_or(|byte| byte.is_ascii_whitespace() || byte.is_ascii_control())))
            || (dialect == ScriptDialect::MySql && bytes[i] == b'#')
        {
            i = sql[i..].find('\n').map_or(sql.len(), |n| i + n);
            continue;
        }
        if bytes[i..].starts_with(b"/*") {
            if dialect == ScriptDialect::MySql && bytes[i..].starts_with(b"/*!") {
                return Err("MySQL executable comments are unsupported in Run script".into());
            }
            let mut depth = 1;
            i += 2;
            while i < bytes.len() && depth > 0 {
                if bytes[i..].starts_with(b"/*") && matches!(dialect, ScriptDialect::SqlServer) {
                    depth += 1;
                    i += 2;
                } else if bytes[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth != 0 {
                return Err("Unterminated SQL comment".into());
            }
            continue;
        }
        let oracle_block = dialect == ScriptDialect::Oracle && oracle_block(&words);
        if dialect != ScriptDialect::SqlServer
            && !oracle_block
            && trigger_depth == 0
            && sql[i..].starts_with(&delimiter)
        {
            if has_code {
                if delimiter == ";"
                    && matches!(dialect, ScriptDialect::Db2 | ScriptDialect::MySql)
                    && words.first().is_some_and(|word| word == "BEGIN")
                    && words
                        .get(1)
                        .is_some_and(|word| matches!(word.as_str(), "ATOMIC" | "NOT"))
                {
                    return Err("Use a custom delimiter for a compound statement".into());
                }
                if delimiter == ";"
                    && matches!(dialect, ScriptDialect::MySql | ScriptDialect::Db2)
                    && words.first().is_some_and(|word| word == "CREATE")
                    && words.iter().any(|word| {
                        matches!(
                            word.as_str(),
                            "PROCEDURE" | "FUNCTION" | "TRIGGER" | "EVENT"
                        )
                    })
                {
                    return Err("Use a custom delimiter for a stored program body".into());
                }
                let end = if dialect == ScriptDialect::Sqlite || dialect == ScriptDialect::Oracle {
                    i + delimiter.len()
                } else {
                    i
                };
                out.push(sql[start..end].trim().to_owned());
            }
            i += delimiter.len();
            start = i;
            has_code = false;
            words.clear();
            continue;
        }
        has_code = true;
        if dialect == ScriptDialect::Oracle
            && let Some(prefix_len) = oracle_quote_prefix(&bytes[i..])
        {
            let open = *bytes
                .get(i + prefix_len)
                .ok_or("Unterminated Oracle q literal")?;
            if !open.is_ascii() {
                return Err("Use an ASCII delimiter for Oracle q literals".into());
            }
            let close = match open {
                b'[' => b']',
                b'(' => b')',
                b'{' => b'}',
                b'<' => b'>',
                other => other,
            };
            i += prefix_len + 1;
            loop {
                if i + 1 >= bytes.len() {
                    return Err("Unterminated Oracle q literal".into());
                }
                if bytes[i] == close && bytes[i + 1] == b'\'' {
                    i += 2;
                    break;
                }
                i += 1;
            }
        } else if matches!(bytes[i], b'\'' | b'"' | b'`')
            || (matches!(dialect, ScriptDialect::Sqlite | ScriptDialect::SqlServer)
                && bytes[i] == b'[')
        {
            let quote = if bytes[i] == b'[' { b']' } else { bytes[i] };
            i += 1;
            loop {
                if i >= bytes.len() {
                    return Err("Unterminated SQL literal or identifier".into());
                }
                if bytes[i] == b'\\' && dialect == ScriptDialect::MySql {
                    return Err(
                        "Backslash quoting depends on SQL mode; use doubled quotes instead".into(),
                    );
                }
                if bytes[i] == quote {
                    i += 1;
                    if bytes.get(i) == Some(&quote) {
                        i += 1;
                    } else {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' || bytes[i] >= 128 {
            let begin = i;
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric()
                    || matches!(bytes[i], b'_' | b'$')
                    || bytes[i] >= 128)
                && !bytes[i..].starts_with(delimiter.as_bytes())
            {
                i += 1;
            }
            let word = sql[begin..i].to_ascii_uppercase();
            if dialect == ScriptDialect::Sqlite
                && words.first().is_some_and(|word| word == "CREATE")
                && words.iter().take(4).any(|word| word == "TRIGGER")
            {
                match word.as_str() {
                    "BEGIN" | "CASE" => trigger_depth += 1,
                    "END" => {
                        trigger_depth = trigger_depth
                            .checked_sub(1)
                            .ok_or("Unmatched trigger END")?
                    }
                    _ => {}
                }
            }
            words.push(word);
        } else {
            i += 1;
        }
    }
    if trigger_depth != 0 {
        return Err("Unterminated SQLite trigger body".into());
    }
    if has_code {
        if (dialect == ScriptDialect::Oracle && oracle_block(&words) && !selected)
            || (matches!(dialect, ScriptDialect::MySql | ScriptDialect::Db2) && delimiter != ";")
        {
            return Err("Missing script terminator for procedural statement".into());
        }
        out.push(sql[start..].trim().to_owned());
    }
    Ok(out)
}

fn oracle_quote_prefix(bytes: &[u8]) -> Option<usize> {
    match bytes {
        [b'q' | b'Q', b'\'', ..] => Some(2),
        [b'n' | b'N', b'q' | b'Q', b'\'', ..] => Some(3),
        _ => None,
    }
}

fn oracle_block(words: &[String]) -> bool {
    words.first().is_some_and(|first| {
        matches!(first.as_str(), "BEGIN" | "DECLARE")
            || (first == "CREATE"
                && words.iter().take(6).any(|word| {
                    matches!(
                        word.as_str(),
                        "PROCEDURE" | "FUNCTION" | "PACKAGE" | "TRIGGER" | "TYPE"
                    )
                }))
    })
}

#[cfg(test)]
mod tests {
    use super::{ScriptDialect::*, split};

    #[test]
    fn mysql_delimiters_preserve_compound_bodies_and_ignore_quoted_directives() {
        let sql = "DELIMITER $$\nCREATE PROCEDURE p() BEGIN SELECT 'a;$$'; SELECT 2; END$$\nDELIMITER ;\nCALL p(); # comment\nSELECT 3;";
        let parts = split(sql, MySql).unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(
            parts[0],
            "CREATE PROCEDURE p() BEGIN SELECT 'a;$$'; SELECT 2; END"
        );
        assert_eq!(parts[1], "CALL p()");
        assert_eq!(split("SELECT 1--2; SELECT 3;", MySql).unwrap().len(), 2);
        assert!(parts[2].ends_with("SELECT 3"));
        assert!(split("DELIMITER\nSELECT 1;", MySql).is_err());
        assert!(
            split(
                "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; END",
                MySql
            )
            .is_err()
        );
        assert!(split("SELECT 'a\\b';", MySql).is_err());
        assert!(split("SELECT 1;\nDELIMITER 'x'\nSELECT 2;", MySql).is_err());
    }

    #[test]
    fn sqlserver_go_keeps_batch_scope_and_never_repeats_writes() {
        let sql = "DECLARE @n int = 1; SELECT @n, 'GO';\nGO -- separator\nBEGIN TRANSACTION; SELECT 2;\ngo\nROLLBACK;";
        let parts = split(sql, SqlServer).unwrap();
        assert_eq!(parts.len(), 3);
        assert!(parts[0].starts_with("DECLARE @n"));
        assert!(parts[1].contains("BEGIN TRANSACTION; SELECT 2;"));
        assert_eq!(parts[2], "ROLLBACK;");
        assert!(split("INSERT INTO t VALUES (1);\nGO 2", SqlServer).is_err());
        assert_eq!(
            split("SELECT '[GO]'; /*\nGO\n*/ SELECT [GO];", SqlServer)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn oracle_slash_and_db2_terminator_preserve_nested_blocks() {
        let sql = "DECLARE n NUMBER := 1;\nBEGIN SELECT q'[a;/]' INTO n FROM DUAL; BEGIN NULL; END; END;\n/\nSELECT 1 FROM DUAL;";
        let parts = split(sql, Oracle).unwrap();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].ends_with("END;"));
        assert_eq!(parts[1], "SELECT 1 FROM DUAL;");
        assert!(split("BEGIN NULL; END;", Oracle).is_err());
        assert_eq!(
            super::split_selection("BEGIN NULL; END;", Oracle).unwrap(),
            ["BEGIN NULL; END;"]
        );
        assert!(split("BEGIN ATOMIC VALUES 1; VALUES 2; END;", Db2).is_err());
        assert!(split("BEGIN NOT ATOMIC SELECT 1; SELECT 2; END;", MySql).is_err());
        let parts = split("--#SET TERMINATOR @\nBEGIN ATOMIC VALUES 1; VALUES 2; END@\n--#SET TERMINATOR ;\nVALUES 3;", Db2).unwrap();
        assert_eq!(parts, ["BEGIN ATOMIC VALUES 1; VALUES 2; END", "VALUES 3"]);
    }

    #[test]
    fn sqlite_triggers_and_postgres_dollar_bodies_remain_whole() {
        let sql = "CREATE TEMP TRIGGER tr AFTER INSERT ON t BEGIN INSERT INTO log VALUES (CASE WHEN new.id = 1 THEN 'a;' ELSE 'b' END); UPDATE log SET v = 'done'; END; SELECT 1;";
        let parts = split(sql, Sqlite).unwrap();
        assert_eq!(parts.len(), 2);
        assert!(parts[0].ends_with("END;"));
        assert_eq!(
            split("BEGIN; SELECT 1; ROLLBACK;", Sqlite).unwrap().len(),
            3
        );
        assert!(
            split(
                "CREATE TRIGGER tr AFTER INSERT ON t BEGIN SELECT 1;",
                Sqlite
            )
            .is_err()
        );
        assert_eq!(
            split("DO $$BEGIN RAISE NOTICE 'a;b'; END$$; SELECT 1;", Postgres)
                .unwrap()
                .len(),
            2
        );
        assert!(split("SELECT 'unterminated", Postgres).is_err());
    }
}
