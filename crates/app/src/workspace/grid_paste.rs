use super::*;
use ui_grid::{GridCoord, RowRef};

const MAX_PASTE_BYTES: usize = 256 * 1024;
const MAX_PASTE_CELLS: usize = 1024;

struct PasteCell {
    row: RowRef,
    column: usize,
    value: String,
    original: Option<Vec<u8>>,
}

struct PasteSource {
    pager: Arc<db_wire::QueryResultPager>,
    columns: Vec<usize>,
    rows: Vec<(RowRef, bool)>,
}

impl PasteSource {
    fn capture(grid: &VirtualDataGrid, start: GridCoord) -> Result<Self, String> {
        let pager = grid.pager.clone().ok_or("No loaded result")?;
        let columns = grid.display_columns();
        let first = columns
            .iter()
            .position(|&column| column == start.col)
            .ok_or("Select a visible cell first")?;
        let rows = (start.row..grid.row_count())
            .take(MAX_PASTE_CELLS)
            .map(|row| {
                let source = grid.changeset.resolve_row(row);
                let deleted =
                    matches!(source, RowRef::Fetched(row) if grid.changeset.is_deleted(row));
                (source, deleted)
            })
            .collect();
        Ok(Self {
            pager,
            columns: columns[first..].to_vec(),
            rows,
        })
    }
}

fn prepare_paste(source: &PasteSource, text: &str) -> Result<Vec<PasteCell>, String> {
    if text.len() > MAX_PASTE_BYTES || text.contains('\0') {
        return Err("Paste must fit within 256 KiB and cannot contain NUL".into());
    }
    let pager = &source.pager;
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    let mut cells = Vec::new();
    let mut expected_width = None;
    let mut original_bytes = 0;
    for (offset, line) in text.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let &(row, deleted) = source
            .rows
            .get(offset)
            .ok_or("Paste extends past loaded rows or the 1,024-cell limit")?;
        if deleted {
            return Err("Paste includes a row marked for deletion".into());
        }
        let original = match row {
            RowRef::Fetched(row) => {
                let (page, row) = pager
                    .locate_row(row)
                    .ok_or("Paste source row is unavailable")?;
                Some((
                    pager.get_page(page).map_err(|error| error.to_string())?,
                    row,
                ))
            }
            RowRef::New(_) => None,
        };
        let mut width = 0;
        for (offset, value) in line.split('\t').enumerate() {
            let column = *source
                .columns
                .get(offset)
                .ok_or("Paste extends past visible columns")?;
            if cells.len() == MAX_PASTE_CELLS {
                return Err("Paste supports up to 1,024 cells".into());
            }
            let metadata = &pager.columns[column];
            if pager.query_edit_origin.as_ref().is_some_and(|origin| {
                !origin.columns[column].writable || origin.columns[column].key
            }) {
                return Err(format!("Column {} is read-only", metadata.name));
            }
            if !valid_text(metadata.data_type, value) {
                return Err(format!(
                    "Invalid or unsupported value for column {}",
                    metadata.name
                ));
            }
            let original = original
                .as_ref()
                .and_then(|(page, row)| page.get_cell(*row, column));
            original_bytes += original.map_or(0, <[u8]>::len);
            if original_bytes > MAX_PASTE_BYTES {
                return Err("Original cell values exceed the 256 KiB paste budget".into());
            }
            let value = normalized_text(metadata.data_type, value);
            cells.push(PasteCell {
                row,
                column,
                value: value.into(),
                original: original.map(<[u8]>::to_vec),
            });
            width += 1;
        }
        if expected_width
            .replace(width)
            .is_some_and(|expected| expected != width)
        {
            return Err("Paste requires a rectangular tab-separated block".into());
        }
    }
    Ok(cells)
}

fn normalized_text(kind: DataType, value: &str) -> &str {
    if kind == DataType::Bool {
        if value.eq_ignore_ascii_case("true") || value == "1" {
            "1"
        } else {
            "0"
        }
    } else {
        value
    }
}

