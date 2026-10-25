use ui_grid::ColumnOrder;

use super::*;

/// Dropdown of a result grid column header, opened from its arrow
#[derive(Debug, Clone)]
pub struct ColumnMenu {
    pub tab_idx: usize,
    pub column: String,
    /// Window position of the click on the arrow; the menu opens there
    pub position: Point<Pixels>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColumnMenuAction {
    OrderAsc,
    OrderDesc,
    RemoveFiltersAndOrderings,
}

impl Workspace {
    pub(super) fn open_column_menu(&mut self, tab_idx: usize, col_idx: usize, position: Point<Pixels>) {
        let column = self
            .tabs
            .get(tab_idx)
            .and_then(|t| t.grid.pager.as_ref())
            .and_then(|p| p.columns.get(col_idx))
            .map(|c| c.name.clone());
        if let Some(column) = column {
            self.column_menu = Some(ColumnMenu { tab_idx, column, position });
        }
    }

    fn run_column_menu_action(&mut self, action: ColumnMenuAction, menu: &ColumnMenu, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(menu.tab_idx) else {
            return;
        };
        match action {
            ColumnMenuAction::OrderAsc | ColumnMenuAction::OrderDesc => {
                tab.grid.ordering = Some(ColumnOrder {
                    column: menu.column.clone(),
                    descending: action == ColumnMenuAction::OrderDesc,
                });
            }
            ColumnMenuAction::RemoveFiltersAndOrderings => {
                tab.grid.ordering = None;
                tab.filter_sql.clear();
            }
        }
        self.active_tab = menu.tab_idx;
        self.run_active_query(cx);
    }

    pub(super) fn render_column_menu(&mut self, menu: ColumnMenu, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let tab = self.tabs.get(menu.tab_idx);
        let current = tab.and_then(|t| t.grid.ordering.clone());
        let has_filter_or_order = tab.is_some_and(|t| t.grid.ordering.is_some() || !t.filter_sql.trim().is_empty());

        let item = |id: &'static str, icon: &'static str, label: String, checked: bool, enabled: bool, action: ColumnMenuAction, cx: &mut Context<Self>| {
            let mut row = div()
                .id(id)
                .flex()
                .items_center()
                .gap_2()
                .h(px(24.0))
                .px_2()
                .rounded_sm()
                .text_size(px(12.0))
                .text_color(if enabled { theme.text_primary } else { theme.text_muted })
                .child(
                    div()
                        .w(px(14.0))
                        .flex()
                        .justify_center()
                        .text_color(if enabled { rgb(0x58a6ff) } else { theme.text_muted })
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
                        this.column_menu = None;
                        this.run_column_menu_action(action, &menu, cx);
                        cx.notify();
                    }));
            }
            row
        };
        let separator = || div().h(px(1.0)).my_1().mx_1().bg(theme.border);
        let is_current = |descending: bool| {
            current.as_ref().is_some_and(|o| o.column == menu.column && o.descending == descending)
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
                this.column_menu = None;
                cx.notify();
            }))
            .child(item(
                "column-menu-order-asc",
                "↑",
                format!("Order by {} ASC", menu.column),
                is_current(false),
                true,
                ColumnMenuAction::OrderAsc,
                cx,
            ))
            .child(item(
                "column-menu-order-desc",
                "↓",
                format!("Order by {} DESC", menu.column),
                is_current(true),
                true,
                ColumnMenuAction::OrderDesc,
                cx,
            ))
            .child(separator())
            .child(item(
                "column-menu-remove-all",
                "✕",
                "Remove all filters/orderings".to_string(),
                false,
                has_filter_or_order,
                ColumnMenuAction::RemoveFiltersAndOrderings,
                cx,
            ));

        deferred(anchored().position(menu.position).snap_to_window().child(panel)).with_priority(1)
    }
}
