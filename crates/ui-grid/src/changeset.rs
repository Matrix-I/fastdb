use std::collections::{BTreeSet, HashMap};
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

/// How identifiers and string literals are written in generated SQL
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlDialect {
    /// `"ident"` and `'literal'` (PostgreSQL, SQLite)
    Ansi,
    /// `` `ident` `` and `'literal'` with backslashes escaped
    MySql,
}

impl SqlDialect {
    pub fn quote_ident(self, name: &str) -> String {
        match self {
            SqlDialect::Ansi => format!("\"{}\"", name.replace('"', "\"\"")),
            SqlDialect::MySql => format!("`{}`", name.replace('`', "``")),
        }
    }

    pub fn literal(self, value: Option<&str>) -> String {
        match value {
            None => "NULL".to_string(),
            Some(v) => match self {
                SqlDialect::Ansi => format!("'{}'", v.replace('\'', "''")),
                SqlDialect::MySql => format!("'{}'", v.replace('\\', "\\\\").replace('\'', "''")),
            },
        }
    }
}

/// A row added in the grid, not yet written to the table
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertedRow {
    /// Index of the row among all rows shown in the grid
    pub position: u64,
    /// A `None` cell was never set and is left out of the INSERT so the column default applies
    pub values: Vec<Option<String>>,
}

/// What a row shown in the grid is: a fetched row, by its index in the result, or a
/// new row, by its index in `Changeset::inserted_rows`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowRef {
    Fetched(u64),
    New(usize),
}

