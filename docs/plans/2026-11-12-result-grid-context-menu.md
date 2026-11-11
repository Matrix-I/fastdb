# Result Grid Context Menu Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: development-superpowers:subagent-driven-development or executing-plans-strict. Steps use `- [ ]`.

**Goal / Architecture / Tech Stack / Branch:** Extend the GPUI grid background and route right-clicks through the workspace's existing menu. Rust, GPUI. Branch `feature/result-grid-context-menu`.

## Confirmed Requirements

- Continue row and column rules through the empty part of the result grid.
- Open the context menu from the entire grid panel, including empty space.
- Place Refresh last for table Data grids and reuse the toolbar's reload action.

## Global Constraints

- Preserve virtualized rows, selection and column layout. Keep cell actions tied to valid targets and route Refresh through existing staged-edit guards.
- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and run the app for UI changes.
- Commit only when asked. Desktop acceptance is manual and Computer Use is excluded.

## Technical Defaults

- Empty space retains the selection and offers copy when a selection is available. Cell mutations and reference actions require a clicked data cell.
- Draw rules using the existing displayed column widths, including the row-number gutter. Continue column rules through partially visible blank rows.
- A background click opens the menu before a result is available and leaves any inline edit open. Use an optional pager snapshot and keep cell actions guarded by pager identity.

## Risks & Rollback

- Synthetic rows could change scrolling and fetch behavior. Paint rules in a canvas and leave the list's row count tied to data. Revert the canvas and menu wiring together. Persisted formats and public interfaces are unaffected.

## Grid body and workspace menu

- Files: `crates/ui-grid/src/grid_element/rows.rs` (body and tests), `crates/app/src/workspace/data_panel.rs` (grid event handlers), `crates/app/src/workspace/grid_clipboard.rs` (menu state, actions and tests).
- [ ] Test the range of complete blank rows using `blank_row_indices(total_rows: u64, viewport_height: f32, row_height: f32) -> std::ops::Range<u64>`. Assert that `blank_row_indices(2, 75.0, 20.0)` yields `2..3`, with empty ranges when rows already fill the panel. Run `cargo test -p ui-grid blank_row` and expect the test to fail to compile until the geometry function exists.
- [ ] Compute the blank-row range using `total_rows..total_rows.max((viewport_height / row_height) as u64)`. Add a body canvas to paint the horizontal rules and extend visible column boundaries below the final data row. Use actual body bounds and existing horizontal virtualization.
- [ ] Test table-only Refresh eligibility with matching and stale tab IDs and a SQL tab. Run `cargo test -p fastdb-app grid_refresh` and expect compilation to fail until the eligibility predicate and optional menu fields exist.
- [ ] Change the private menu opener to accept `cell: Option<ui_grid::GridCoord>`. Keep the pager snapshot for copy and cell actions. Consume right-clicks on data cells and use the panel's bubbling handler for background clicks. Append Refresh for the active table Data tab and recheck its identity before calling `run_active_query`.
- [ ] Rerun both targeted tests. Ready to commit the scoped code and guide updates as `feat(app): extend result grid context menu`.

## Verification & Docs

- [ ] Update `docs/guide/navigator-and-tables.md` and `docs/guide/not-built-yet.md`. Read the full diff including untracked files. Run workspace tests, build, Clippy and formatting checks, then launch FastDB.
- [ ] Desktop acceptance: open a short or empty table result, scroll horizontally, and verify the rules stay aligned below the rows. Right-click a cell and empty space. Refresh should be last and reload the filtered table. Verify that staged edits block Refresh and SQL grids omit Refresh. Record manual acceptance as pending.

## Exact test and implementation reference

### crates/ui-grid/src/grid_element/rows.rs, lines 1–102

