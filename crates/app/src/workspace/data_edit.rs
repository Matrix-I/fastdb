//! DBeaver-style editing of a table's rows in the result grid: cells are edited in place,
//! rows are added, duplicated or marked for deletion, and everything stays staged in the
//! grid's changeset until Save writes it in one transaction or Cancel drops it.

use std::rc::Rc;

use gpui::prelude::FluentBuilder;
use ui_grid::{CellEdit, CellEditorRenderer, RowRef, SqlDialect};

use super::*;

impl Workspace {
    /// Whether the active tab shows a table whose rows can be edited
    pub(super) fn can_edit_active_rows(&self) -> bool {
        let Some(tab) = self.tabs.get(self.active_tab) else { return false };
        tab.table_name.is_some() && tab.grid.pager.is_some() && self.driver.is_some()
    }

    /// Whether the active tab has staged edits waiting for Save or Cancel
    pub(super) fn has_pending_edits(&self) -> bool {
        self.tabs.get(self.active_tab).is_some_and(|t| t.grid.changeset.is_dirty() || t.grid.editing_cell.is_some())
    }

    /// Opens the in-place editor on a cell (double-click or the "Edit cell" button)
    pub(super) fn begin_cell_edit(&mut self, row: u64, col: usize) {
        self.commit_cell_edit();
        if !self.can_edit_active_rows() {
            self.status_message = "Only rows of a table opened from the navigator can be edited.".to_string();
            return;
        }
        let tab = &mut self.tabs[self.active_tab];
        let is_deleted = matches!(tab.grid.changeset.resolve_row(row), RowRef::Fetched(f) if tab.grid.changeset.is_deleted(f));
        if is_deleted || row >= tab.grid.row_count() {
            return;
        }
        let text = tab.grid.cell_value(row, col).unwrap_or_default();
        tab.grid.selection = ui_grid::GridSelection::single(row, col);
        tab.grid.editing_cell = Some(CellEdit { row, col, text, caret: usize::MAX, selected_all: true });
        self.cell_edit_history.clear();
        self.is_filter_focused = false;
        self.blur_fetch_size();
        self.cursor_visible = true;
    }

    /// Opens the editor on the focused cell
    pub(super) fn edit_focused_cell(&mut self) {
        let focus = self.tabs.get(self.active_tab).and_then(|t| t.grid.selection.focus);
        match focus {
            Some(f) => self.begin_cell_edit(f.row, f.col),
            None => self.status_message = "Select a cell to edit.".to_string(),
        }
    }

    /// Closes the in-place editor, staging its text in the changeset as one undo step
    pub(super) fn commit_cell_edit(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else { return };
        let Some(edit) = tab.grid.editing_cell.take() else { return };
        let Some(pager) = tab.grid.pager.clone() else { return };
        let unchanged_text = tab.grid.cell_value(edit.row, edit.col).as_deref() == Some(edit.text.as_str());
        tab.grid.stage(|changeset| match changeset.resolve_row(edit.row) {
            RowRef::New(i) => {
                if let Some(cell) = changeset.inserted_rows.get_mut(i).and_then(|r| r.values.get_mut(edit.col)) {
                    *cell = Some(edit.text);
                }
            }
            RowRef::Fetched(row) => {
                let original = pager.get_cell(row, edit.col);
                let unchanged = original.as_deref().map(|b| String::from_utf8_lossy(b) == edit.text.as_str()).unwrap_or(false);
                if unchanged {
                    changeset.revert_cell(row, edit.col);
                } else if !unchanged_text {
                    changeset.set_cell(row, edit.col, original, Some(edit.text));
                }
            }
        });
    }

