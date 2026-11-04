use ui_grid::ColumnOrder;

use super::*;

/// Dropdown of a result grid column header, opened from its arrow
#[derive(Clone)]
pub struct ColumnMenu {
    tab_id: u64,
    pager: Arc<db_wire::QueryResultPager>,
    source_column: usize,
    pub column: String,
    /// Window position of the click on the arrow; the menu opens there
    pub position: Point<Pixels>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnMenuAction {
    Order { descending: bool, append: bool },
    Move { forward: bool },
    Hide,
    ResetLayout,
    RemoveFiltersAndOrderings,
}

impl Workspace {
    pub(super) fn open_column_menu(
        &mut self,
        tab_idx: usize,
        col_idx: usize,
        position: Point<Pixels>,
    ) {
        let Some(tab) = self.tabs.get(tab_idx) else {
            return;
        };
        let Some(pager) = tab.grid.pager.clone() else {
            return;
        };
        let Some(column) = pager.columns.get(col_idx).map(|column| column.name.clone()) else {
            return;
        };
        self.menus.column_menu = Some(ColumnMenu {
            tab_id: tab.id,
            pager,
            source_column: col_idx,
            column,
            position,
        });
    }

    fn column_menu_index(&self, menu: &ColumnMenu) -> Option<usize> {
        let tab = self.tabs.get(self.active_tab)?;
        (tab.id == menu.tab_id
            && tab
                .grid
                .pager
                .as_ref()
                .is_some_and(|pager| Arc::ptr_eq(pager, &menu.pager)))
        .then_some(self.active_tab)
    }

    fn run_column_menu_action(
        &mut self,
        action: ColumnMenuAction,
        menu: &ColumnMenu,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.column_menu_index(menu) else {
            return;
        };
        let tab = &mut self.tabs[index];
        match action {
            ColumnMenuAction::Move { forward } => tab.grid.move_column(menu.source_column, forward),
            ColumnMenuAction::Hide => tab.grid.hide_column(menu.source_column),
            ColumnMenuAction::ResetLayout => tab.grid.reset_column_layout(),
            action => {
                if tab.grid.changeset.is_dirty() || tab.grid.editing_cell.is_some() {
                    tab.last_error =
                        Some("Save or Cancel row edits before changing sort or filters".into());
                    return;
                }
                if self.operations.is_running() || tab.table_name.is_none() {
                    return;
                }
                match action {
                    ColumnMenuAction::Order { descending, append } => {
                        let clauses = sql_intel::filter_clauses(&tab.filter_sql);
                        if !tab.filter_sql.trim().is_empty()
                            && clauses.is_none_or(|c| c.order_by || c.limit || c.offset || c.fetch)
                        {
                            tab.last_error = Some("Remove explicit ordering or paging from the filter before using header sorting".into());
                            return;
                        }
                        set_order_key(&mut tab.grid.ordering, &menu.column, descending, append);
                    }
                    ColumnMenuAction::RemoveFiltersAndOrderings => {
                        tab.grid.ordering.clear();
                        tab.filter_sql.clear();
                        tab.filter_spec = None;
                    }
                    _ => unreachable!(),
                }
                self.run_active_query(cx);
            }
        }
    }