/// Edits staged in the result grid until they are saved to the database or cancelled:
/// changed cells of fetched rows, rows added between them, and fetched rows marked for
/// deletion. Edits and deletions refer to fetched rows by their index in the result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changeset {
    pub changes: HashMap<CellKey, ModifiedCell>,
    /// New rows, ordered by their position in the grid
    pub inserted_rows: Vec<InsertedRow>,
    pub deleted_rows: BTreeSet<u64>,
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

    /// The staged value of a fetched cell: `None` when unchanged, `Some(None)` when set to NULL
    pub fn edited_value(&self, row_idx: u64, col_idx: usize) -> Option<Option<&str>> {
        self.changes
            .get(&CellKey { row_idx, col_idx })
            .map(|m| m.new_value.as_deref())
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
        self.inserted_rows.clear();
        self.deleted_rows.clear();
    }

    /// Number of pending changes: edited cells, new rows and deleted rows
    pub fn count(&self) -> usize {
        self.changes.len() + self.inserted_rows.len() + self.deleted_rows.len()
    }

    pub fn is_dirty(&self) -> bool {
        self.count() > 0
    }

    pub fn is_deleted(&self, row_idx: u64) -> bool {
        self.deleted_rows.contains(&row_idx)
    }

    /// What the grid row at `position` is. Positions past the last row map to fetched
    /// indexes past the result, which callers bound by the row count.
    pub fn resolve_row(&self, position: u64) -> RowRef {
        let before = self.inserted_rows.partition_point(|r| r.position < position);
        match self.inserted_rows.get(before) {
            Some(r) if r.position == position => RowRef::New(before),
            _ => RowRef::Fetched(position - before as u64),
        }
    }

    /// Adds a new row at grid `position`, moving the rows from there down by one
    pub fn insert_row_at(&mut self, position: u64, values: Vec<Option<String>>) {
        for r in self.inserted_rows.iter_mut().filter(|r| r.position >= position) {
            r.position += 1;
        }
        let at = self.inserted_rows.partition_point(|r| r.position < position);
        self.inserted_rows.insert(at, InsertedRow { position, values });
    }

    /// Drops the new row `index`, moving the rows below it up by one
    pub fn remove_inserted_row(&mut self, index: usize) {
        let removed = self.inserted_rows.remove(index);
        for r in self.inserted_rows.iter_mut().filter(|r| r.position > removed.position) {
            r.position -= 1;
        }
    }

    /// Generates the statements that apply every staged change: UPDATEs for edited rows,
    /// INSERTs for new rows and DELETEs for deleted rows. Rows are identified by the
    /// `key_cols` values, or by all of their original values when there is no key.
    pub fn generate_sql(
        &self,
        target: &str,
        pager: &QueryResultPager,
        key_cols: &[usize],
        dialect: SqlDialect,
    ) -> Vec<String> {
        let col_name = |c: usize| {
            let name = pager.columns.get(c).map(|col| col.name.as_str()).unwrap_or("col");
            dialect.quote_ident(name)
        };
        let where_clause = |row: u64| {
            let all_cols: Vec<usize> = (0..pager.columns.len()).collect();
            let cols = if key_cols.is_empty() { &all_cols[..] } else { key_cols };
            cols.iter()
                .map(|&c| match pager.get_cell(row, c) {
                    Some(bytes) => format!("{} = {}", col_name(c), dialect.literal(Some(&String::from_utf8_lossy(&bytes)))),
                    None => format!("{} IS NULL", col_name(c)),
                })
                .collect::<Vec<_>>()
                .join(" AND ")
        };

        let mut statements = Vec::new();

        let mut edited_rows: HashMap<u64, Vec<(usize, Option<&str>)>> = HashMap::new();
        for (key, cell) in &self.changes {
            if !self.deleted_rows.contains(&key.row_idx) {
                edited_rows.entry(key.row_idx).or_default().push((key.col_idx, cell.new_value.as_deref()));
            }
        }
        let mut edited_rows: Vec<_> = edited_rows.into_iter().collect();
        edited_rows.sort_by_key(|(row, _)| *row);
        for (row, mut edits) in edited_rows {
            edits.sort_by_key(|(c, _)| *c);
            let set_clause = edits
                .iter()
                .map(|&(c, v)| format!("{} = {}", col_name(c), dialect.literal(v)))
                .collect::<Vec<_>>()
                .join(", ");
            statements.push(format!("UPDATE {target} SET {set_clause} WHERE {};", where_clause(row)));
        }

        for row in &self.inserted_rows {
            let set: Vec<(usize, &str)> = row
                .values
                .iter()
                .enumerate()
                .filter_map(|(c, v)| v.as_deref().map(|v| (c, v)))
                .collect();
            if set.is_empty() {
                statements.push(match dialect {
                    SqlDialect::Ansi => format!("INSERT INTO {target} DEFAULT VALUES;"),
                    SqlDialect::MySql => format!("INSERT INTO {target} () VALUES ();"),
                });
            } else {
                let cols = set.iter().map(|&(c, _)| col_name(c)).collect::<Vec<_>>().join(", ");
                let vals = set.iter().map(|&(_, v)| dialect.literal(Some(v))).collect::<Vec<_>>().join(", ");
                statements.push(format!("INSERT INTO {target} ({cols}) VALUES ({vals});"));
            }
        }

        for &row in &self.deleted_rows {
            statements.push(format!("DELETE FROM {target} WHERE {};", where_clause(row)));
        }

        statements
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mem_engine::{ColumnMeta, DataType};

    fn users_pager() -> QueryResultPager {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int4),
            ColumnMeta::new("username", DataType::Text),
        ];
        let pager = QueryResultPager::new(cols).unwrap();
        pager.append_row(vec![Some(&b"42"[..]), Some(&b"old_name"[..])]).unwrap();
        pager.append_row(vec![Some(&b"43"[..]), None]).unwrap();
        pager
    }

    #[test]
    fn test_changeset_and_sql_generation() {
        let pager = users_pager();

        let mut cs = Changeset::new();
        cs.set_cell(0, 1, Some(b"old_name".to_vec()), Some("it's new".to_string()));

        assert!(cs.is_modified(0, 1));
        assert_eq!(cs.get_modified_value(0, 1), Some("it's new"));

        let stmts = cs.generate_sql("\"public\".\"users\"", &pager, &[0], SqlDialect::Ansi);
        assert_eq!(
            stmts,
            vec!["UPDATE \"public\".\"users\" SET \"username\" = 'it''s new' WHERE \"id\" = '42';"]
        );
    }

    #[test]
    fn test_insert_and_delete_sql() {
        let pager = users_pager();

        let mut cs = Changeset::new();
        cs.insert_row_at(2, vec![None, None]);
        cs.insert_row_at(2, vec![None, Some("new\\user".to_string())]);
        cs.deleted_rows.insert(1);
        // An edit of a deleted row is dropped: the row goes away anyway
        cs.set_cell(1, 1, None, Some("ignored".to_string()));
        assert_eq!(cs.count(), 4);

        let stmts = cs.generate_sql("`users`", &pager, &[], SqlDialect::MySql);
        assert_eq!(
            stmts,
            vec![
                "INSERT INTO `users` (`username`) VALUES ('new\\\\user');",
                "INSERT INTO `users` () VALUES ();",
                "DELETE FROM `users` WHERE `id` = '43' AND `username` IS NULL;",
            ]
        );
    }

    #[test]
    fn test_new_rows_sit_between_fetched_rows() {
        let mut cs = Changeset::new();
        // Fetched rows 0..3; a row added below row 0, then another above it
        cs.insert_row_at(1, vec![Some("a".to_string())]);
        cs.insert_row_at(1, vec![Some("b".to_string())]);
        let order: Vec<RowRef> = (0..5).map(|p| cs.resolve_row(p)).collect();
        assert_eq!(
            order,
            vec![RowRef::Fetched(0), RowRef::New(0), RowRef::New(1), RowRef::Fetched(1), RowRef::Fetched(2)]
        );
        assert_eq!(cs.inserted_rows[0].values, vec![Some("b".to_string())]);

        cs.remove_inserted_row(0);
        assert_eq!(cs.resolve_row(1), RowRef::New(0));
        assert_eq!(cs.resolve_row(2), RowRef::Fetched(1));
    }
}
