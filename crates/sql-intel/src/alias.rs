//! Table aliases in SQL text: picking one for a table being completed after FROM / JOIN,
//! and finding the table an alias stands for, so `f.` can complete that table's columns.

/// Words that end a table reference, so they are never taken for its alias
const CLAUSE_WORDS: &[&str] = &[
    "WHERE", "ON", "USING", "JOIN", "INNER", "LEFT", "RIGHT", "FULL", "OUTER", "CROSS", "NATURAL",
    "GROUP", "ORDER", "HAVING", "LIMIT", "OFFSET", "UNION", "EXCEPT", "INTERSECT", "WINDOW", "FOR",
    "SET", "VALUES", "RETURNING",
];

/// A name in SQL text: a bare word or a quoted identifier, reduced to its last part
/// when qualified (`public."apiClient"` → `apiClient`)
#[derive(Debug, Clone, PartialEq, Eq)]
enum Item {
    Name { text: String, quoted: bool },
    Comma,
    Other,
}

impl Item {
    /// The keyword this item spells, uppercased, if it is a bare word
    fn bare_upper(&self) -> Option<String> {
        match self {
            Item::Name { text, quoted: false } => Some(text.to_uppercase()),
            _ => None,
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// Splits SQL into names, commas and everything else, skipping strings and `--` comments
fn items(query: &str) -> Vec<Item> {
    let chars: Vec<char> = query.chars().collect();
    let mut out: Vec<Item> = Vec::new();
    // Set right after a `.`, so the next name replaces the qualifier before it
    let mut after_dot = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let name = if c == '"' || c == '`' {
            let close = (i + 1..chars.len()).find(|&j| chars[j] == c).unwrap_or(chars.len());
            let text: String = chars[i + 1..close].iter().collect();
            i = close + 1;
            Some(Item::Name { text, quoted: true })
        } else if is_word_char(c) {
            let end = (i..chars.len()).find(|&j| !is_word_char(chars[j])).unwrap_or(chars.len());
            let text: String = chars[i..end].iter().collect();
            i = end;
            Some(Item::Name { text, quoted: false })
        } else {
            None
        };
        if let Some(name) = name {
            if after_dot && matches!(out.last(), Some(Item::Name { .. })) {
                out.pop();
            }
            out.push(name);
            after_dot = false;
            continue;
        }
        match c {
            '.' => after_dot = true,
            ',' => {
                out.push(Item::Comma);
                after_dot = false;
            }
            '\'' => {
                i = (i + 1..chars.len()).find(|&j| chars[j] == '\'').unwrap_or(chars.len());
                out.push(Item::Other);
                after_dot = false;
            }
            '-' if chars.get(i + 1) == Some(&'-') => {
                i = (i..chars.len()).find(|&j| chars[j] == '\n').unwrap_or(chars.len());
            }
            c if c.is_whitespace() => {}
            _ => {
                out.push(Item::Other);
                after_dot = false;
            }
        }
        i += 1;
    }
    out
}

/// `(table, alias)` pairs declared after FROM, JOIN or a comma
fn table_aliases(query: &str) -> Vec<(String, Item)> {
    let items = items(query);
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let starts_table = *item == Item::Comma || matches!(item.bare_upper().as_deref(), Some("FROM" | "JOIN"));
        let Some(Item::Name { text: table, .. }) = items.get(i + 1).filter(|_| starts_table) else { continue };
        let mut k = i + 2;
        if items.get(k).and_then(Item::bare_upper).as_deref() == Some("AS") {
            k += 1;
        }
        if let Some(alias @ Item::Name { .. }) = items.get(k) {
            let is_clause = alias.bare_upper().is_some_and(|w| CLAUSE_WORDS.contains(&w.as_str()));
            if !is_clause {
                out.push((table.clone(), alias.clone()));
            }
        }
    }
    out
}

/// Whether a table name typed at `word_start` sits right after FROM or JOIN, where
/// DBeaver-style completion adds an alias after it
pub fn expects_table_alias(query: &str, word_start: usize) -> bool {
    let before = query[..word_start.min(query.len())].trim_end();
    let before = before.strip_suffix(['"', '`']).unwrap_or(before).trim_end();
    let word: String = before.chars().rev().take_while(|&c| is_word_char(c)).collect::<Vec<_>>().into_iter().rev().collect();
    matches!(word.to_uppercase().as_str(), "FROM" | "JOIN")
}

/// Alias for `table`: its first letter in lowercase (`t` when that is not a letter),
/// followed by 1, 2, … while the statement already uses that name
pub fn pick_table_alias(query: &str, table: &str) -> String {
    let base = table
        .chars()
        .next()
        .filter(|c| c.is_alphabetic())
        .map_or('t', |c| c.to_lowercase().next().unwrap_or('t'))
        .to_string();
    let used: Vec<String> = items(query)
        .into_iter()
        .filter_map(|item| match item {
            Item::Name { text, .. } => Some(text.to_lowercase()),
            _ => None,
        })
        .collect();
    std::iter::once(base.clone())
        .chain((1..).map(|n| format!("{base}{n}")))
        .find(|candidate| !used.contains(candidate))
        .unwrap_or(base)
}

/// Table that `alias` stands for in `query` (`FROM "featureFlag" f`, `JOIN users AS u`),
/// without schema or quotes
pub fn resolve_alias(query: &str, alias: &str) -> Option<String> {
    table_aliases(query).into_iter().find_map(|(table, item)| match item {
        Item::Name { text, quoted: true } if text == alias => Some(table),
        Item::Name { text, quoted: false } if text.eq_ignore_ascii_case(alias) => Some(table),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::{expects_table_alias, pick_table_alias, resolve_alias};

    fn at_end(sql: &str, word: &str) -> bool {
        expects_table_alias(sql, sql.len() - word.len())
    }

    #[test]
    fn aliases_follow_tables_after_from_and_join_only() {
        assert!(at_end("SELECT * FROM feat", "feat"));
        assert!(at_end("select * from a x left join feat", "feat"));
        assert!(at_end("SELECT * FROM \"feat", "feat"));
        assert!(at_end("SELECT * FROM ", ""));
        assert!(!at_end("SELECT feat", "feat"));
        assert!(!at_end("INSERT INTO feat", "feat"));
        assert!(!at_end("UPDATE feat", "feat"));
        assert!(!at_end("SELECT * FROM t WHERE feat", "feat"));
    }

    #[test]
    fn alias_is_the_first_letter_numbered_when_taken() {
        assert_eq!(pick_table_alias("SELECT * FROM ", "featureFlag"), "f");
        assert_eq!(pick_table_alias("SELECT * FROM \"featureFlag\" f JOIN ", "feature"), "f1");
        assert_eq!(pick_table_alias("SELECT * FROM a f JOIN b f1 JOIN ", "flags"), "f2");
        assert_eq!(pick_table_alias("SELECT * FROM ", "_audit"), "t");
        assert_eq!(pick_table_alias("SELECT * FROM ", "Users"), "u");
    }

    #[test]
    fn aliases_resolve_to_their_table() {
        let sql = "SELECT * FROM \"featureFlag\" f JOIN public.\"apiClient\" AS ac ON ac.id = f.id, users u WHERE f.";
        assert_eq!(resolve_alias(sql, "f").as_deref(), Some("featureFlag"));
        assert_eq!(resolve_alias(sql, "ac").as_deref(), Some("apiClient"));
        assert_eq!(resolve_alias(sql, "u").as_deref(), Some("users"));
        assert_eq!(resolve_alias(sql, "x"), None);
        // A table without an alias is not mistaken for one
        assert_eq!(resolve_alias("SELECT * FROM users WHERE users.", "users"), None);
        assert_eq!(resolve_alias("SELECT * FROM users WHERE u.", "u"), None);
    }
}