fn valid_text(kind: DataType, value: &str) -> bool {
    match kind {
        DataType::Text | DataType::Varchar => true,
        DataType::Bool => matches!(
            value.to_ascii_lowercase().as_str(),
            "true" | "false" | "0" | "1"
        ),
        DataType::Int2 => value.parse::<i16>().is_ok(),
        DataType::Int4 => value.parse::<i32>().is_ok(),
        DataType::Int8 => value.parse::<i64>().is_ok(),
        DataType::Float4 => value.parse::<f32>().is_ok_and(f32::is_finite),
        DataType::Float8 => value.parse::<f64>().is_ok_and(f64::is_finite),
        DataType::Numeric => {
            let value = value.strip_prefix(['-', '+']).unwrap_or(value);
            value.bytes().any(|byte| byte.is_ascii_digit())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'.')
                && value.bytes().filter(|byte| *byte == b'.').count() <= 1
        }
        DataType::Json => serde_json::from_str::<serde_json::Value>(value).is_ok(),
        DataType::Uuid => {
            value.len() == 36
                && value.bytes().enumerate().all(|(index, byte)| {
                    if matches!(index, 8 | 13 | 18 | 23) {
                        byte == b'-'
                    } else {
                        byte.is_ascii_hexdigit()
                    }
                })
        }
        _ => false,
    }
}

fn apply_paste(grid: &mut VirtualDataGrid, cells: Vec<PasteCell>) {
    grid.stage(|changes| {
        for cell in cells {
            match cell.row {
                RowRef::New(row) => {
                    let row = &mut changes.inserted_rows[row];
                    row.explicit_nulls.remove(&cell.column);
                    row.values[cell.column] = Some(cell.value);
                }
                RowRef::Fetched(row) => {
                    if cell.original.as_deref() == Some(cell.value.as_bytes()) {
                        changes.revert_cell(row, cell.column);
                    } else {
                        changes.set_cell(row, cell.column, cell.original, Some(cell.value));
                    }
                }
            }
        }
    });
}

