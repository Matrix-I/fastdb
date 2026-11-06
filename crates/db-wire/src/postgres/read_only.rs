//! PostgreSQL's native read-only mode applies to writes hidden inside queries.
use super::*;

pub(super) const DENIED: &str = "This PostgreSQL connection is read-only";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Begin,
    SessionControl,
    Query,
}

fn classify(sql: &str) -> Result<Command> {
    let statement = without_leading_comments(sql)
        .trim_end()
        .trim_end_matches(';')
        .trim_end();
    if statement.eq_ignore_ascii_case("BEGIN") {
        return Ok(Command::Begin);
    }
    let mut words = statement.split_ascii_whitespace();
    let first = words.next().unwrap_or_default();
    let second = words.next().unwrap_or_default();
    if first.eq_ignore_ascii_case("SAVEPOINT")
        || first.eq_ignore_ascii_case("RELEASE")
        || (first.eq_ignore_ascii_case("ROLLBACK") && second.eq_ignore_ascii_case("TO"))
        || (first.eq_ignore_ascii_case("SET") && second.eq_ignore_ascii_case("search_path"))
        || [
            "BEGIN READ ONLY",
            "COMMIT",
            "ROLLBACK",
            "SET TRANSACTION ISOLATION LEVEL READ COMMITTED",
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
            "SET TRANSACTION ISOLATION LEVEL READ UNCOMMITTED",
        ]
        .iter()
        .any(|command| statement.eq_ignore_ascii_case(command))
    {
        return Ok(Command::SessionControl);
    }
    let keyword = statement.split(|ch: char| !ch.is_ascii_alphabetic()).next();
    anyhow::ensure!(
        keyword.is_some_and(
            |word| ["SELECT", "WITH", "SHOW", "EXPLAIN", "VALUES", "TABLE"]
                .iter()
                .any(|allowed| word.eq_ignore_ascii_case(allowed))
        ),
        "{DENIED}"
    );
    Ok(Command::Query)
}

pub(super) async fn prepare<'a>(client: &Client, sql: &'a str) -> Result<&'a str> {
    match classify(sql)? {
        Command::Begin => Ok("BEGIN READ ONLY"),
        // The extended protocol prepares a single statement, including for
        // schema and savepoint controls with user-supplied identifiers.
        Command::SessionControl => Ok(sql),
        Command::Query => {
            // SELECT can change the default through set_config(). Restore it
            // before each user query. Any open transaction began READ ONLY.
            async {
                crate::sql_observer::observe!(
                    "PostgreSQL",
                    "SET SESSION default_transaction_read_only = on",
                    |query| client.batch_execute(query).await
                )
            }
            .await?;
            Ok(sql)
        }
    }
}

fn without_leading_comments(mut sql: &str) -> &str {
    loop {
        sql = sql.trim_start();
        if let Some(comment) = sql.strip_prefix("--") {
            sql = comment
                .split_once(['\r', '\n'])
                .map_or("", |(_, tail)| tail);
        } else if let Some(mut comment) = sql.strip_prefix("/*") {
            let mut depth = 1;
            while depth > 0 {
                let Some(index) = comment.find(['/', '*']) else {
                    return "";
                };
                comment = &comment[index..];
                if let Some(tail) = comment.strip_prefix("/*") {
                    depth += 1;
                    comment = tail;
                } else if let Some(tail) = comment.strip_prefix("*/") {
                    depth -= 1;
                    comment = tail;
                } else {
                    comment = &comment[1..];
                }
            }
            sql = comment;
        } else {
            return sql;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_classifies_safe_controls_and_rejects_mode_changes() {
        assert_eq!(
            classify("/* nested /* note */ */ BEGIN; ").unwrap(),
            Command::Begin
        );
        for sql in [
            "SET search_path TO \"app schema\"",
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            "SAVEPOINT safe",
            "ROLLBACK TO SAVEPOINT safe",
            "RELEASE SAVEPOINT safe",
            "ROLLBACK",
        ] {
            assert_eq!(classify(sql).unwrap(), Command::SessionControl, "{sql}");
        }
        assert_eq!(classify("-- comment\nSELECT 1").unwrap(), Command::Query);
        for sql in [
            "BEGIN READ WRITE",
            "-- note\rBEGIN READ WRITE /*\nSET search_path */",
            "SET TRANSACTION READ WRITE",
            "SET TRANSACTION ISOLATION LEVEL READ COMMITTED, READ WRITE",
            "SET SESSION default_transaction_read_only = off",
            "COMMIT PREPARED 'other'",
        ] {
            assert!(classify(sql).is_err(), "{sql}");
        }
    }
}
