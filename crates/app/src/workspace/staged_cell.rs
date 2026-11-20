use ui_grid::{Changeset, RowRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StagedCell<'a> {
    Unchanged,
    Default,
    Null,
    Text(&'a str),
}

impl<'a> StagedCell<'a> {
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