impl Workspace {
    pub(super) fn paste_grid_cells(&mut self, cx: &mut Context<Self>) {
        if !self.can_edit_active_rows() {
            return;
        }
        let Some(driver) = self
            .tab_session(self.active_tab)
            .driver
            .as_ref()
            .map(Arc::downgrade)
        else {
            return;
        };
        let tab = &mut self.tabs[self.active_tab];
        if tab.view_mode != DataViewMode::Grid
            || tab.grid.editing_cell.is_some()
            || tab.schema_stale
        {
            return;
        }
        let Some(start) = tab.grid.selection.focus else {
            return;
        };
        let Some(text) = cx
            .read_from_clipboard()
            .and_then(|clipboard| clipboard.text())
        else {
            return;
        };
        if text.len() > MAX_PASTE_BYTES {
            tab.last_error = Some("Paste must fit within 256 KiB".into());
            cx.notify();
            return;
        }
        let source = match PasteSource::capture(&tab.grid, start) {
            Ok(source) => source,
            Err(error) => {
                tab.last_error = Some(error);
                cx.notify();
                return;
            }
        };
        if tab.paste_pending {
            return;
        }
        tab.paste_pending = true;
        let tab_id = tab.id;
        let pager = source.pager.clone();
        let revision = tab.grid.changeset_revision();
        let layout = tab.grid.display_columns().to_vec();
        let worker = cx
            .background_executor()
            .spawn(async move { prepare_paste(&source, &text) });
        cx.spawn(async move |this, cx| {
            let result = worker.await;
            let _ = this.update(cx, |this, cx| {
                let Some(index) = this.tabs.iter().position(|tab| tab.id == tab_id) else {
                    return;
                };
                this.tabs[index].paste_pending = false;
                let writable = this.active_tab == index
                    && this.can_edit_active_rows()
                    && this
                        .tab_session(index)
                        .driver
                        .as_ref()
                        .is_some_and(|current| {
                            driver
                                .upgrade()
                                .is_some_and(|original| Arc::ptr_eq(current, &original))
                        });
                let tab = &mut this.tabs[index];
                if !writable
                    || tab.view_mode != DataViewMode::Grid
                    || tab.schema_stale
                    || tab.grid.editing_cell.is_some()
                    || tab.grid.changeset_revision() != revision
                    || tab.grid.selection.focus != Some(start)
                    || tab.grid.display_columns() != layout
                    || !tab
                        .grid
                        .pager
                        .as_ref()
                        .is_some_and(|active| Arc::ptr_eq(active, &pager))
                {
                    tab.last_error = Some("Paste cancelled because the grid changed".into());
                } else {
                    match result {
                        Ok(cells) => {
                            apply_paste(&mut tab.grid, cells);
                            tab.last_error = None;
                        }
                        Err(error) => tab.last_error = Some(error),
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// The lookup already read these values from a real referenced row. Validate the
/// destination and capture the complete original key before staging any cell.
pub(super) fn stage_reference_values(
    grid: &mut VirtualDataGrid,
    row: u64,
    values: Vec<(usize, String)>,
) -> Result<(), String> {
    let pager = grid.pager.as_ref().ok_or("Source result is unavailable")?;
    if row >= grid.row_count() || values.is_empty() || values.len() > 32 {
        return Err("Source row or key is unavailable".into());
    }
    let row = grid.changeset.resolve_row(row);
    let fetched = match row {
        RowRef::Fetched(row) => {
            if grid.changeset.is_deleted(row) {
                return Err("Source row is marked for deletion".into());
            }
            let (page, local) = pager.locate_row(row).ok_or("Source row is unavailable")?;
            Some((
                pager.get_page(page).map_err(|error| error.to_string())?,
                local,
            ))
        }
        RowRef::New(_) => None,
    };
    let cells = values
        .into_iter()
        .map(|(column, value)| {
            let metadata = pager
                .columns
                .get(column)
                .ok_or("Source key column is unavailable")?;
            if !valid_text(metadata.data_type, &value) || value.contains('\0') {
                return Err(format!(
                    "Lookup value is incompatible with {}",
                    metadata.name
                ));
            }
            let original = fetched
                .as_ref()
                .and_then(|(page, local)| page.get_cell(*local, column));
            if original.is_some_and(|bytes| bytes.len() > 8192) {
                return Err("Original key value exceeds 8 KiB".into());
            }
            Ok(PasteCell {
                row,
                column,
                value: normalized_text(metadata.data_type, &value).into(),
                original: original.map(<[u8]>::to_vec),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    apply_paste(grid, cells);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{PasteSource, apply_paste, prepare_paste};
    use db_wire::{ColumnMeta, DataType, QueryResultPager};
    use std::sync::Arc;
    use ui_grid::{GridCoord, VirtualDataGrid};

    #[test]
    fn paste_validates_entire_block_and_undoes_as_one_edit() {
        let pager = Arc::new(
            QueryResultPager::new(vec![
                ColumnMeta::new("id", DataType::Int2),
                ColumnMeta::new("label", DataType::Text),
            ])
            .unwrap(),
        );
        pager.append_row([Some("1"), Some("old")]).unwrap();
        pager.append_row([Some("2"), Some("other")]).unwrap();
        let mut grid = VirtualDataGrid::new(Some(pager), ui_core::theme::Theme::dark());
        grid.move_column(1, false);
        let start = GridCoord { row: 0, col: 1 };
        let source = PasteSource::capture(&grid, start).unwrap();
        assert!(prepare_paste(&source, "new\t3\n\toverflow").is_err());
        assert!(!grid.changeset.is_dirty());
        let cells = prepare_paste(&source, "NULL\t3\r\n\t4\r\n").unwrap();
        apply_paste(&mut grid, cells);
        assert_eq!(grid.cell_value(0, 1).as_deref(), Some("NULL"));
        assert_eq!(grid.cell_value(1, 1).as_deref(), Some(""));
        assert_eq!(grid.cell_value(1, 0).as_deref(), Some("4"));
        assert!(grid.step_staged_edits(false));
        assert_eq!(grid.cell_value(1, 0).as_deref(), Some("2"));
        assert!(!grid.changeset.is_dirty());
    }

    #[test]
    fn composite_lookup_stages_every_key_or_none() {
        let pager = Arc::new(
            QueryResultPager::new(vec![
                ColumnMeta::new("tenant", DataType::Int4),
                ColumnMeta::new("customer", DataType::Int4),
            ])
            .unwrap(),
        );
        pager.append_row([Some("1"), Some("2")]).unwrap();
        let mut grid = VirtualDataGrid::new(Some(pager), ui_core::theme::Theme::dark());
        assert!(
            super::stage_reference_values(&mut grid, 0, vec![(0, "4".into()), (1, "bad".into())])
                .is_err()
        );
        assert!(!grid.changeset.is_dirty());
        super::stage_reference_values(&mut grid, 0, vec![(0, "4".into()), (1, "8".into())])
            .unwrap();
        assert_eq!(grid.cell_value(0, 0).as_deref(), Some("4"));
        assert_eq!(grid.cell_value(0, 1).as_deref(), Some("8"));
        assert!(grid.step_staged_edits(false));
        assert!(!grid.changeset.is_dirty());
    }
}
