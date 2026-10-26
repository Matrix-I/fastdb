//! Helpers for SQL script tabs: paging a free-form SELECT like a table.

/// Wraps a plain SELECT in a sub-select reading `size` rows from `offset`, so a script's
/// result is fetched in chunks like a table's. Returns `None` for any other statement, or
/// one that already limits or locks its rows, which then runs exactly as written.
pub(super) fn paged_select(statement: &str, size: usize, offset: u64) -> Option<String> {
    let body = statement.trim().trim_end_matches(|c: char| c == ';' || c.is_whitespace());
    let first_keyword = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("--"))?
        .split(|c: char| !c.is_alphanumeric())
        .next()?
        .to_uppercase();
    if first_keyword != "SELECT" && first_keyword != "WITH" {
        return None;
    }
    let upper = body.to_uppercase();
    let words: Vec<&str> = upper.split(|c: char| !c.is_alphanumeric() && c != '_').collect();
    let has = |w: &str| words.contains(&w);
    if has("LIMIT") || has("OFFSET") || upper.contains("FETCH FIRST") || upper.contains("FETCH NEXT") || upper.contains("FOR UPDATE") {
        return None;
    }
    // The closing parenthesis goes on its own line so a trailing `--` comment cannot swallow it
    let mut sql = format!("SELECT * FROM ({body}\n) fastdb_page LIMIT {size}");
    if offset > 0 {
        sql.push_str(&format!(" OFFSET {offset}"));
    }
    Some(sql)
}


#[cfg(test)]
mod tests {
    use super::paged_select;

    #[test]
    fn plain_selects_are_paged() {
        assert_eq!(
            paged_select("SELECT * FROM users;", 200, 0).as_deref(),
            Some("SELECT * FROM (SELECT * FROM users\n) fastdb_page LIMIT 200")
        );
        assert_eq!(
            paged_select("with t as (select 1) select * from t", 50, 100).as_deref(),
            Some("SELECT * FROM (with t as (select 1) select * from t\n) fastdb_page LIMIT 50 OFFSET 100")
        );
        assert_eq!(
            paged_select("-- latest users\nSELECT id FROM users ;; ", 10, 0).as_deref(),
            Some("SELECT * FROM (-- latest users\nSELECT id FROM users\n) fastdb_page LIMIT 10")
        );
    }

    #[test]
    fn statements_that_limit_lock_or_change_rows_run_as_written() {
        assert_eq!(paged_select("SELECT * FROM users LIMIT 5", 200, 0), None);
        assert_eq!(paged_select("select * from users fetch first 5 rows only", 200, 0), None);
        assert_eq!(paged_select("SELECT * FROM users FOR UPDATE", 200, 0), None);
        assert_eq!(paged_select("UPDATE users SET a = 1", 200, 0), None);
        assert_eq!(paged_select("SHOW TABLES", 200, 0), None);
        assert_eq!(paged_select("   ", 200, 0), None);
    }

    #[test]
    fn a_trailing_comment_does_not_swallow_the_closing_parenthesis() {
        assert_eq!(
            paged_select("SELECT 1 -- one", 10, 0).as_deref(),
            Some("SELECT * FROM (SELECT 1 -- one\n) fastdb_page LIMIT 10")
        );
    }

}