    /// Closes the in-place editor without keeping what was typed
    pub(super) fn cancel_cell_edit(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.grid.editing_cell = None;
        }
    }

    /// Adds an empty row below the focused row, or at the end without a selection,
    /// and opens the editor on it
    pub(super) fn add_row(&mut self) {
        if !self.can_edit_active_rows() {
            return;
        }
        self.commit_cell_edit();
        let tab = &mut self.tabs[self.active_tab];
        let num_cols = tab.grid.pager.as_ref().map(|p| p.num_cols() as usize).unwrap_or(0);
        let row_count = tab.grid.row_count();
        let row = tab.grid.selection.focus.map(|f| (f.row + 1).min(row_count)).unwrap_or(row_count);
        tab.grid.stage(|changeset| changeset.insert_row_at(row, vec![None; num_cols]));
        tab.grid.vertical_scroll_handle.scroll_to_item(row as usize, ScrollStrategy::Bottom);
        let col = tab.grid.selection.focus.map(|f| f.col).unwrap_or(0).min(num_cols.saturating_sub(1));
        self.begin_cell_edit(row, col);
    }

    /// Adds a copy of every selected row right below the selection
    pub(super) fn duplicate_selected_rows(&mut self) {
        if !self.can_edit_active_rows() {
            return;
        }
        self.commit_cell_edit();
        let tab = &mut self.tabs[self.active_tab];
        let Some((min_r, max_r, _, _)) = tab.grid.selection.bounding_box() else {
            self.status_message = "Select a row to duplicate.".to_string();
            return;
        };
        let num_cols = tab.grid.pager.as_ref().map(|p| p.num_cols() as usize).unwrap_or(0);
        let max_r = max_r.min(tab.grid.row_count().saturating_sub(1));
        let copies: Vec<Vec<Option<String>>> = (min_r..=max_r)
            .map(|r| (0..num_cols).map(|c| tab.grid.cell_value(r, c)).collect())
            .collect();
        let first_new = max_r + 1;
        let added = copies.len() as u64;
        tab.grid.stage(|changeset| {
            for (i, values) in copies.into_iter().enumerate() {
                changeset.insert_row_at(first_new + i as u64, values);
            }
        });
        tab.grid.selection.set_range(
            ui_grid::GridCoord { row: first_new, col: 0 },
            ui_grid::GridCoord { row: first_new + added - 1, col: num_cols.saturating_sub(1) },
        );
        tab.grid.vertical_scroll_handle.scroll_to_item((first_new + added - 1) as usize, ScrollStrategy::Bottom);
        self.status_message = format!("Duplicated {} row(s) — Save to write them to the table.", added);
    }

    /// Marks every selected fetched row for deletion; selected new rows are dropped at once
    pub(super) fn delete_selected_rows(&mut self) {
        if !self.can_edit_active_rows() {
            return;
        }
        self.cancel_cell_edit();
        let tab = &mut self.tabs[self.active_tab];
        let Some((min_r, max_r, _, _)) = tab.grid.selection.bounding_box() else {
            self.status_message = "Select a row to delete.".to_string();
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
        self.status_message = "Rows marked for deletion — Save to delete them from the table.".to_string();
    }

    /// Drops every staged edit of the active tab (the Cancel button); Cmd+Z brings them back
    pub(super) fn discard_pending_edits(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.grid.editing_cell = None;
            let had_rows = !tab.grid.changeset.inserted_rows.is_empty();
            tab.grid.stage(|changeset| changeset.clear());
            if had_rows {
                tab.grid.selection.clear();
            }
            self.status_message = "Changes cancelled.".to_string();
        }
    }

    /// Undoes (`redo == false`) or redoes the last staged change of the active table:
    /// a cell edit, added, duplicated or deleted rows, or Cancel
    pub(super) fn step_pending_edits(&mut self, redo: bool) {
        self.commit_cell_edit();
        let Some(tab) = self.tabs.get_mut(self.active_tab) else { return };
        if tab.grid.step_staged_edits(redo) {
            self.status_message = format!("{} — {} pending change(s).", if redo { "Redo" } else { "Undo" }, tab.grid.changeset.count());
        }
    }

    /// Writes the staged edits of the active tab to the database in one transaction,
    /// then reloads the rows (Save button / Cmd+S)
    pub(super) fn save_pending_edits(&mut self, cx: &mut Context<Self>) {
        self.commit_cell_edit();
        let Some(driver) = self.driver.clone() else { return };
        let Some(tab) = self.tabs.get(self.active_tab) else { return };
        if !tab.grid.changeset.is_dirty() {
            return;
        }
        if self.is_running_query {
            self.status_message = "⏳ A query is still running — wait for it before saving.".to_string();
            cx.notify();
            return;
        }
        let (Some(schema), Some(table), Some(pager)) = (tab.schema_name.clone(), tab.table_name.clone(), tab.grid.pager.clone()) else {
            return;
        };

        let dialect = match self.selected_driver_kind {
            DbDriverKind::MySql => SqlDialect::MySql,
            _ => SqlDialect::Ansi,
        };
        let target = match self.selected_driver_kind {
            DbDriverKind::Sqlite => dialect.quote_ident(&table),
            _ => format!("{}.{}", dialect.quote_ident(&schema), dialect.quote_ident(&table)),
        };
        // Rows are found by their primary key when the result has every key column
        let pk_names: Vec<&str> = self
            .table_columns
            .get(&(schema.clone(), table.clone()))
            .map(|cols| cols.iter().filter(|c| c.is_primary_key).map(|c| c.name.as_str()).collect())
            .unwrap_or_default();
        let key_cols: Vec<usize> = pk_names
            .iter()
            .filter_map(|name| pager.columns.iter().position(|c| c.name == *name))
            .collect();
        let key_cols = if key_cols.len() == pk_names.len() { key_cols } else { Vec::new() };

        let statements = tab.grid.changeset.generate_sql(&target, &pager, &key_cols, dialect);
        let tab_id = tab.id;
        self.is_running_query = true;
        self.status_message = format!("Saving {} change(s)…", statements.len());
        cx.notify();

        cx.spawn(async move |this, cx| {
            let result = db_wire::run_on_shared_runtime(async move { driver.apply_row_changes(&statements).await }).await;
            let _ = this.update(cx, |this, cx| {
                this.is_running_query = false;
                let Some(idx) = this.tabs.iter().position(|t| t.id == tab_id) else { return };
                match result {
                    Ok(applied) => {
                        this.tabs[idx].grid.changeset.clear();
                        this.tabs[idx].grid.changeset_history.clear();
                        this.tabs[idx].grid.selection.clear();
                        this.status_message = format!("✅ Saved {} change(s).", applied);
                        if idx == this.active_tab {
                            this.run_active_query(cx);
                        }
                    }
                    Err(e) => {
                        this.status_message = format!("❌ Save failed, nothing was changed: {:#}", e);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Save / Cancel and the row editing buttons at the start of the result footer
    pub(super) fn render_edit_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let can_edit = self.can_edit_active_rows();
        let has_selection = self.tabs.get(self.active_tab).is_some_and(|t| t.grid.selection.focus.is_some());
        let dirty = self.has_pending_edits() && !self.is_running_query;

        let button = |id: &'static str, icon: &'static str, label: Option<&'static str>, enabled: bool| {
            let el = div()
                .id(id)
                .flex()
                .items_center()
                .gap_1()
                .h(px(20.0))
                .px_1()
                .rounded_xs()
                .text_size(px(11.0))
                .text_color(if enabled { theme.text_primary } else { theme.text_muted })
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
            .child(button("btn-save-edits", "icons/save.svg", Some("Save"), dirty).when(dirty, |b| {
                b.on_click(cx.listener(|this, _, _, cx| {
                    this.save_pending_edits(cx);
                    cx.notify();
                }))
            }))
            .child(button("btn-cancel-edits", "icons/cancel.svg", Some("Cancel"), dirty).when(dirty, |b| {
                b.on_click(cx.listener(|this, _, _, cx| {
                    this.discard_pending_edits();
                    cx.notify();
                }))
            }))
            .child(separator())
            .child(button("btn-edit-cell", "icons/row_edit.svg", None, can_edit && has_selection).when(can_edit && has_selection, |b| {
                b.on_click(cx.listener(|this, _, _, cx| {
                    this.edit_focused_cell();
                    cx.notify();
                }))
            }))
            .child(button("btn-add-row", "icons/row_add.svg", None, can_edit).when(can_edit, |b| {
                b.on_click(cx.listener(|this, _, _, cx| {
                    this.add_row();
                    cx.notify();
                }))
            }))
            .child(button("btn-duplicate-row", "icons/row_copy.svg", None, can_edit && has_selection).when(can_edit && has_selection, |b| {
                b.on_click(cx.listener(|this, _, _, cx| {
                    this.duplicate_selected_rows();
                    cx.notify();
                }))
            }))
            .child(button("btn-delete-row", "icons/row_delete.svg", None, can_edit && has_selection).when(can_edit && has_selection, |b| {
                b.on_click(cx.listener(|this, _, _, cx| {
                    this.delete_selected_rows();
                    cx.notify();
                }))
            }))
            .child(separator())
    }

    /// The editor drawn inside the cell being edited
    pub(super) fn cell_editor_renderer(&self, cx: &mut Context<Self>) -> Option<(u64, usize, CellEditorRenderer)> {
        let edit = self.tabs.get(self.active_tab)?.grid.editing_cell.clone()?;
        let caret_visible = self.cursor_visible;
        let selection_bg = self.theme.bg_grid_cell_selected;
        let workspace = cx.entity().downgrade();
        let pick: CaretPick = Rc::new(move |index, clicks, _, cx| {
            let _ = workspace.update(cx, |this, cx| {
                if let Some(e) = this.tabs.get_mut(this.active_tab).and_then(|t| t.grid.editing_cell.as_mut()) {
                    e.caret = index;
                    e.selected_all = clicks >= 2;
                }
                this.cursor_visible = true;
                cx.notify();
            });
        });
        let (row, col) = (edit.row, edit.col);
        let render: CellEditorRenderer = Rc::new(move || {
            let caret = (!edit.selected_all).then(|| edit.caret.min(edit.text.chars().count()));
            div()
                .flex()
                .items_center()
                .overflow_hidden()
                .when(edit.selected_all, |d| d.bg(selection_bg))
                .children(render_caret_text(&edit.text, caret, caret_visible, &pick))
                .into_any_element()
        });
        Some((row, col, render))
    }
}
