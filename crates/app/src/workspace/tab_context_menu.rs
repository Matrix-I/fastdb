use super::*;

/// Right-click menu of a tab in the top bar
#[derive(Debug, Clone)]
pub struct TabContextMenu {
    pub tab_idx: usize,
    /// Window position of the right click; the menu opens there
    pub position: Point<Pixels>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TabMenuAction {
    Close,
    CloseOthers,
    CloseLeft,
    CloseRight,
    CloseAll,
}

impl TabMenuAction {
    /// Whether the tab at `idx` stays open when this action runs on tab `target`
    fn keeps(self, idx: usize, target: usize) -> bool {
        match self {
            TabMenuAction::Close => idx != target,
            TabMenuAction::CloseOthers => idx == target,
            TabMenuAction::CloseLeft => idx >= target,
            TabMenuAction::CloseRight => idx <= target,
            TabMenuAction::CloseAll => false,
        }
    }
}

/// Active tab index after closing every tab that `keep` rejects.
/// The active tab stays active if it survives; otherwise `fallback` (the right-clicked tab)
/// takes over if it survives, else the survivor that slides into the old active slot
/// (its right neighbour), or the last tab when nothing is left to its right.
fn active_after_close(
    len: usize,
    active: usize,
    fallback: usize,
    keep: impl Fn(usize) -> bool,
) -> usize {
    let new_index = |idx: usize| (0..idx).filter(|&i| keep(i)).count();
    if active < len && keep(active) {
        return new_index(active);
    }
    if fallback < len && keep(fallback) {
        return new_index(fallback);
    }
    let survivors = new_index(len);
    new_index(active.min(len)).min(survivors.saturating_sub(1))
}

impl Workspace {
    pub(super) fn open_tab_context_menu(&mut self, tab_idx: usize, position: Point<Pixels>) {
        self.menus.tab_context_menu = Some(TabContextMenu { tab_idx, position });
    }

    /// Closes every tab that `keep` rejects (by index) and re-points the active tab
    pub(super) fn close_tabs_where(
        &mut self,
        fallback: usize,
        keep: impl Fn(usize) -> bool,
        cx: &mut Context<Self>,
    ) {
        let ids = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(i, _)| !keep(*i))
            .map(|(_, t)| t.id)
            .collect::<Vec<_>>();
        let _ = fallback;
        self.save_then(ids.clone(), AfterSave::Close(ids), cx);
    }

    pub(super) fn close_saved_tabs(&mut self, ids: &[u64], cx: &mut Context<Self>) {
        if self
            .operations
            .active()
            .is_some_and(|op| ids.contains(&op.tab_id))
        {
            self.cancel_running_query(cx);
        }
        let active_id = self.tabs.get(self.active_tab).map(|t| t.id);
        let new_index =
            active_after_close(self.tabs.len(), self.active_tab, self.active_tab, |i| {
                !ids.contains(&self.tabs[i].id)
            });
        self.tabs.retain(|tab| !ids.contains(&tab.id));
        self.active_tab = active_id
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(new_index);
        self.input.is_filter_focused = false;
        self.save_session();
        cx.notify();
    }

    fn run_tab_menu_action(
        &mut self,
        action: TabMenuAction,
        target: usize,
        cx: &mut Context<Self>,
    ) {
        if target < self.tabs.len() {
            self.close_tabs_where(target, |idx| action.keeps(idx, target), cx);
        }
    }