```rust
use super::*;

fn blank_row_indices(
    total_rows: u64,
    viewport_height: f32,
    row_height: f32,
) -> std::ops::Range<u64> {
    total_rows..total_rows.max((viewport_height / row_height) as u64)
}

impl VirtualDataGrid {
    pub(super) fn render_rows(
        &self,
        total_width: f32,
        on_cell_mouse_down: Option<CellMouseDownHandler>,
        on_cell_mouse_move: Option<CellMouseMoveHandler>,
        cell_editor: Option<(u64, usize, CellEditorRenderer)>,
    ) -> AnyElement {
        let theme = &self.theme;
        let row_height = theme.row_height;
        let gutter_width = GUTTER_WIDTH;
        let total_rows = self.row_count();
        // 2. Data Rows with full virtualization using uniform_list
        let total_rows_usize = total_rows as usize;
        let vertical_scroll_handle = self.vertical_scroll_handle.clone();
        let pager_clone = self.pager.clone();
        let column_widths_clone = self.column_widths.clone();
        let selection = self.selection_display_range();
        let changeset_clone = self.changeset.clone();
        let theme_bg_grid_alt = theme.bg_grid_alt;
        let theme_bg_grid = theme.bg_grid;
        let theme_border = theme.border;
        let theme_text_muted = theme.text_muted;
        let theme_text_primary = theme.text_primary;
        let theme_bg_cell_selected = theme.bg_grid_cell_selected;
        let theme_bg_cell_modified = theme.bg_grid_cell_modified;
        let theme_border_focused = theme.border_focused;
        let theme_bg_editor = theme.bg_editor;
        let theme_text_success = theme.text_success;
        let theme_text_error = theme.text_error;
        let scroll_x = -f32::from(self.horizontal_scroll_handle.offset().x);
        let display_columns = self.display_columns().to_vec();
        let display_widths: Vec<_> = display_columns
            .iter()
            .map(|&col| self.displayed_column_width(col))
            .collect();
        let (visible_cols, cols_offset) =
            visible_columns(&display_widths, scroll_x, self.viewport_width);

        let blank_rules = {
            let visible_cols = visible_cols.clone();
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let data_height = total_rows as f32 * row_height;
                    let blank_height = f32::from(bounds.size.height) - data_height;
                    if blank_height <= 0.0 {
                        return;
                    }

                    let blank_top = bounds.top() + px(data_height);
                    let rule = px(1.0);
                    let mut right_edge = bounds.left() + px(gutter_width);
                    let mut paint_column = |edge| {
                        window.paint_quad(fill(
                            Bounds::new(
                                point(edge - rule, blank_top),
                                size(rule, px(blank_height)),
                            ),
                            theme_border,
                        ));
                    };
                    paint_column(right_edge);
                    right_edge += px(cols_offset);
                    for position in visible_cols {
                        right_edge += px(display_widths[position]);
                        paint_column(right_edge);
                    }
                    for row in blank_row_indices(total_rows, bounds.size.height.into(), row_height)
                    {
                        let y = bounds.top() + px((row + 1) as f32 * row_height) - rule;
                        window.paint_quad(fill(
                            Bounds::new(point(bounds.left(), y), size(bounds.size.width, rule)),
                            theme_border,
                        ));
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
        };

        let body = div()
            .id("grid-data-body-container")
            .flex()
            .flex_col()
            .flex_1()
            .w(px(total_width))
            .h_full()
            .overflow_hidden()
```

### crates/ui-grid/src/grid_element/rows.rs, lines 245–257

```rust
#[cfg(test)]
mod tests {
    use super::blank_row_indices;

    #[test]
    fn blank_row_rules_fit_the_viewport_without_adding_data_rows() {
        assert_eq!(blank_row_indices(2, 75.0, 20.0), 2..3);
        assert_eq!(blank_row_indices(0, 60.0, 20.0), 0..3);
        assert_eq!(blank_row_indices(4, 75.0, 20.0), 4..4);
    }
}
```

### crates/app/src/workspace/grid_clipboard.rs, lines 25–49

```rust
#[derive(Clone)]
pub(super) struct GridContextMenu {
    tab_id: u64,
    pager: Option<Arc<db_wire::QueryResultPager>>,
    position: Point<Pixels>,
    cell: Option<ui_grid::GridCoord>,
    revision: u64,
}

impl GridContextMenu {
    fn matches_result(&self, tab: &TabState) -> bool {
        tab.id == self.tab_id
            && tab
                .grid
                .pager
                .as_ref()
                .zip(self.pager.as_ref())
                .is_some_and(|(current, original)| Arc::ptr_eq(current, original))
    }

    fn can_refresh(&self, tab: &TabState) -> bool {
        tab.id == self.tab_id && tab.table_name.is_some() && tab.active_subtab == TableSubTab::Data
    }
}

```

### crates/app/src/workspace/grid_clipboard.rs, lines 168–215

