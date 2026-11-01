use std::collections::{BTreeSet, HashMap};

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
        let before = self
            .inserted_rows
            .partition_point(|r| r.position < position);
        match self.inserted_rows.get(before) {
            Some(r) if r.position == position => RowRef::New(before),
            _ => RowRef::Fetched(position - before as u64),
        }
    }

    /// Adds a new row at grid `position`, moving the rows from there down by one
    pub fn insert_row_at(&mut self, position: u64, values: Vec<Option<String>>) {
        for r in self
            .inserted_rows
            .iter_mut()
            .filter(|r| r.position >= position)
        {
            r.position += 1;
        }
        let at = self
            .inserted_rows
            .partition_point(|r| r.position < position);
        self.inserted_rows
            .insert(at, InsertedRow { position, values });
    }

    /// Drops the new row `index`, moving the rows below it up by one
    pub fn remove_inserted_row(&mut self, index: usize) {
        let removed = self.inserted_rows.remove(index);
        for r in self
            .inserted_rows
            .iter_mut()
            .filter(|r| r.position > removed.position)
        {
            r.position -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_rows_sit_between_fetched_rows() {
        let mut cs = Changeset::new();
        // Fetched rows 0..3; a row added below row 0, then another above it
        cs.insert_row_at(1, vec![Some("a".to_string())]);
        cs.insert_row_at(1, vec![Some("b".to_string())]);
        let order: Vec<RowRef> = (0..5).map(|p| cs.resolve_row(p)).collect();
        assert_eq!(
            order,
            vec![
                RowRef::Fetched(0),
                RowRef::New(0),
                RowRef::New(1),
                RowRef::Fetched(1),
                RowRef::Fetched(2)
            ]
        );
        assert_eq!(cs.inserted_rows[0].values, vec![Some("b".to_string())]);

        cs.remove_inserted_row(0);
        assert_eq!(cs.resolve_row(1), RowRef::New(0));
        assert_eq!(cs.resolve_row(2), RowRef::Fetched(1));
    }
}
