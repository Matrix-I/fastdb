use crate::{Changeset, GridCoord, RowRef};
use mem_engine::QueryResultPager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindDirection {
    Next,
    Previous,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FindOutcome {
    Found(GridCoord),
    NotFound,
    Cancelled,
    Failed(String),
}

/// Searches displayed values in row-major order, wrapping once and retaining no
/// match list. Call on a worker: fetching spilled pages can perform disk I/O.
pub fn find_cell(
    pager: &QueryResultPager,
    changes: &Changeset,
    needle: &str,
    from: Option<GridCoord>,
    direction: FindDirection,
    cancelled: impl FnMut() -> bool,
) -> FindOutcome {
    find_displayed_cell(
        pager,
        changes,
        &(0..pager.columns.len()).collect::<Vec<_>>(),
        needle,
        from,
        direction,
        cancelled,
    )
}

/// Searches the visible source columns in display order.
pub fn find_displayed_cell(
    pager: &QueryResultPager,
    changes: &Changeset,
    columns: &[usize],
    needle: &str,
    from: Option<GridCoord>,
    direction: FindDirection,
    mut cancelled: impl FnMut() -> bool,
) -> FindOutcome {
    let fetched_rows = pager.total_rows();
    let Some(rows) = fetched_rows.checked_add(changes.inserted_rows.len() as u64) else {
        return FindOutcome::Failed("Result row count exceeds supported range".into());
    };
    let cols = columns.len();
    if columns.iter().any(|&index| index >= pager.columns.len()) {
        return FindOutcome::Failed("Column layout no longer matches this result".into());
    }
    let mut previous_position = None;
    for (index, row) in changes.inserted_rows.iter().enumerate() {
        if cancelled() {
            return FindOutcome::Cancelled;
        }
        if row.position > fetched_rows + index as u64
            || previous_position.is_some_and(|previous| row.position <= previous)
        {
            return FindOutcome::Failed(
                "Staged row positions are invalid; refresh the result".into(),
            );
        }
        previous_position = Some(row.position);
    }
    if needle.is_empty() || rows == 0 || cols == 0 {
        return FindOutcome::NotFound;
    }
    let total = u128::from(rows) * cols as u128;
    let step = |index| match direction {
        FindDirection::Next => {
            if index == total - 1 {
                0
            } else {
                index + 1
            }
        }
        FindDirection::Previous => {
            if index == 0 {
                total - 1
            } else {
                index - 1
            }
        }
    };
    let mut index = from
        .filter(|cell| cell.row < rows)
        .and_then(|cell| {
            columns
                .iter()
                .position(|&column| column == cell.col)
                .map(|position| step(u128::from(cell.row) * cols as u128 + position as u128))
        })
        .unwrap_or(match direction {
            FindDirection::Next => 0,
            FindDirection::Previous => total - 1,
        });
    for _ in 0..total {
        if cancelled() {
            return FindOutcome::Cancelled;
        }
        let cell = GridCoord {
            row: (index / cols as u128) as u64,
            col: columns[(index % cols as u128) as usize],
        };
        match cell_matches(pager, changes, cell, needle) {
            Ok(true) => return FindOutcome::Found(cell),
            Ok(false) => {}
            Err(error) => return FindOutcome::Failed(error),
        }
        index = step(index);
    }
    FindOutcome::NotFound
}

fn cell_matches(
    pager: &QueryResultPager,
    changes: &Changeset,
    cell: GridCoord,
    needle: &str,
) -> Result<bool, String> {
    let fetched = match changes.resolve_row(cell.row) {
        RowRef::New(index) => {
            return Ok(changes
                .inserted_rows
                .get(index)
                .and_then(|row| row.values.get(cell.col))
                .and_then(Option::as_deref)
                .is_some_and(|value| value.contains(needle)));
        }
        RowRef::Fetched(row) => row,
    };
    if let Some(value) = changes.edited_value(fetched, cell.col) {
        return Ok(value.is_some_and(|value| value.contains(needle)));
    }
    // Unlike get_cell/resolve_cell, do not turn a failed spill read into NULL or
    // a misleading "no match". The page borrow also avoids copying cell bytes.
    let (page, row) = pager
        .locate_row(fetched)
        .ok_or("Result changed during search")?;
    let page = pager
        .get_page(page)
        .map_err(|error| format!("Cannot read result page: {error}"))?;
    Ok(page
        .get_cell(row, cell.col)
        .is_some_and(|value| String::from_utf8_lossy(value).contains(needle)))
}

#[cfg(test)]
mod tests {
    use super::{FindDirection, FindOutcome, find_cell};
    use crate::{Changeset, GridCoord};
    use mem_engine::{ColumnMeta, DataType, QueryResultPager};

    fn pager() -> QueryResultPager {
        let pager = QueryResultPager::new(vec![
            ColumnMeta::new("a", DataType::Text),
            ColumnMeta::new("b", DataType::Text),
        ])
        .unwrap();
        pager
            .append_row(vec![Some("名 match".as_bytes()), None])
            .unwrap();
        pager
            .append_row(vec![Some(b"decoy"), Some(b"match")])
            .unwrap();
        pager
    }

    #[test]
    fn spilled_pages_are_searched_without_loading_a_full_result_copy() {
        let pager = QueryResultPager::new(vec![ColumnMeta::new("value", DataType::Text)])
            .unwrap()
            .with_max_resident_pages(2);
        let filler = "x".repeat(4096);
        for row in 0..200 {
            pager
                .append_row(vec![Some(if row == 175 { "disk match" } else { &filler })])
                .unwrap();
        }
        assert_eq!(
            find_cell(
                &pager,
                &Changeset::new(),
                "disk match",
                None,
                FindDirection::Next,
                || false
            ),
            FindOutcome::Found(GridCoord { row: 175, col: 0 })
        );
        assert_eq!(
            find_cell(
                &pager,
                &Changeset::new(),
                "disk match",
                None,
                FindDirection::Previous,
                || false
            ),
            FindOutcome::Found(GridCoord { row: 175, col: 0 })
        );
        assert_eq!(pager.total_rows(), 200);
    }

    #[test]
    fn staged_null_hides_original_and_deleted_rows_remain_searchable_as_displayed() {
        let pager = pager();
        let mut changes = Changeset::new();
        changes.set_cell(0, 0, Some("名 match".as_bytes().to_vec()), None);
        changes.deleted_rows.insert(1);
        assert_eq!(
            find_cell(&pager, &changes, "名", None, FindDirection::Next, || false),
            FindOutcome::NotFound
        );
        assert_eq!(
            find_cell(&pager, &changes, "match", None, FindDirection::Next, || {
                false
            }),
            FindOutcome::Found(GridCoord { row: 1, col: 1 })
        );
    }

    #[test]
    fn invalid_staged_row_positions_fail_instead_of_panicking_or_reporting_no_match() {
        let pager = pager();
        let mut changes = Changeset::new();
        changes.insert_row_at(100, vec![Some("value".into())]);
        assert!(
            matches!(find_cell(&pager, &changes, "value", None, FindDirection::Next, || false), FindOutcome::Failed(message) if message.contains("Staged row positions"))
        );
    }

    #[test]
    fn next_previous_wrap_and_case_are_literal_and_do_not_change_edits() {
        let pager = pager();
        let mut edits = Changeset::new();
        edits.set_cell(1, 0, Some(b"decoy".to_vec()), Some("staged match".into()));
        edits.insert_row_at(1, vec![Some("inserted match".into()), None]);
        let original = edits.clone();
        let find = |from, direction| find_cell(&pager, &edits, "match", from, direction, || false);
        assert_eq!(
            find(None, FindDirection::Next),
            FindOutcome::Found(GridCoord { row: 0, col: 0 })
        );
        assert_eq!(
            find(Some(GridCoord { row: 0, col: 0 }), FindDirection::Next),
            FindOutcome::Found(GridCoord { row: 1, col: 0 })
        );
        assert_eq!(
            find(Some(GridCoord { row: 0, col: 0 }), FindDirection::Previous),
            FindOutcome::Found(GridCoord { row: 2, col: 1 })
        );
        assert_eq!(
            find(Some(GridCoord { row: 2, col: 1 }), FindDirection::Next),
            FindOutcome::Found(GridCoord { row: 0, col: 0 })
        );
        assert_eq!(
            find_cell(&pager, &edits, "Match", None, FindDirection::Next, || false),
            FindOutcome::NotFound
        );
        assert_eq!(
            find_cell(&pager, &edits, "名", None, FindDirection::Next, || false),
            FindOutcome::Found(GridCoord { row: 0, col: 0 })
        );
        assert_eq!(edits, original);
    }

    #[test]
    fn empty_null_invalid_selection_and_cancellation_are_explicit() {
        let pager = pager();
        let edits = Changeset::new();
        assert_eq!(
            find_cell(&pager, &edits, "", None, FindDirection::Next, || false),
            FindOutcome::NotFound
        );
        assert_eq!(
            find_cell(&pager, &edits, "NULL", None, FindDirection::Next, || false),
            FindOutcome::NotFound
        );
        assert_eq!(
            find_cell(
                &pager,
                &edits,
                "match",
                Some(GridCoord {
                    row: u64::MAX,
                    col: usize::MAX
                }),
                FindDirection::Next,
                || false
            ),
            FindOutcome::Found(GridCoord { row: 0, col: 0 })
        );
        let mut checked = 0;
        assert_eq!(
            find_cell(&pager, &edits, "absent", None, FindDirection::Next, || {
                checked += 1;
                checked == 2
            }),
            FindOutcome::Cancelled
        );
        assert_eq!(checked, 2);
        let empty = QueryResultPager::new(Vec::new()).unwrap();
        assert_eq!(
            find_cell(&empty, &edits, "match", None, FindDirection::Next, || false),
            FindOutcome::NotFound
        );
    }
}