```rust
    pub(super) fn open_grid_context_menu(
        &mut self,
        index: usize,
        cell: Option<ui_grid::GridCoord>,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if cell.is_some() {
            self.commit_cell_edit();
        }
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let pager = tab.grid.pager.clone();
        if let Some(ui_grid::GridCoord { row, col }) = cell {
            let Some(pager) = pager.as_ref() else {
                return;
            };
            if row >= tab.grid.row_count() || col >= pager.columns.len() {
                return;
            }
            let selected =
                tab.grid
                    .selection_display_range()
                    .is_some_and(|(first, last, columns)| {
                        (first..=last).contains(&row)
                            && tab.grid.display_columns()[columns].contains(&col)
                    });
            if !selected {
                tab.grid.selection = GridSelection::single(row, col);
            }
        }
        tab.editor_focused = false;
        self.input.is_filter_focused = false;
        self.input.is_fetch_size_focused = false;
        self.menus.nav_context_menu = None;
        self.menus.tab_context_menu = None;
        self.menus.column_menu = None;
        self.menus.grid_context_menu = Some(GridContextMenu {
            tab_id: tab.id,
            pager,
            position,
            cell,
            revision: tab.grid.changeset_revision(),
        });
        cx.notify();
    }

```

### crates/app/src/workspace/grid_clipboard.rs, lines 427–451

```rust
                        })),
                );
            }
        }
        if self
            .tabs
            .get(self.active_tab)
            .is_some_and(|tab| menu.can_refresh(tab))
        {
            let refresh_menu = menu.clone();
            panel = panel.child(super::script_tools::tool("Refresh").on_click(cx.listener(
                move |this, _, _, cx| {
                    this.menus.grid_context_menu = None;
                    if this
                        .tabs
                        .get(this.active_tab)
                        .is_some_and(|tab| refresh_menu.can_refresh(tab))
                    {
                        this.run_active_query(cx);
                    }
                    cx.notify();
                },
            )));
        }
        deferred(
```

### crates/app/src/workspace/grid_clipboard.rs, lines 470–497

```rust
    #[test]
    fn grid_refresh_targets_only_the_active_table_data_tab() {
        use super::{GridContextMenu, TabState, TableSubTab};
        let mut tab = TabState::table(
            7,
            "main",
            "items",
            Theme::dark(),
            Arc::new(sql_intel::SchemaCache::new().unwrap()),
        );
        let menu = GridContextMenu {
            tab_id: 7,
            pager: None,
            position: Default::default(),
            cell: None,
            revision: 0,
        };

        assert!(menu.can_refresh(&tab));
        tab.id = 8;
        assert!(!menu.can_refresh(&tab));
        tab.id = 7;
        tab.active_subtab = TableSubTab::Properties;
        assert!(!menu.can_refresh(&tab));
        tab.active_subtab = TableSubTab::Data;
        tab.table_name = None;
        assert!(!menu.can_refresh(&tab));
    }
```

### crates/app/src/workspace/data_panel.rs, lines 194–212

```rust
                            let view = cx.entity().downgrade();
                            let cell_view = view.clone();
                            let on_cell_mouse_down: ui_grid::CellMouseDownHandler = std::rc::Rc::new(move |row, col, event, window, app| {
                                if event.button == MouseButton::Right {
                                    app.stop_propagation();
                                }
                                let _ = cell_view.update(app, |this, cx| {
                                    this.input.grid_drag = None;
                                    window.focus(&this.focus_handle);
                                    if event.button == MouseButton::Right {
                                        this.open_grid_context_menu(active_tab_idx, Some(ui_grid::GridCoord { row, col }), event.position, cx);
                                        return;
                                    }
                                    this.select_grid_cell(row, col, event.modifiers.shift, cx);
                                    if event.click_count >= 2 && !event.modifiers.shift {
                                        this.begin_cell_edit(row, col);
                                    } else {
                                        this.input.grid_drag = super::grid_selection::GridDragSelection::begin(&this.tabs[active_tab_idx]);
                                    }
```

### crates/app/src/workspace/data_panel.rs, lines 292–310

```rust
                                    .into_any_element()
                            });
                            div()
                                .id("result-grid-panel")
                                .flex()
                                .flex_col()
                                .flex_1()
                                .size_full()
                                .overflow_hidden()
                                .on_mouse_down(MouseButton::Right, cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    this.input.grid_drag = None;
                                    window.focus(&this.focus_handle);
                                    this.open_grid_context_menu(active_tab_idx, None, event.position, cx);
                                }))
                                .child(grid)
                                .into_any_element()
                        }
                        DataViewMode::Text => {
                            let tab = &self.tabs[active_tab_idx];
```

## Verification evidence

- The geometry negative control removed blank rows and failed with `2..2` versus `2..3`.
- The Refresh negative control hid Refresh for every tab and failed the eligible table assertion.
- Both negative controls were restored before the final workspace checks.
- Independent implementation review found no blocking issues. The user confirmed manual GUI acceptance on 2026-11-12.

- Final verification passed formatting, Clippy, workspace build and 693 tests. External provider tests were unconfigured and skipped their live checks.
