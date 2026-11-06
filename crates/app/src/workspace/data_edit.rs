//! DBeaver-style editing of a table's rows in the result grid: cells are edited in place,
//! rows are added, duplicated or marked for deletion, and everything stays staged in the
//! grid's changeset until Save writes it in one transaction or Cancel drops it.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use ui_grid::{CellEdit, CellEditorRenderer, RowRef};

use super::*;
use db_wire::sql::{RowChanges, SqlDialect};

const MAX_DUPLICATE_CELLS: u64 = 1024;

fn validate_duplicate_range(first: u64, last: u64, columns: usize) -> Result<(), String> {
    let rows = last
        .checked_sub(first)
        .and_then(|span| span.checked_add(1))
        .ok_or("The selected row range is unavailable")?;
    if rows.saturating_mul(columns as u64) > MAX_DUPLICATE_CELLS {
        return Err("Duplicate supports up to 1,024 cells at a time".into());
    }
    Ok(())
}

fn duplicate_cell(
    grid: &VirtualDataGrid,
    row: u64,
    column: usize,
    provider: DbDriverKind,
) -> Result<Option<String>, String> {
    let pager = grid.pager.clone().ok_or("No loaded result")?;
    let row = grid.changeset.resolve_row(row);
    let staged = match row {
        RowRef::Fetched(row) => grid.changeset.edited_value(row, column),
        RowRef::New(index) => Some(grid.changeset.inserted_rows[index].values[column].as_deref()),
    };
    if staged
        .flatten()
        .is_some_and(|value| value.len() > super::value_files::MAX_VALUE_BYTES * 2 + 2)
    {
        return Err("Value exceeds the duplication size limit".into());
    }
    super::value_editor::read_value(super::value_editor::ValueRead {
        pager,
        row,
        column,
        staged: staged.map(|value| value.map(str::to_owned)),
        provider,
    })
    .map(|(value, _)| value)
}