    pub(super) fn render_tab_context_menu(
        &mut self,
        menu: TabContextMenu,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let target = menu.tab_idx;
        let len = self.tabs.len();
        let has_left = target > 0;
        let has_right = target + 1 < len;

        let item = |id: &'static str,
                    label: &'static str,
                    enabled: bool,
                    action: TabMenuAction,
                    cx: &mut Context<Self>| {
            let mut row = div()
                .id(id)
                .flex()
                .items_center()
                .h(px(24.0))
                .px_2()
                .rounded_sm()
                .text_size(px(12.0))
                .text_color(if enabled {
                    theme.text_primary
                } else {
                    theme.text_muted
                })
                .child(label);
            if enabled {
                row = row
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menus.tab_context_menu = None;
                        this.run_tab_menu_action(action, target, cx);
                        cx.notify();
                    }));
            }
            row
        };
        let separator = || div().h(px(1.0)).my_1().mx_1().bg(theme.border);

        let mut panel = div()
            .id("tab-context-menu")
            .occlude()
            .flex()
            .flex_col()
            .min_w(px(200.0))
            .p_1()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menus.tab_context_menu = None;
                cx.notify();
            }))
            .child(item(
                "tab-menu-close",
                "Close",
                true,
                TabMenuAction::Close,
                cx,
            ))
            .child(item(
                "tab-menu-close-others",
                "Close Others",
                len > 1,
                TabMenuAction::CloseOthers,
                cx,
            ));
        // Directional items only show when there is a tab on that side
        if has_left {
            panel = panel.child(item(
                "tab-menu-close-left",
                "Close Tabs to the Left",
                true,
                TabMenuAction::CloseLeft,
                cx,
            ));
        }
        if has_right {
            panel = panel.child(item(
                "tab-menu-close-right",
                "Close Tabs to the Right",
                true,
                TabMenuAction::CloseRight,
                cx,
            ));
        }
        panel = panel.child(separator()).child(item(
            "tab-menu-close-all",
            "Close All",
            true,
            TabMenuAction::CloseAll,
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

#[cfg(test)]
mod tests {
    // Not a glob: `super::*` brings in `gpui::test`, which shadows the built-in `#[test]`.
    use super::{TabMenuAction, active_after_close};

    fn after(action: TabMenuAction, len: usize, active: usize, target: usize) -> usize {
        active_after_close(len, active, target, |i| action.keeps(i, target))
    }

    #[test]
    fn close_keeps_active_and_shifts_it_left() {
        // Closing a tab left of the active one shifts the active index down
        assert_eq!(after(TabMenuAction::Close, 5, 3, 1), 2);
        // Closing a tab right of the active one leaves it alone
        assert_eq!(after(TabMenuAction::Close, 5, 1, 3), 1);
    }

    #[test]
    fn closing_the_active_tab_activates_its_right_neighbour() {
        assert_eq!(after(TabMenuAction::Close, 5, 2, 2), 2);
        assert_eq!(after(TabMenuAction::Close, 5, 0, 0), 0);
        // The last tab falls back to its left neighbour
        assert_eq!(after(TabMenuAction::Close, 5, 4, 4), 3);
    }

    #[test]
    fn close_others_activates_the_target() {
        assert_eq!(after(TabMenuAction::CloseOthers, 5, 4, 2), 0);
    }

    #[test]
    fn close_left_and_right() {
        // Active survives: index shifts by the number of closed tabs before it
        assert_eq!(after(TabMenuAction::CloseLeft, 5, 4, 2), 2);
        assert_eq!(after(TabMenuAction::CloseRight, 5, 1, 2), 1);
        // Active is closed: the right-clicked tab takes over
        assert_eq!(after(TabMenuAction::CloseLeft, 5, 0, 2), 0);
        assert_eq!(after(TabMenuAction::CloseRight, 5, 4, 2), 2);
    }

    #[test]
    fn keeps_matches_each_action() {
        let kept = |a: TabMenuAction| (0..5).filter(|&i| a.keeps(i, 2)).collect::<Vec<_>>();
        assert_eq!(kept(TabMenuAction::Close), vec![0, 1, 3, 4]);
        assert_eq!(kept(TabMenuAction::CloseOthers), vec![2]);
        assert_eq!(kept(TabMenuAction::CloseLeft), vec![2, 3, 4]);
        assert_eq!(kept(TabMenuAction::CloseRight), vec![0, 1, 2]);
        assert!(kept(TabMenuAction::CloseAll).is_empty());
    }
}
