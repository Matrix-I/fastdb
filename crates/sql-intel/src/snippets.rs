//! Reusable SQL templates and snippets for standard database development.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlSnippet {
    pub id: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub description: &'static str,
    pub template: &'static str,
}

pub fn default_snippets() -> Vec<SqlSnippet> {
    vec![
        // Query snippets
        SqlSnippet {
            id: "select_all",
            title: "Select All Rows",
            category: "Query",
            description: "Query all columns with a basic WHERE clause filter",
            template: "SELECT *\nFROM table_name\nWHERE condition\nLIMIT 100;",
        },
        SqlSnippet {
            id: "select_count",
            title: "Select Count Aggregation",
            category: "Query",
            description: "Count total rows matching a condition",
            template: "SELECT COUNT(*) AS total_count\nFROM table_name\nWHERE condition;",
        },
        SqlSnippet {
            id: "select_paged",
            title: "Select Paged Results",
            category: "Query",
            description: "Fetch rows with ORDER BY, LIMIT, and OFFSET pagination",
            template: "SELECT *\nFROM table_name\nORDER BY id ASC\nLIMIT 50 OFFSET 0;",
        },
        SqlSnippet {
            id: "select_distinct",
            title: "Select Distinct Values",
            category: "Query",
            description: "Retrieve deduplicated column values",
            template: "SELECT DISTINCT column_name\nFROM table_name\nORDER BY column_name;",
        },
        // DML snippets
        SqlSnippet {
            id: "insert_row",
            title: "Insert Single Row",
            category: "DML",
            description: "Insert a new row specifying column names and literal values",
            template: "INSERT INTO table_name (column1, column2)\nVALUES ('value1', 'value2');",
        },
        SqlSnippet {
            id: "insert_returning",
            title: "Insert with Returning",
            category: "DML",
            description: "Insert a row and return generated keys or attributes",
            template: "INSERT INTO table_name (name, description)\nVALUES ('Item Name', 'Item Description')\nRETURNING id, created_at;",
        },
        SqlSnippet {
            id: "update_rows",
            title: "Update Filtered Rows",
            category: "DML",
            description: "Update column values for rows matching a predicate",
            template: "UPDATE table_name\nSET column1 = 'new_value',\n    updated_at = CURRENT_TIMESTAMP\nWHERE id = 1;",
        },
        SqlSnippet {
            id: "delete_rows",
            title: "Delete Filtered Rows",
            category: "DML",
            description: "Remove rows matching a specific condition",
            template: "DELETE FROM table_name\nWHERE id = 1;",
        },
        SqlSnippet {
            id: "upsert_pg",
            title: "Upsert on Conflict (PostgreSQL / SQLite)",
            category: "DML",
            description: "Insert or update existing row on unique key conflict",
            template: "INSERT INTO table_name (id, name, updated_at)\nVALUES (1, 'Sample', CURRENT_TIMESTAMP)\nON CONFLICT (id) DO UPDATE\nSET name = EXCLUDED.name,\n    updated_at = EXCLUDED.updated_at;",
        },
        // DDL snippets
        SqlSnippet {
            id: "create_table",
            title: "Create Table with Primary Key",
            category: "DDL",
            description: "Define a new table with typed columns and primary key constraint",
            template: "CREATE TABLE table_name (\n    id SERIAL PRIMARY KEY,\n    name VARCHAR(255) NOT NULL,\n    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP\n);",
        },
        SqlSnippet {
            id: "create_view",
            title: "Create View",
            category: "DDL",
            description: "Define a virtual view over an underlying SQL query",
            template: "CREATE VIEW view_name AS\nSELECT id, name\nFROM table_name\nWHERE active = TRUE;",
        },
        SqlSnippet {
            id: "create_index",
            title: "Create B-Tree Index",
            category: "DDL",
            description: "Create an index to accelerate queries on specific columns",
            template: "CREATE INDEX idx_table_column\nON table_name (column_name);",
        },
        SqlSnippet {
            id: "alter_add_column",
            title: "Alter Table Add Column",
            category: "DDL",
            description: "Add a new column with type and constraints",
            template: "ALTER TABLE table_name\nADD COLUMN new_column VARCHAR(100);",
        },
        SqlSnippet {
            id: "drop_table",
            title: "Drop Table",
            category: "DDL",
            description: "Drop a table if it exists, cascading constraints",
            template: "DROP TABLE IF EXISTS table_name CASCADE;",
        },
        // Advanced
        SqlSnippet {
            id: "cte_query",
            title: "Common Table Expression (CTE)",
            category: "Advanced",
            description: "Define temporary named result sets for complex multi-step queries",
            template: "WITH cte_summary AS (\n    SELECT department_id, COUNT(*) AS employee_count\n    FROM employees\n    GROUP BY department_id\n)\nSELECT *\nFROM cte_summary\nWHERE employee_count > 5;",
        },
        SqlSnippet {
            id: "inner_join",
            title: "Inner Join Two Tables",
            category: "Advanced",
            description: "Join two tables matching on foreign key relationship",
            template: "SELECT a.id, a.name, b.detail\nFROM table_a a\nINNER JOIN table_b b ON a.b_id = b.id;",
        },
        SqlSnippet {
            id: "left_join",
            title: "Left Outer Join Two Tables",
            category: "Advanced",
            description: "Join tables preserving all records from the left table",
            template: "SELECT a.id, a.name, b.detail\nFROM table_a a\nLEFT JOIN table_b b ON a.b_id = b.id;",
        },
        SqlSnippet {
            id: "window_function",
            title: "Window Function Aggregation",
            category: "Advanced",
            description: "Compute partition-based aggregate without collapsing rows",
            template: "SELECT id, category, amount,\n    AVG(amount) OVER (PARTITION BY category) AS avg_category_amount,\n    ROW_NUMBER() OVER (PARTITION BY category ORDER BY amount DESC) AS rank\nFROM sales;",
        },
    ]
}

/// Filters snippets by case-insensitive matching across title, category, description, and ID.
pub fn filter_snippets(snippets: &[SqlSnippet], query: &str) -> Vec<SqlSnippet> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return snippets.to_vec();
    }
    snippets
        .iter()
        .filter(|s| {
            s.title.to_lowercase().contains(&q)
                || s.category.to_lowercase().contains(&q)
                || s.description.to_lowercase().contains(&q)
                || s.id.to_lowercase().contains(&q)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_snippets_contains_core_categories() {
        let snippets = default_snippets();
        assert!(!snippets.is_empty());
        let categories: Vec<&str> = snippets.iter().map(|s| s.category).collect();
        assert!(categories.contains(&"Query"));
        assert!(categories.contains(&"DML"));
        assert!(categories.contains(&"DDL"));
        assert!(categories.contains(&"Advanced"));
    }

    #[test]
    fn filter_matches_title_and_category() {
        let snippets = default_snippets();
        let query_snippets = filter_snippets(&snippets, "cte");
        assert_eq!(query_snippets.len(), 1);
        assert_eq!(query_snippets[0].id, "cte_query");

        let ddl_snippets = filter_snippets(&snippets, "DDL");
        assert!(ddl_snippets.len() >= 4);
    }
}
