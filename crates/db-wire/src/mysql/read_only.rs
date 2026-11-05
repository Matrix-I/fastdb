//! Guard MySQL's text protocol, which otherwise accepts multiple statements.
use super::*;
use std::borrow::Cow;

pub(super) const DENIED: &str = "This MySQL/MariaDB connection is read-only";
const NATIVE_MODE: &str = "SET SESSION TRANSACTION READ ONLY";

fn statement(sql: &str) -> Result<(Cow<'_, str>, bool)> {
    let mut leading = sql.trim_start();
    loop {
        if let Some(comment) = leading.strip_prefix('#').or_else(|| {
            leading.strip_prefix("--").filter(|tail| {
                tail.as_bytes()
                    .first()
                    .is_some_and(|byte| byte.is_ascii_whitespace())
            })
        }) {
            leading = comment
                .split_once('\n')
                .map_or("", |(_, tail)| tail.trim_start());
        } else if let Some(comment) = leading.strip_prefix("/*")
            && !comment.starts_with(['!', '+'])
            && !comment.starts_with("M!")
        {
            leading = comment
                .split_once("*/")
                .map_or("", |(_, tail)| tail.trim_start());
        } else {
            break;
        }
    }
    let command = leading.trim_end().trim_end_matches(';').trim_end();
    if ["BEGIN", "START TRANSACTION", "START TRANSACTION READ ONLY"]
        .iter()
        .any(|allowed| command.eq_ignore_ascii_case(allowed))
    {
        return Ok(("START TRANSACTION READ ONLY".into(), false));
    }
    if let Some(control) = [
        "COMMIT",
        "ROLLBACK",
        "SET autocommit = 0",
        "SET autocommit = 1",
        "SET SESSION autocommit = 0",
        "SET SESSION autocommit = 1",
        "SET SESSION TRANSACTION ISOLATION LEVEL READ COMMITTED",
        "SET SESSION TRANSACTION ISOLATION LEVEL READ UNCOMMITTED",
        "SET SESSION TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "SET SESSION TRANSACTION ISOLATION LEVEL SERIALIZABLE",
    ]
    .into_iter()
    .find(|allowed| command.eq_ignore_ascii_case(allowed))
    {
        return Ok((control.into(), false));
    }
    let keyword = command
        .split(|ch: char| !ch.is_ascii_alphabetic())
        .next()
        .unwrap_or_default();
    if let Some(control) = identifier_control(command)? {
        return Ok((control.into(), false));
    }
    let permitted = ["SELECT", "WITH", "SHOW", "EXPLAIN", "DESCRIBE", "DESC"]
        .iter()
        .any(|allowed| keyword.eq_ignore_ascii_case(allowed));
    anyhow::ensure!(permitted, "{DENIED}");
    // Only the classified suffix is sent, independent of server comment rules.
    Ok((leading.into(), true))
}

fn identifier_control(command: &str) -> Result<Option<String>> {
    fn take_keyword<'a>(sql: &'a str, keyword: &str) -> Option<&'a str> {
        let (first, rest) = sql.split_once(|ch: char| ch.is_ascii_whitespace())?;
        first
            .eq_ignore_ascii_case(keyword)
            .then(|| rest.trim_start())
    }

    let (prefix, raw) = if let Some(name) = take_keyword(command, "USE") {
        ("USE", name)
    } else if let Some(name) = take_keyword(command, "SAVEPOINT") {
        ("SAVEPOINT", name)
    } else if let Some(name) =
        take_keyword(command, "RELEASE").and_then(|sql| take_keyword(sql, "SAVEPOINT"))
    {
        ("RELEASE SAVEPOINT", name)
    } else if let Some(name) =
        take_keyword(command, "ROLLBACK").and_then(|sql| take_keyword(sql, "TO"))
    {
        (
            "ROLLBACK TO SAVEPOINT",
            take_keyword(name, "SAVEPOINT").unwrap_or(name),
        )
    } else {
        return Ok(None);
    };
    let name = if let Some(quoted) = raw
        .strip_prefix('`')
        .and_then(|name| name.strip_suffix('`'))
    {
        let name = quoted.replace("``", "`");
        anyhow::ensure!(
            crate::sql::quote_ident(crate::DbDriverKind::MySql, &name) == raw,
            "{DENIED}"
        );
        name
    } else {
        anyhow::ensure!(
            raw.bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_$".contains(&byte)),
            "{DENIED}"
        );
        raw.to_owned()
    };
    crate::session_context::validate_name(&name).map_err(anyhow::Error::msg)?;
    let quoted = crate::sql::quote_ident(crate::DbDriverKind::MySql, &name);
    Ok(Some(format!("{prefix} {quoted}")))
}

pub(super) async fn prepare<'a>(
    connection: &mut mysql_async::Conn,
    sql: &'a str,
) -> Result<(Cow<'a, str>, bool)> {
    let sql = statement(sql)?;
    connection.query_drop(NATIVE_MODE).await?;
    Ok(sql)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mysql_read_only_policy_rejects_mode_changes_and_executable_comments() {
        assert_eq!(
            statement("BEGIN; ").unwrap(),
            ("START TRANSACTION READ ONLY".into(), false)
        );
        assert!(statement("/* comment */ SELECT 1").is_ok());
        assert_eq!(
            statement("USE `app schema`").unwrap(),
            ("USE `app schema`".into(), false)
        );
        assert_eq!(
            statement("rollback to safe").unwrap(),
            ("ROLLBACK TO SAVEPOINT `safe`".into(), false)
        );
        assert_eq!(
            statement("SAVEPOINT `s``1`").unwrap(),
            ("SAVEPOINT `s``1`".into(), false)
        );
        for sql in [
            "START TRANSACTION READ WRITE",
            "USE `db`; DELETE FROM items",
            "SAVEPOINT `x`; SELECT `y`",
            "ROLLBACK TO safe; START TRANSACTION READ WRITE",
            "SET SESSION TRANSACTION READ WRITE",
            "/*!80000 SET SESSION TRANSACTION READ WRITE */ SELECT 1",
            "/*M! SET SESSION TRANSACTION READ WRITE */ SELECT 1",
            "-- note\rSELECT ignored\nSTART TRANSACTION READ WRITE",
            "CREATE TEMPORARY TABLE items (id int)",
        ] {
            assert!(statement(sql).is_err(), "{sql}");
        }
    }
}