impl Workspace {
    /// Whether the active tab shows a table whose rows can be edited
    pub(super) fn can_edit_active_rows(&self) -> bool {
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return false;
        };
        !self.operations.is_running()
            && (tab.table_name.is_some() || self.editable_query_result(self.active_tab))
            && tab.reference_picker.is_none()
            && tab.grid.pager.is_some()
            && self
                .tab_session(self.active_tab)
                .driver
                .as_ref()
                .is_some_and(|driver| !driver.is_read_only())
    }

    /// Whether the active tab has staged edits waiting for Save or Cancel
    pub(super) fn has_pending_edits(&self) -> bool {
        self.tabs
            .get(self.active_tab)
            .is_some_and(|t| t.grid.changeset.is_dirty() || t.grid.editing_cell.is_some())
    }

    /// Opens the in-place editor on a cell (double-click or the "Edit cell" button)
    pub(super) fn begin_cell_edit(&mut self, row: u64, col: usize) {
        self.commit_cell_edit();
        if !self.can_edit_active_rows() {
            return;
        }
        let tab = &mut self.tabs[self.active_tab];
        let is_deleted = matches!(tab.grid.changeset.resolve_row(row), RowRef::Fetched(f) if tab.grid.changeset.is_deleted(f));
        if is_deleted || row >= tab.grid.row_count() || !tab.grid.display_columns().contains(&col) {
            return;
        }
        if tab.is_script()
            && !tab
                .grid
                .pager
                .as_ref()
                .and_then(|pager| pager.query_edit_origin.as_ref())
                .and_then(|origin| origin.columns.get(col))
                .is_some_and(|column| column.writable && !column.key)
        {
            tab.last_error = Some("This query result column is read-only".into());
            return;
        }
        if tab
            .grid
            .pager
            .as_ref()
            .and_then(|pager| pager.columns.get(col))
            .is_some_and(|column| column.data_type == DataType::Bytea)
        {
            tab.last_error =
                Some("Use Value editor to edit binary bytes without text conversion".into());
            return;
        }
        let text = tab.grid.cell_value(row, col).unwrap_or_default();
        tab.grid.selection = ui_grid::GridSelection::single(row, col);
        tab.grid.editing_cell = Some(CellEdit {
            row,
            col,
            text,
            caret: usize::MAX,
            anchor: Some(0),
        });
        self.input.cell_edit_history.clear();
        self.input.is_filter_focused = false;
        self.blur_fetch_size();
        self.input.cursor_visible = true;
    }

    /// Opens the editor on the focused cell
    pub(super) fn edit_focused_cell(&mut self) {
        let focus = self
            .tabs
            .get(self.active_tab)
            .and_then(|t| t.grid.selection.focus);
        if let Some(f) = focus {
            self.begin_cell_edit(f.row, f.col);
        }
    }

    /// Closes the in-place editor, staging its text in the changeset as one undo step
    pub(super) fn commit_cell_edit(&mut self) {
        if self.operations.is_running() {
            return;
        }
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        let Some(edit) = tab.grid.editing_cell.take() else {
            return;
        };
        let Some(pager) = tab.grid.pager.clone() else {
            return;
        };
        let unchanged_text =
            tab.grid.cell_value(edit.row, edit.col).as_deref() == Some(edit.text.as_str());
        tab.grid
            .stage(|changeset| match changeset.resolve_row(edit.row) {
                RowRef::New(i) => {
                    if let Some(row) = changeset.inserted_rows.get_mut(i)
                        && let Some(cell) = row.values.get_mut(edit.col)
                    {
                        row.explicit_nulls.remove(&edit.col);
                        *cell = Some(edit.text);
                    }
                }
                RowRef::Fetched(row) => {
                    let original = pager.get_cell(row, edit.col);
                    let unchanged = original
                        .as_deref()
                        .map(|b| String::from_utf8_lossy(b) == edit.text.as_str())
                        .unwrap_or(false);
                    if unchanged {
                        changeset.revert_cell(row, edit.col);
                    } else if !unchanged_text {
                        changeset.set_cell(row, edit.col, original, Some(edit.text));
                    }
                }
            });
    }

    pub(super) fn set_grid_empty_value(
        &mut self,
        focus: ui_grid::GridCoord,
        default: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit_active_rows() {
            return;
        }
        let tab = &mut self.tabs[self.active_tab];
        let Some(pager) = tab.grid.pager.clone() else {
            return;
        };
        let Some(column) = pager.columns.get(focus.col) else {
            return;
        };
        if tab.is_script()
            && !pager
                .query_edit_origin
                .as_ref()
                .and_then(|origin| origin.columns.get(focus.col))
                .is_some_and(|column| column.writable && !column.key)
        {
            return;
        }
        if tab.schema_stale || tab.grid.editing_cell.is_some() || focus.row >= tab.grid.row_count()
        {
            return;
        }
        if !default && !column.nullable {
            tab.last_error = Some(format!("Column {} does not accept SQL NULL", column.name));
            cx.notify();
            return;
        }
        let row = tab.grid.changeset.resolve_row(focus.row);
        match row {
            RowRef::New(index) => tab.grid.stage(|changes| {
                let row = &mut changes.inserted_rows[index];
                row.values[focus.col] = None;
                if default {
                    row.explicit_nulls.remove(&focus.col);
                } else {
                    row.explicit_nulls.insert(focus.col);
                }
            }),
            RowRef::Fetched(row) if !default && !tab.grid.changeset.is_deleted(row) => {
                let Some((page, local_row)) = pager.locate_row(row) else {
                    return;
                };
                let page = match pager.get_page(page) {
                    Ok(page) => page,
                    Err(error) => {
                        tab.last_error = Some(format!("Cannot read original cell: {error}"));
                        cx.notify();
                        return;
                    }
                };
                let original = page.get_cell(local_row, focus.col).map(<[u8]>::to_vec);
                tab.grid.stage(|changes| {
                    if original.is_some() {
                        changes.set_cell(row, focus.col, original, None);
                    } else {
                        changes.revert_cell(row, focus.col);
                    }
                });
            }
            _ => return,
        }
        tab.last_error = None;
        cx.notify();
    }

    /// Closes the in-place editor without keeping what was typed
    pub(super) fn cancel_cell_edit(&mut self) {
        if self.operations.is_running() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.grid.editing_cell = None;
        }
    }

    /// Adds an empty row below the focused row, or at the end without a selection,
    /// and opens the editor on it
    pub(super) fn add_row(&mut self) {
        if self
            .tabs
            .get(self.active_tab)
            .is_some_and(TabState::is_script)
        {
            return;
        }

        if !self.can_edit_active_rows() {
            return;
        }
        self.commit_cell_edit();
        let tab = &mut self.tabs[self.active_tab];
        let num_cols = tab
            .grid
            .pager
            .as_ref()
            .map(|p| p.num_cols() as usize)
            .unwrap_or(0);
        let Some(first_column) = tab.grid.display_columns().first().copied() else {
            return;
        };
        let row_count = tab.grid.row_count();
        let row = tab
            .grid
            .selection
            .focus
            .map(|f| (f.row + 1).min(row_count))
            .unwrap_or(row_count);
        tab.grid
            .stage(|changeset| changeset.insert_row_at(row, vec![None; num_cols]));
        tab.grid
            .vertical_scroll_handle
            .scroll_to_item(row as usize, ScrollStrategy::Bottom);
        let col = tab
            .grid
            .selection
            .focus
            .map(|f| f.col)
            .filter(|col| tab.grid.display_columns().contains(col))
            .unwrap_or(first_column);
        self.begin_cell_edit(row, col);
    }

    /// Adds a copy of every selected row right below the selection
    pub(super) fn duplicate_selected_rows(&mut self) {
        if self
            .tabs
            .get(self.active_tab)
            .is_some_and(TabState::is_script)
        {
            return;
        }

        if !self.can_edit_active_rows() {
            return;
        }
        self.commit_cell_edit();
        let provider = self.tab_provider(self.active_tab);
        let tab = &mut self.tabs[self.active_tab];
        let Some((min_r, max_r, _, _)) = tab.grid.selection.bounding_box() else {
            return;
        };
        let num_cols = tab
            .grid
            .pager
            .as_ref()
            .map(|p| p.num_cols() as usize)
            .unwrap_or(0);
        let max_r = max_r.min(tab.grid.row_count().saturating_sub(1));
        let (Some(&first_column), Some(&last_column)) = (
            tab.grid.display_columns().first(),
            tab.grid.display_columns().last(),
        ) else {
            return;
        };
        if let Err(error) = validate_duplicate_range(min_r, max_r, num_cols) {
            tab.last_error = Some(error);
            return;
        }
        let mut bytes = 0usize;
        let copies: Result<Vec<_>, String> = (min_r..=max_r)
            .map(|row| {
                let values = (0..num_cols)
                    .map(|column| {
                        let value = duplicate_cell(&tab.grid, row, column, provider)?;
                        bytes += value.as_ref().map_or(0, String::len);
                        if bytes > super::value_files::MAX_VALUE_BYTES {
                            return Err("Duplicate values exceed the 256 KiB budget".into());
                        }
                        Ok(value)
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let nulls = match tab.grid.changeset.resolve_row(row) {
                    RowRef::New(index) => tab.grid.changeset.inserted_rows[index]
                        .explicit_nulls
                        .clone(),
                    RowRef::Fetched(_) => values
                        .iter()
                        .enumerate()
                        .filter_map(|(column, value)| value.is_none().then_some(column))
                        .collect(),
                };
                Ok((values, nulls))
            })
            .collect();
        let copies = match copies {
            Ok(copies) => copies,
            Err(error) => {
                tab.last_error = Some(error);
                return;
            }
        };
        if copies.is_empty() {
            return;
        }
        let first_new = max_r + 1;
        let added = copies.len() as u64;
        tab.grid.stage(|changeset| {
            for (i, (values, nulls)) in copies.into_iter().enumerate() {
                let position = first_new + i as u64;
                changeset.insert_row_at(position, values);
                if let RowRef::New(index) = changeset.resolve_row(position) {
                    changeset.inserted_rows[index].explicit_nulls = nulls;
                }
            }
        });
        tab.grid.selection.set_range(
            ui_grid::GridCoord {
                row: first_new,
                col: first_column,
            },
            ui_grid::GridCoord {
                row: first_new + added - 1,
                col: last_column,
            },
        );
        tab.grid
            .vertical_scroll_handle
            .scroll_to_item((first_new + added - 1) as usize, ScrollStrategy::Bottom);
    }

    /// Marks every selected fetched row for deletion; selected new rows are dropped at once
    pub(super) fn delete_selected_rows(&mut self) {
        if self
            .tabs
            .get(self.active_tab)
            .is_some_and(TabState::is_script)
        {
            return;
        }

        if !self.can_edit_active_rows() {
            return;
        }
        self.cancel_cell_edit();
        let tab = &mut self.tabs[self.active_tab];
        let Some((min_r, max_r, _, _)) = tab.grid.selection.bounding_box() else {
            return;
        };
        let max_r = max_r.min(tab.grid.row_count().saturating_sub(1));
        let dropped_new_rows = tab.grid.stage(|changeset| {
            let mut dropped_new_rows = false;
            // Bottom up, so dropping a new row does not move the rows still to visit
            for r in (min_r..=max_r).rev() {
                match changeset.resolve_row(r) {
                    RowRef::Fetched(f) => {
                        changeset.deleted_rows.insert(f);
                    }
                    RowRef::New(i) => {
                        changeset.remove_inserted_row(i);
                        dropped_new_rows = true;
                    }
                }
            }
            dropped_new_rows
        });
        if dropped_new_rows {
            tab.grid.selection.clear();
        }
    }

    /// Drops every staged edit of the active tab (the Cancel button); Cmd+Z brings them back
    pub(super) fn discard_pending_edits(&mut self) {
        if self.operations.is_running() {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.grid.editing_cell = None;
            let had_rows = !tab.grid.changeset.inserted_rows.is_empty();
            tab.grid.stage(|changeset| changeset.clear());
            if had_rows {
                tab.grid.selection.clear();
            }
        }
    }

    /// Undoes (`redo == false`) or redoes the last staged change of the active table:
    /// a cell edit, added, duplicated or deleted rows, or Cancel
    pub(super) fn step_pending_edits(&mut self, redo: bool) {
        if self.operations.is_running() {
            return;
        }
        self.commit_cell_edit();
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        tab.grid.step_staged_edits(redo);
    }

    pub(super) fn table_edit_statements(&self, index: usize) -> Result<Vec<String>, String> {
        let tab = self.tabs.get(index).ok_or("Table tab is unavailable")?;
        if !tab.grid.changeset.is_dirty() {
            return Ok(Vec::new());
        }
        let (Some(schema), Some(table), Some(pager)) = (
            tab.schema_name.clone(),
            tab.table_name.clone(),
            tab.grid.pager.clone(),
        ) else {
            return Err("Table result is unavailable".into());
        };

        let dialect = SqlDialect(self.tab_provider(index));
        let target = dialect.qualified_table(&schema, &table);
        // Rows are found by their primary key when the result has every key column
        let pk_names: Vec<&str> = self
            .tab_session(index)
            .table_columns
            .get(&(schema.clone(), table.clone()))
            .map(|cols| {
                cols.iter()
                    .filter(|c| c.is_primary_key)
                    .map(|c| c.name.as_str())
                    .collect()
            })
            .unwrap_or_default();
        let key_cols: Vec<usize> = pk_names
            .iter()
            .filter_map(|name| pager.columns.iter().position(|c| c.name == *name))
            .collect();
        let key_cols = if key_cols.len() == pk_names.len() {
            key_cols
        } else {
            Vec::new()
        };

        let changes = &tab.grid.changeset;
        let request = RowChanges {
            changes: changes
                .changes
                .iter()
                .map(|(key, cell)| ((key.row_idx, key.col_idx), cell.new_value.clone()))
                .collect(),
            inserted_rows: changes
                .inserted_rows
                .iter()
                .map(|row| row.values.clone())
                .collect(),
            inserted_nulls: changes
                .inserted_rows
                .iter()
                .enumerate()
                .flat_map(|(row, values)| {
                    values
                        .explicit_nulls
                        .iter()
                        .map(move |&column| (row, column))
                })
                .collect(),
            deleted_rows: changes.deleted_rows.clone(),
        };
        request
            .plan_sql(&target, &pager, &key_cols, dialect)
            .map_err(|error| format!("{error:#}"))
    }

    /// Writes the staged edits of the active tab to the database in one transaction,
    /// then reloads the rows (Save button / Cmd+S)
    pub(super) fn save_pending_edits(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab)
            && tab.schema_stale
        {
            tab.last_error =
                Some("Schema changed; cancel staged row edits and reload before saving".into());
            cx.notify();
            return;
        }
        self.commit_cell_edit();
        if self
            .tabs
            .get(self.active_tab)
            .is_some_and(TabState::is_script)
        {
            self.save_query_result_edits(cx);
            return;
        }
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };
        if !tab.grid.changeset.is_dirty() {
            return;
        }
        let Some(driver) = self.tab_session(self.active_tab).driver.clone() else {
            return;
        };
        if driver.supports_script_session() {
            let boundary = if self.tabs[self.active_tab].manual_transactions {
                super::table_session::Boundary::Save
            } else {
                super::table_session::Boundary::Commit
            };
            self.apply_table_transaction(boundary, cx);
            return;
        }
        if self.operations.is_running() {
            cx.notify();
            return;
        }
        let statements = match self.table_edit_statements(self.active_tab) {
            Ok(statements) => statements,
            Err(error) => {
                self.tabs[self.active_tab].last_error = Some(error);
                cx.notify();
                return;
            }
        };
        let tab_id = tab.id;
        let connection_id = tab
            .connection_id
            .clone()
            .expect("table tab owns a connection");
        let saved_changes = tab.grid.changeset.clone();
        let Some(operation_id) =
            self.operations
                .begin(tab_id, connection_id, driver.clone(), OperationKind::Save)
        else {
            return;
        };
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = db_wire::run_on_shared_runtime(async move {
                driver.apply_row_changes(&statements).await
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                if !this.operations.finish(operation_id) {
                    return;
                }
                let Some(idx) = this.tabs.iter().position(|t| t.id == tab_id) else {
                    return;
                };
                let saved = this.tabs[idx]
                    .finish_row_save(&saved_changes, result.map_err(|error| format!("{error:#}")));
                if saved && idx == this.active_tab {
                    this.run_active_query(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Save / Cancel and the row editing buttons at the start of the result footer
    pub(super) fn render_edit_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = &self.theme;
        let can_edit = self.can_edit_active_rows();
        let can_change_rows = can_edit
            && self
                .tabs
                .get(self.active_tab)
                .is_some_and(|tab| !tab.is_script());
        let has_selection = self
            .tabs
            .get(self.active_tab)
            .is_some_and(|t| t.grid.selection.focus.is_some());
        let dirty = self.has_pending_edits() && !self.operations.is_running();

        let button =
            |id: &'static str, icon: &'static str, label: Option<&'static str>, enabled: bool| {
                let el = div()
                    .id(id)
                    .flex()
                    .items_center()
                    .gap_1()
                    .h(px(20.0))
                    .px_1()
                    .rounded_xs()
                    .text_size(px(11.0))
                    .text_color(if enabled {
                        theme.text_primary
                    } else {
                        theme.text_muted
                    })
                    .child(img(icon).w(px(14.0)).h(px(14.0)))
                    .children(label);
                if enabled {
                    el.cursor_pointer().hover(|s| s.bg(theme.bg_surface_hover))
                } else {
                    el.opacity(0.45)
                }
            };
        let separator = || div().w(px(1.0)).h(px(14.0)).bg(theme.border);

        div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                button("btn-save-edits", "icons/save.svg", Some("Save"), dirty).when(dirty, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| {
                        this.save_pending_edits(cx);
                        cx.notify();
                    }))
                }),
            )
            .child(
                button(
                    "btn-cancel-edits",
                    "icons/cancel.svg",
                    Some("Cancel"),
                    dirty,
                )
                .when(dirty, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| {
                        this.discard_pending_edits();
                        cx.notify();
                    }))
                }),
            )
            .child(separator())
            .child(
                button(
                    "btn-edit-cell",
                    "icons/row_edit.svg",
                    None,
                    can_edit && has_selection,
                )
                .when(can_edit && has_selection, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| {
                        this.edit_focused_cell();
                        cx.notify();
                    }))
                }),
            )
            .child(
                button("btn-add-row", "icons/row_add.svg", None, can_change_rows).when(
                    can_change_rows,
                    |b| {
                        b.on_click(cx.listener(|this, _, _, cx| {
                            this.add_row();
                            cx.notify();
                        }))
                    },
                ),
            )
            .child(
                button(
                    "btn-duplicate-row",
                    "icons/row_copy.svg",
                    None,
                    can_change_rows && has_selection,
                )
                .when(can_change_rows && has_selection, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| {
                        this.duplicate_selected_rows();
                        cx.notify();
                    }))
                }),
            )
            .child(
                button(
                    "btn-delete-row",
                    "icons/row_delete.svg",
                    None,
                    can_change_rows && has_selection,
                )
                .when(can_change_rows && has_selection, |b| {
                    b.on_click(cx.listener(|this, _, _, cx| {
                        this.delete_selected_rows();
                        cx.notify();
                    }))
                }),
            )
            .child(separator())
    }

    /// The editor drawn inside the cell being edited
    pub(super) fn cell_editor_renderer(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<(u64, usize, CellEditorRenderer)> {
        let edit = self.tabs.get(self.active_tab)?.grid.editing_cell.clone()?;
        let caret_visible = self.input.cursor_visible;
        let workspace = cx.entity().downgrade();
        let pick: CaretPick = Rc::new(move |index, kind, _, cx| {
            let _ = workspace.update(cx, |this, cx| {
                if let Some(e) = this
                    .tabs
                    .get_mut(this.active_tab)
                    .and_then(|t| t.grid.editing_cell.as_mut())
                {
                    apply_pick(&mut e.caret, &mut e.anchor, index, kind, true);
                }
                this.input.cursor_visible = true;
                cx.notify();
            });
        });
        let (row, col) = (edit.row, edit.col);
        let render: CellEditorRenderer = Rc::new(move || {
            let len = edit.text.chars().count();
            let selection = selection_range(edit.anchor, edit.caret, len);
            ui_editor::line_input::pick_surface(
                div().flex().items_center().overflow_hidden(),
                usize::MAX,
                &pick,
            )
            .children(render_caret_text(
                &edit.text,
                Some(edit.caret.min(len)),
                caret_visible,
                selection,
                &pick,
            ))
            .into_any_element()
        });
        Some((row, col, render))
    }
}