    pub(super) fn render_column_menu(
        &mut self,
        menu: ColumnMenu,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let tab = self
            .column_menu_index(&menu)
            .and_then(|index| self.tabs.get(index));
        let can_sort = tab.is_some_and(|tab| tab.table_name.is_some());
        let can_layout = tab.is_some_and(|tab| tab.grid.editing_cell.is_none());
        let has_filter_or_order = tab
            .is_some_and(|tab| !tab.grid.ordering.is_empty() || !tab.filter_sql.trim().is_empty());
        let can_move = |forward| {
            can_layout
                && tab.is_some_and(|tab| {
                    tab.grid
                        .adjacent_column(menu.source_column, forward)
                        .is_some()
                })
        };

        let item = |id: &'static str,
                    icon: &'static str,
                    label: String,
                    checked: bool,
                    enabled: bool,
                    action: ColumnMenuAction,
                    cx: &mut Context<Self>| {
            let mut row = div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .h(px(24.0))
                .px_2()
                .rounded_sm()
                .text_size(px(12.0))
                .text_color(if enabled {
                    theme.text_primary
                } else {
                    theme.text_muted
                })
                .child(
                    div()
                        .w(px(14.0))
                        .flex()
                        .justify_center()
                        .text_color(if enabled {
                            rgb(0x58a6ff)
                        } else {
                            theme.text_muted
                        })
                        .child(icon),
                )
                .child(div().flex_1().child(label))
                .children(checked.then(|| div().text_color(theme.text_muted).child("✓")));
            if enabled {
                let menu = menu.clone();
                row = row
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menus.column_menu = None;
                        this.run_column_menu_action(action, &menu, cx);
                        cx.notify();
                    }));
            }
            row
        };
        let separator = || div().h(px(1.0)).my_1().mx_1().bg(theme.border);
        let is_current = |descending| {
            tab.is_some_and(|tab| {
                tab.grid
                    .ordering
                    .iter()
                    .any(|key| key.column == menu.column && key.descending == descending)
            })
        };

        let panel = div()
            .id("column-menu")
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(220.0))
            .p_1()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menus.column_menu = None;
                cx.notify();
            }))
            .children(can_sort.then(|| {
                div()
                    .child(item(
                        "column-menu-order-asc",
                        "↑",
                        "Sort ascending".into(),
                        is_current(false),
                        true,
                        ColumnMenuAction::Order {
                            descending: false,
                            append: false,
                        },
                        cx,
                    ))
                    .child(item(
                        "column-menu-order-desc",
                        "↓",
                        "Sort descending".into(),
                        is_current(true),
                        true,
                        ColumnMenuAction::Order {
                            descending: true,
                            append: false,
                        },
                        cx,
                    ))
                    .child(item(
                        "column-menu-append-asc",
                        "+",
                        "Add ascending sort key".into(),
                        false,
                        true,
                        ColumnMenuAction::Order {
                            descending: false,
                            append: true,
                        },
                        cx,
                    ))
                    .child(item(
                        "column-menu-append-desc",
                        "+",
                        "Add descending sort key".into(),
                        false,
                        true,
                        ColumnMenuAction::Order {
                            descending: true,
                            append: true,
                        },
                        cx,
                    ))
                    .child(item(
                        "column-menu-remove-all",
                        "✕",
                        "Remove all filters/orderings".into(),
                        false,
                        has_filter_or_order,
                        ColumnMenuAction::RemoveFiltersAndOrderings,
                        cx,
                    ))
                    .child(separator())
            }))
            .child(item(
                "column-menu-left",
                "←",
                "Move left".into(),
                false,
                can_move(false),
                ColumnMenuAction::Move { forward: false },
                cx,
            ))
            .child(item(
                "column-menu-right",
                "→",
                "Move right".into(),
                false,
                can_move(true),
                ColumnMenuAction::Move { forward: true },
                cx,
            ))
            .child(item(
                "column-menu-hide",
                "−",
                "Hide column".into(),
                false,
                can_layout && tab.is_some_and(|tab| tab.grid.display_columns().len() > 1),
                ColumnMenuAction::Hide,
                cx,
            ))
            .child(item(
                "column-menu-reset-layout",
                "↺",
                "Restore all columns".into(),
                false,
                can_layout,
                ColumnMenuAction::ResetLayout,
                cx,
            ));

        deferred(
            anchored()
                .position(menu.position)
                .snap_to_window()
                .child(panel),
        )
        .with_priority(1)
    }
}

fn set_order_key(keys: &mut Vec<ColumnOrder>, column: &str, descending: bool, append: bool) {
    if !append {
        keys.clear();
    }
    if let Some(key) = keys.iter_mut().find(|key| key.column == column) {
        key.descending = descending;
    } else {
        keys.push(ColumnOrder {
            column: column.into(),
            descending,
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn appending_existing_key_keeps_priority_and_updates_direction() {
        let mut keys = Vec::new();
        super::set_order_key(&mut keys, "group", false, false);
        super::set_order_key(&mut keys, "rank", true, true);
        super::set_order_key(&mut keys, "group", true, true);
        assert_eq!(
            keys.iter()
                .map(|key| (key.column.as_str(), key.descending))
                .collect::<Vec<_>>(),
            [("group", true), ("rank", true)]
        );
        super::set_order_key(&mut keys, "rank", false, false);
        assert_eq!(keys.len(), 1);
        assert!(!keys[0].descending);
    }
}
