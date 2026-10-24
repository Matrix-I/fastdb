use crate::schema_cache::SchemaCache;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Keyword,
    Table,
    Column,
    Function,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionItem {
    pub label: String,
    pub detail: String,
    pub kind: CompletionKind,
}

const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "JOIN", "INNER JOIN", "LEFT JOIN", "RIGHT JOIN", "ON",
    "GROUP BY", "ORDER BY", "HAVING", "LIMIT", "OFFSET", "INSERT INTO", "VALUES",
    "UPDATE", "SET", "DELETE FROM", "CREATE TABLE", "DROP TABLE", "ALTER TABLE",
    "AS", "DISTINCT", "AND", "OR", "NOT", "IN", "IS NULL", "IS NOT NULL",
    "UNION ALL", "UNION", "EXISTS", "BETWEEN", "CASE", "WHEN", "THEN", "ELSE", "END",
];

const SQL_FUNCTIONS: &[&str] = &[
    "COUNT", "SUM", "AVG", "MIN", "MAX", "COALESCE", "NULLIF", "NOW", "LOWER",
    "UPPER", "TRIM", "LENGTH", "SUBSTRING", "CONCAT", "ROUND", "DATE_TRUNC",
];

pub fn get_completions(
    query: &str,
    cursor_pos: usize,
    cache: &SchemaCache,
) -> Vec<CompletionItem> {
    let prefix = &query[..cursor_pos.min(query.len())];
    let mut items = Vec::new();

    // Check if cursor follows a dot (e.g. `table.`, `users.`)
    if let Some(last_dot_pos) = prefix.rfind('.') {
        let after_dot = &prefix[last_dot_pos + 1..];
        let before_dot = prefix[..last_dot_pos].trim_end();

        // Extract identifier before dot
        let table_ident = before_dot
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .last()
            .unwrap_or("");

        if !table_ident.is_empty() {
            if let Ok(cols) = cache.search_columns(table_ident, after_dot) {
                for col in cols {
                    items.push(CompletionItem {
                        label: col,
                        detail: format!("Column ({table_ident})"),
                        kind: CompletionKind::Column,
                    });
                }
                return items;
            }
        }
    }

    // Extract current word prefix
    let current_word = prefix
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .last()
        .unwrap_or("")
        .to_uppercase();

    // Context check: after FROM or JOIN?
    let trimmed_upper = prefix.trim_end().to_uppercase();
    let is_after_from_or_join = trimmed_upper.ends_with("FROM")
        || trimmed_upper.ends_with("JOIN")
        || trimmed_upper.contains("FROM ")
        || trimmed_upper.contains("JOIN ");

    if is_after_from_or_join {
        if let Ok(tables) = cache.search_tables(&current_word.to_lowercase()) {
            for tbl in tables {
                items.push(CompletionItem {
                    label: tbl,
                    detail: "Table".to_string(),
                    kind: CompletionKind::Table,
                });
            }
        }
    }

    // Suggest matching SQL Keywords
    for &kw in SQL_KEYWORDS {
        if current_word.is_empty() || kw.starts_with(&current_word) {
            items.push(CompletionItem {
                label: kw.to_string(),
                detail: "Keyword".to_string(),
                kind: CompletionKind::Keyword,
            });
        }
    }

    // Suggest matching SQL Functions
    for &func in SQL_FUNCTIONS {
        if current_word.is_empty() || func.starts_with(&current_word) {
            items.push(CompletionItem {
                label: format!("{func}()"),
                detail: "Function".to_string(),
                kind: CompletionKind::Function,
            });
        }
    }

    // Suggest general table names
    if !is_after_from_or_join {
        if let Ok(tables) = cache.search_tables(&current_word.to_lowercase()) {
            for tbl in tables {
                items.push(CompletionItem {
                    label: tbl,
                    detail: "Table".to_string(),
                    kind: CompletionKind::Table,
                });
            }
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_completion_keywords_and_tables() {
        let cache = SchemaCache::new().unwrap();
        cache.add_table("public", "customers", "BASE TABLE").unwrap();
        cache.add_column("public", "customers", "email", "varchar", false).unwrap();

        // 1. Keyword completion
        let res = get_completions("SEL", 3, &cache);
        assert!(res.iter().any(|item| item.label == "SELECT"));

        // 2. Table completion after FROM
        let res2 = get_completions("SELECT * FROM cust", 18, &cache);
        assert!(res2.iter().any(|item| item.label == "customers"));

        // 3. Column completion after dot
        let res3 = get_completions("SELECT customers.", 17, &cache);
        assert!(res3.iter().any(|item| item.label == "email"));
    }
}
