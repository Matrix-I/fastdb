use std::collections::HashMap;
use mem_engine::QueryResultPager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellKey {
    pub row_idx: u64,
    pub col_idx: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModifiedCell {
    pub original_value: Option<Vec<u8>>,
    pub new_value: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct Changeset {
    pub changes: HashMap<CellKey, ModifiedCell>,
}

impl Changeset {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_modified(&self, row_idx: u64, col_idx: usize) -> bool {
        self.changes.contains_key(&CellKey { row_idx, col_idx })
    }

    pub fn get_modified_value(&self, row_idx: u64, col_idx: usize) -> Option<&str> {
        self.changes
            .get(&CellKey { row_idx, col_idx })
            .and_then(|m| m.new_value.as_deref())
    }

    pub fn set_cell(
        &mut self,
        row_idx: u64,
        col_idx: usize,
        original_value: Option<Vec<u8>>,
        new_value: Option<String>,
    ) {
        let key = CellKey { row_idx, col_idx };
        self.changes.insert(
            key,
            ModifiedCell {
                original_value,
                new_value,
            },
        );
    }

    pub fn revert_cell(&mut self, row_idx: u64, col_idx: usize) {
        self.changes.remove(&CellKey { row_idx, col_idx });
    }

    pub fn clear(&mut self) {
        self.changes.clear();
    }

    pub fn count(&self) -> usize {
        self.changes.len()
    }

    /// Generates parameterized or quoted UPDATE SQL statements for all staged inline edits
    pub fn generate_update_sql(&self, table_name: &str, pager: &QueryResultPager) -> Vec<String> {
        if self.changes.is_empty() {
            return Vec::new();
        }

        // Group cell modifications by row
        let mut row_modifications: HashMap<u64, Vec<(usize, &Option<String>)>> = HashMap::new();
        for (key, mod_cell) in &self.changes {
            row_modifications
                .entry(key.row_idx)
                .or_default()
                .push((key.col_idx, &mod_cell.new_value));
        }

        // Identify primary key columns if marked
        let pk_cols: Vec<(usize, &str)> = pager
            .columns
            .iter()
            .enumerate()
            .filter(|(_, col)| col.is_primary_key)
            .map(|(idx, col)| (idx, col.name.as_str()))
            .collect();

        let mut sql_statements = Vec::new();

        for (row_idx, edits) in row_modifications {
            let mut set_clauses = Vec::new();
            for (col_idx, new_val) in edits {
                let col_name = pager
                    .columns
                    .get(col_idx)
                    .map(|c| c.name.as_str())
                    .unwrap_or("col");

                match new_val {
                    Some(val) => {
                        let escaped = val.replace('\'', "''");
                        set_clauses.push(format!("\"{col_name}\" = '{escaped}'"));
                    }
                    None => {
                        set_clauses.push(format!("\"{col_name}\" = NULL"));
                    }
                }
            }

            let mut where_clauses = Vec::new();
            if !pk_cols.is_empty() {
                // Use primary keys for robust identification
                for &(pk_idx, pk_name) in &pk_cols {
                    if let Some(cell) = pager.get_cell(row_idx, pk_idx) {
                        let s = String::from_utf8_lossy(&cell).replace('\'', "''");
                        where_clauses.push(format!("\"{pk_name}\" = '{s}'"));
                    }
                }
            } else {
                // Fallback to all original values for optimistic locking
                for (col_idx, col) in pager.columns.iter().enumerate() {
                    let col_name = col.name.as_str();
                    if let Some(orig_cell) = pager.get_cell(row_idx, col_idx) {
                        let s = String::from_utf8_lossy(&orig_cell).replace('\'', "''");
                        where_clauses.push(format!("\"{col_name}\" = '{s}'"));
                    } else {
                        where_clauses.push(format!("\"{col_name}\" IS NULL"));
                    }
                }
            }

            let sql = format!(
                "UPDATE \"{}\" SET {} WHERE {};",
                table_name,
                set_clauses.join(", "),
                where_clauses.join(" AND ")
            );
            sql_statements.push(sql);
        }

        sql_statements
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mem_engine::{ColumnMeta, DataType};

    #[test]
    fn test_changeset_and_sql_generation() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int4).with_pk(true),
            ColumnMeta::new("username", DataType::Text),
        ];
        let pager = QueryResultPager::new(cols).unwrap();
        pager.append_row(vec![Some(&b"42"[..]), Some(&b"old_name"[..])]).unwrap();

        let mut cs = Changeset::new();
        cs.set_cell(0, 1, Some(b"old_name".to_vec()), Some("new_name".to_string()));

        assert!(cs.is_modified(0, 1));
        assert_eq!(cs.get_modified_value(0, 1), Some("new_name"));

        let stmts = cs.generate_update_sql("users", &pager);
        assert_eq!(stmts.len(), 1);
        assert_eq!(
            stmts[0],
            "UPDATE \"users\" SET \"username\" = 'new_name' WHERE \"id\" = '42';"
        );
    }
}
