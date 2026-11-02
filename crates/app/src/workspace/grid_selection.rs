use super::*;
use ui_grid::GridCoord;

/// A drag belongs to the exact result where the pointer was pressed.
pub(super) struct GridDragSelection {
    tab_id: u64,
    pager: Arc<db_wire::QueryResultPager>,
    anchor: GridCoord,
}

impl GridDragSelection {
    pub(super) fn begin(tab: &TabState) -> Option<Self> {
        if tab.grid.editing_cell.is_some() {
            return None;
        }
        let pager = tab.grid.pager.clone()?;
        let anchor = tab.grid.selection.anchor?;
        if anchor.row >= tab.grid.row_count() || anchor.col >= pager.columns.len() {
            return None;
        }
        Some(Self {
            tab_id: tab.id,
            pager,
            anchor,
        })
    }

    // None ends an obsolete drag; Some(false) avoids a render for pointer movement
    // inside the same cell or outside the current result's bounds.
    fn extend(&self, tab: &mut TabState, focus: GridCoord) -> Option<bool> {
        if tab.id != self.tab_id
            || tab.grid.editing_cell.is_some()
            || !tab
                .grid
                .pager
                .as_ref()
                .is_some_and(|p| Arc::ptr_eq(p, &self.pager))
        {
            return None;
        }
        if focus.row >= tab.grid.row_count() || focus.col >= self.pager.columns.len() {
            return Some(false);
        }
        let changed = tab.grid.selection.anchor != Some(self.anchor)
            || tab.grid.selection.focus != Some(focus);
        if changed {
            tab.grid.selection.set_range(self.anchor, focus);
        }
        Some(changed)
    }
}

impl Workspace {
    pub(super) fn extend_grid_drag(&mut self, row: u64, col: usize, cx: &mut Context<Self>) {
        let Some(drag) = &self.input.grid_drag else {
            return;
        };
        if self.layout.resizing_column.is_some() || self.layout.resizing_panel.is_some() {
            self.input.grid_drag = None;
            return;
        }
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            self.input.grid_drag = None;
            return;
        };
        let previous_col = tab.grid.selection.focus.map(|f| f.col);
        match drag.extend(tab, GridCoord { row, col }) {
            None => self.input.grid_drag = None,
            Some(false) => {}
            Some(true) => {
                if previous_col != Some(col) {
                    self.value_panel.format_override = None;
                }
                self.value_panel.show_format_menu = false;
                cx.notify();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GridDragSelection;
    use crate::workspace::TabState;
    use db_wire::{ColumnMeta, QueryResultPager};
    use mem_engine::DataType;
    use sql_intel::SchemaCache;
    use std::sync::Arc;
    use ui_core::theme::Theme;
    use ui_grid::{CellEdit, GridCoord, GridSelection};

    fn tab(id: u64) -> TabState {
        let mut tab = TabState::table(
            id,
            "public",
            "items",
            Theme::dark(),
            Arc::new(SchemaCache::new().unwrap()),
        );
        let pager = Arc::new(
            QueryResultPager::new(vec![
                ColumnMeta::new("id", DataType::Int4),
                ColumnMeta::new("label", DataType::Text),
            ])
            .unwrap(),
        );
        for row in 0..4 {
            pager
                .append_row([Some(row.to_string()), Some("label".into())])
                .unwrap();
        }
        tab.grid.set_pager(pager);
        tab
    }

    #[test]
    fn drag_extends_both_directions_and_keeps_the_original_anchor() {
        let mut tab = tab(1);
        tab.grid.selection = GridSelection::single(2, 1);
        let drag = GridDragSelection::begin(&tab).unwrap();
        assert_eq!(
            drag.extend(&mut tab, GridCoord { row: 0, col: 0 }),
            Some(true)
        );
        assert_eq!(tab.grid.selection.bounding_box(), Some((0, 2, 0, 1)));
        assert_eq!(
            drag.extend(&mut tab, GridCoord { row: 3, col: 1 }),
            Some(true)
        );
        assert_eq!(tab.grid.selection.bounding_box(), Some((2, 3, 1, 1)));
        assert_eq!(
            tab.grid.selection.anchor,
            Some(GridCoord { row: 2, col: 1 })
        );
        assert_eq!(
            drag.extend(&mut tab, GridCoord { row: 3, col: 1 }),
            Some(false)
        );
        // The selected rectangle is directly usable by existing clipboard formats.
        assert_eq!(tab.grid.selection_text().as_deref(), Some("label\nlabel"));
    }

    #[test]
    fn shift_drag_preserves_the_existing_selection_anchor() {
        let mut tab = tab(1);
        tab.grid
            .selection
            .set_range(GridCoord { row: 0, col: 0 }, GridCoord { row: 2, col: 1 });
        let drag = GridDragSelection::begin(&tab).unwrap();
        drag.extend(&mut tab, GridCoord { row: 3, col: 1 });
        assert_eq!(tab.grid.selection.bounding_box(), Some((0, 3, 0, 1)));
    }

    #[test]
    fn drags_cannot_cross_tabs_or_replacement_results() {
        let mut original = tab(1);
        original.grid.selection = GridSelection::single(0, 0);
        let drag = GridDragSelection::begin(&original).unwrap();
        let mut other = tab(2);
        other.grid.pager = original.grid.pager.clone();
        assert_eq!(drag.extend(&mut other, GridCoord { row: 1, col: 1 }), None);
        assert!(other.grid.selection.anchor.is_none());
        original.grid.set_pager(tab(1).grid.pager.unwrap());
        assert_eq!(
            drag.extend(&mut original, GridCoord { row: 1, col: 1 }),
            None
        );
        assert!(original.grid.selection.anchor.is_none());
    }

    #[test]
    fn hover_out_of_bounds_and_cell_editing_do_not_change_selection() {
        let mut tab = tab(1);
        assert!(GridDragSelection::begin(&tab).is_none());
        tab.grid.selection = GridSelection::single(99, 99);
        assert!(GridDragSelection::begin(&tab).is_none());
        tab.grid.selection = GridSelection::single(0, 0);
        let drag = GridDragSelection::begin(&tab).unwrap();
        assert_eq!(
            drag.extend(&mut tab, GridCoord { row: 99, col: 99 }),
            Some(false)
        );
        assert_eq!(tab.grid.selection, GridSelection::single(0, 0));
        tab.grid.editing_cell = Some(CellEdit {
            row: 0,
            col: 0,
            text: "edit".into(),
            caret: 0,
            anchor: None,
        });
        assert!(GridDragSelection::begin(&tab).is_none());
        assert_eq!(drag.extend(&mut tab, GridCoord { row: 1, col: 1 }), None);
        assert_eq!(tab.grid.selection, GridSelection::single(0, 0));
    }
}