#[cfg(test)]
mod value_tests {
    use super::{duplicate_cell, validate_duplicate_range};
    use db_wire::{DatabaseDriver, DbDriverKind, SqliteDriver};
    use ui_core::theme::Theme;
    use ui_grid::VirtualDataGrid;

    #[test]
    fn duplication_rejects_stale_overflowing_or_oversized_selections() {
        assert!(validate_duplicate_range(9, 8, 1).is_err());
        assert!(validate_duplicate_range(0, u64::MAX, 1).is_err());
        assert!(validate_duplicate_range(0, 1024, 1).is_err());
        assert!(validate_duplicate_range(0, 0, 1025).is_err());
        assert!(validate_duplicate_range(0, 1023, 1).is_ok());
        assert!(validate_duplicate_range(12, 12, 2).is_ok());
    }

    #[test]
    fn duplicate_then_save_preserves_raw_binary_prefix_and_non_utf8() {
        db_wire::get_runtime().block_on(async {
            let driver = SqliteDriver::new_memory();
            driver
                .execute_query("CREATE TABLE blobs (value BLOB)")
                .await
                .unwrap();
            driver
                .execute_query(
                    "INSERT INTO blobs VALUES (X'5c7864656164'), (X'00ff'), (NULL), (X'')",
                )
                .await
                .unwrap();
            let (pager, _, _) = driver
                .execute_query("SELECT value FROM blobs")
                .await
                .unwrap();
            let grid = VirtualDataGrid::new(Some(pager), Theme::dark());
            let mut changes = db_wire::sql::RowChanges::default();
            for row in 0..4 {
                let value = duplicate_cell(&grid, row, 0, DbDriverKind::Sqlite).unwrap();
                if value.is_none() {
                    changes.inserted_nulls.insert((row as usize, 0));
                }
                changes.inserted_rows.push(vec![value]);
            }
            let sql = changes
                .plan_sql(
                    "blobs",
                    grid.pager.as_ref().unwrap(),
                    &[],
                    db_wire::sql::SqlDialect(DbDriverKind::Sqlite),
                )
                .unwrap();
            driver.apply_row_changes(&sql).await.unwrap();
            let (stored, _, _) = driver
                .execute_query("SELECT value FROM blobs")
                .await
                .unwrap();
            assert_eq!(stored.total_rows(), 8);
            for row in 0..4 {
                assert_eq!(stored.get_cell(row, 0), stored.get_cell(row + 4, 0));
            }
        });
    }
}
