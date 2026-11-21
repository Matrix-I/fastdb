//! One reading of a grid cell's staged state, shared by the Value Panel, Value Editor and
//! Record view so they cannot disagree on NULL versus column DEFAULT.
use ui_grid::{Changeset, RowRef};

/// What the changeset says about one cell, borrowed from it
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StagedCell<'a> {
    /// A fetched cell with no staged edit; its value is read from the pager
    Unchanged,
    /// A new-row cell with no value, left out of the INSERT so the column default applies
    Default,
    /// Staged as SQL NULL: an edited fetched cell or an explicit NULL in a new row
    Null,
    Text(&'a str),
}

impl<'a> StagedCell<'a> {
    /// `None` when `row` or `col` lies outside the changeset's new rows
    pub(super) fn lookup(changes: &'a Changeset, row: RowRef, col: usize) -> Option<Self> {
        match row {
            RowRef::Fetched(index) => Some(match changes.edited_value(index, col) {
                None => Self::Unchanged,
                Some(None) => Self::Null,
                Some(Some(text)) => Self::Text(text),
            }),
            RowRef::New(index) => {
                let row = changes.inserted_rows.get(index)?;
                if row.explicit_nulls.contains(&col) {
                    return Some(Self::Null);
                }
                Some(
                    row.values
                        .get(col)?
                        .as_deref()
                        .map_or(Self::Default, Self::Text),
                )
            }
        }
    }

    /// An unchanged cell is read from the pager. Callers track the default flag separately.
    pub(super) fn value(self) -> Option<Option<&'a str>> {
        match self {
            Self::Unchanged => None,
            Self::Default | Self::Null => Some(None),
            Self::Text(text) => Some(Some(text)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StagedCell::{self, Default, Null, Text, Unchanged};
    use ui_grid::{Changeset, RowRef};

    #[test]
    fn staged_lookup_preserves_cell_states_and_row_positions() {
        let mut changes = Changeset::new();
        changes.insert_row_at(
            1,
            vec![None, None, Some(String::new()), Some("text".into())],
        );
        changes.inserted_rows[0].explicit_nulls.insert(1);
        changes.set_cell(1, 0, Some(b"old".to_vec()), None);
        changes.set_cell(1, 1, Some(b"old".to_vec()), Some("new".into()));

        let inserted = changes.resolve_row(1);
        assert_eq!(inserted, RowRef::New(0));
        for (col, expected) in [Default, Null, Text(""), Text("text")]
            .into_iter()
            .enumerate()
        {
            let actual = StagedCell::lookup(&changes, inserted, col).unwrap();
            assert_eq!(actual, expected);
        }
        assert_eq!(StagedCell::lookup(&changes, inserted, 4), None);
        assert_eq!(StagedCell::lookup(&changes, RowRef::New(1), 0), None);

        let fetched = changes.resolve_row(2);
        assert_eq!(fetched, RowRef::Fetched(1));
        for (col, expected) in [Null, Text("new"), Unchanged].into_iter().enumerate() {
            assert_eq!(StagedCell::lookup(&changes, fetched, col), Some(expected));
        }

        assert_eq!(Unchanged.value(), None);
        assert_eq!(Null.value(), Some(None));
        assert_eq!(Default.value(), Some(None));
        assert_eq!(Text("").value(), Some(Some("")));

        changes.inserted_rows[0].explicit_nulls.insert(4);
        assert_eq!(StagedCell::lookup(&changes, inserted, 4), Some(Null));
    }
}
