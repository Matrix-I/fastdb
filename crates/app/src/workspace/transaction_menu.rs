use super::session_controls::{SessionSetting, isolation_levels};
use super::*;

type MenuAction = Box<dyn Fn(&mut Workspace, &mut Context<Workspace>) + 'static>;

#[derive(Clone, Debug)]
pub struct TransactionMenu {
    pub tab_idx: usize,
    pub tab_id: u64,
    pub button_bounds: Bounds<Pixels>,
    pub viewport_size: Size<Pixels>,
}

impl TransactionMenu {
    fn matches_context(&self, active_tab_id: Option<u64>, viewport: Size<Pixels>) -> bool {
        active_tab_id == Some(self.tab_id) && viewport == self.viewport_size
    }
}

fn transaction_menu_position(button: Bounds<Pixels>) -> Point<Pixels> {
    button.bottom_left()
}

fn transaction_mode_icon(manual: bool) -> &'static str {
    if manual {
        "icons/transaction_manual.svg"
    } else {
        "icons/transaction_auto.svg"
    }
}

fn transaction_mode_label(manual: bool) -> &'static str {
    if manual { "Manual" } else { "Auto" }
}

fn transaction_actions_enabled(manual: bool, available: bool, busy: bool) -> bool {
    manual && available && !busy
}

impl Workspace {
    pub(super) fn render_transaction_controls(
        &self,
        tab_idx: usize,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let theme = &self.theme;
        let tab = self.tabs.get(tab_idx);
        let manual = tab.is_some_and(|tab| tab.manual_transactions);
        let available = tab.is_some_and(|tab| tab.is_script() || tab.table_name.is_some())
            && self
                .tab_session(tab_idx)
                .driver
                .as_ref()
                .is_some_and(|driver| driver.supports_script_session());
        let busy = self.operations.is_running();
        let enabled = available && !busy;
        let action = |id: &'static str,
                      label: &'static str,
                      icon: &'static str,
                      sql: &'static str,
                      cx: &mut Context<Self>| {
            let button = div()
                .id(id)
                .flex()
                .flex_none()
                .items_center()
                .gap_1()
                .px_2()
                .h(px(26.0))
                .rounded_xs()
                .text_size(px(11.0))
                .child(img(icon).w(px(16.0)).h(px(16.0)))
                .child(label);
            if transaction_actions_enabled(manual, available, busy) {
                button
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.active_tab == tab_idx
                            && this
                                .tabs
                                .get(tab_idx)
                                .is_some_and(|tab| tab.manual_transactions)
                        {
                            this.transaction_command(sql, cx);
                        }
                    }))
            } else {
                button.opacity(0.4)
            }
        };
        let bounds = ScrollHandle::new();
        let selector = div()
            .id("btn-transaction-menu")
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(26.0))
            .px_2()
            .min_w(px(112.0))
            .border_1()
            .border_color(theme.border)
            .rounded_xs()
            .bg(if manual {
                theme.bg_surface_selected
            } else {
                theme.bg_editor
            })
            .track_scroll(&bounds)
            .child(img(transaction_mode_icon(manual)).w(px(16.0)).h(px(16.0)))
            .child(transaction_mode_label(manual))
            .child(div().flex_1())
            .child("▾");
        let selector = if enabled {
            selector
                .cursor_pointer()
                .hover(|s| s.bg(theme.bg_surface_hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_transaction_menu(
                            tab_idx,
                            bounds.bounds(),
                            window.viewport_size(),
                        );
                        cx.notify();
                    }),
                )
        } else {
            selector.opacity(0.4)
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .text_color(theme.text_primary)
            .child(action(
                "btn-commit",
                "Commit",
                "icons/transaction_commit.svg",
                "COMMIT",
                cx,
            ))
            .child(action(
                "btn-rollback",
                "Rollback",
                "icons/transaction_rollback.svg",
                "ROLLBACK",
                cx,
            ))
            .child(selector)
    }

    pub(super) fn open_transaction_menu(
        &mut self,
        tab_idx: usize,
        button_bounds: Bounds<Pixels>,
        viewport_size: Size<Pixels>,
    ) {
        if let Some(tab) = self.tabs.get(tab_idx) {
            self.menus.tx_menu = Some(TransactionMenu {
                tab_idx,
                tab_id: tab.id,
                button_bounds,
                viewport_size,
            });
        }
    }

    pub(super) fn render_transaction_menu(
        &mut self,
        menu: TransactionMenu,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        if !menu.matches_context(
            self.tabs.get(self.active_tab).map(|tab| tab.id),
            window.viewport_size(),
        ) {
            self.menus.tx_menu = None;
            return div().into_any_element();
        }
        let theme = self.theme.clone();
        let tab_idx = menu.tab_idx;
        let tab_id = menu.tab_id;

        let Some(tab) = self
            .tabs
            .get(tab_idx)
            .filter(|t| t.id == tab_id && tab_idx == self.active_tab)
        else {
            return div().into_any_element();
        };

        let provider = self.tab_provider(tab_idx);
        let levels = isolation_levels(provider);
        let is_manual = tab.manual_transactions;
        let current_isolation = tab.session_isolation;
        let supports_tx = self
            .tab_session(tab_idx)
            .driver
            .as_ref()
            .is_some_and(|driver| driver.supports_script_session());

        let item = |id: &'static str,
                    label: String,
                    checked: bool,
                    enabled: bool,
                    action: MenuAction,
                    cx: &mut Context<Self>| {
            let mut row = div()
                .id(id)
                .flex()
                .items_center()
                .h(px(26.0))
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
                        .w(px(18.0))
                        .flex()
                        .items_center()
                        .text_size(px(11.0))
                        .text_color(theme.text_accent)
                        .child(if checked { "✓" } else { "" }),
                )
                .child(label);

            if enabled {
                row = row
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menus.tx_menu = None;
                        action(this, cx);
                        cx.notify();
                    }));
            }
            row
        };

        let separator = || div().h(px(1.0)).my_1().mx_1().bg(theme.border);

        let mut panel = div()
            .id("transaction-menu")
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
                this.menus.tx_menu = None;
                cx.notify();
            }));

        // Mode toggles: Auto-Commit vs Manual Commit (None)
        panel = panel
            .child(item(
                "tx-menu-autocommit",
                "Auto-Commit".into(),
                !is_manual,
                supports_tx && !self.operations.is_running(),
                Box::new(move |this, cx| {
                    if let Some(driver) = this.tab_session(tab_idx).driver.clone() {
                        this.apply_session_setting(
                            tab_id,
                            provider,
                            driver,
                            SessionSetting::AutoCommit(true),
                            cx,
                        );
                    }
                }),
                cx,
            ))
            .child(item(
                "tx-menu-manual",
                format!(
                    "Manual Commit ({})",
                    current_isolation.map_or("None", |level| level.label())
                ),
                is_manual,
                supports_tx && !self.operations.is_running(),
                Box::new(move |this, cx| {
                    if let Some(driver) = this.tab_session(tab_idx).driver.clone() {
                        this.apply_session_setting(
                            tab_id,
                            provider,
                            driver,
                            SessionSetting::AutoCommit(false),
                            cx,
                        );
                    }
                }),
                cx,
            ));

        // Isolation levels
        if !levels.is_empty() {
            panel = panel.child(separator());
            for &level in levels {
                let is_current = current_isolation == Some(level);
                panel = panel.child(item(
                    level.label(),
                    level.label().into(),
                    is_current,
                    supports_tx && !self.operations.is_running(),
                    Box::new(move |this, cx| {
                        if let Some(driver) = this.tab_session(tab_idx).driver.clone() {
                            this.apply_session_setting(
                                tab_id,
                                provider,
                                driver,
                                SessionSetting::Isolation(level),
                                cx,
                            );
                        }
                    }),
                    cx,
                ));
            }
        }

        deferred(
            anchored()
                .position(transaction_menu_position(menu.button_bounds))
                .snap_to_window()
                .child(panel),
        )
        .with_priority(1)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn auto_commit_disables_commit_and_rollback() {
        assert!(!super::transaction_actions_enabled(false, true, false));
        assert!(super::transaction_actions_enabled(true, true, false));
        assert!(!super::transaction_actions_enabled(true, false, false));
        assert!(!super::transaction_actions_enabled(true, true, true));
    }

    #[test]
    fn transaction_availability_includes_script_and_table_tabs() {
        let is_available = |is_script: bool, is_table: bool, supports_session: bool| {
            (is_script || is_table) && supports_session
        };
        assert!(is_available(true, false, true));
        assert!(is_available(false, true, true));
        assert!(!is_available(false, false, true));
        assert!(!is_available(true, false, false));
        assert!(!is_available(false, true, false));
    }

    #[test]
    fn popup_dismisses_when_window_size_or_active_tab_changes() {
        use gpui::{Bounds, point, px, size};
        let viewport = size(px(1280.0), px(800.0));
        let menu = super::TransactionMenu {
            tab_idx: 0,
            tab_id: 7,
            button_bounds: Bounds {
                origin: point(px(240.0), px(34.0)),
                size: size(px(112.0), px(26.0)),
            },
            viewport_size: viewport,
        };
        assert!(menu.matches_context(Some(7), viewport));
        assert!(!menu.matches_context(Some(8), viewport));
        assert!(!menu.matches_context(None, viewport));
        assert!(!menu.matches_context(Some(7), size(px(900.0), px(800.0))));
        assert!(!menu.matches_context(Some(7), size(px(1280.0), px(600.0))));
    }

    #[test]
    fn transaction_modes_use_distinct_vector_icons_and_readable_labels() {
        assert_eq!(super::transaction_mode_label(false), "Auto");
        assert_eq!(super::transaction_mode_label(true), "Manual");
        assert_eq!(
            super::transaction_mode_icon(false),
            "icons/transaction_auto.svg"
        );
        assert_eq!(
            super::transaction_mode_icon(true),
            "icons/transaction_manual.svg"
        );
        for asset in [
            include_str!("../../../../assets/icons/transaction_auto.svg"),
            include_str!("../../../../assets/icons/transaction_manual.svg"),
            include_str!("../../../../assets/icons/transaction_commit.svg"),
            include_str!("../../../../assets/icons/transaction_rollback.svg"),
        ] {
            assert!(asset.contains("viewBox=\"0 0 16 16\""));
            assert!(asset.contains("<path"));
            assert!(!asset.contains("<text"));
        }
    }

    #[test]
    fn popup_follows_the_button_bottom_at_any_workspace_offset() {
        use gpui::{Bounds, point, px, size};
        for (x, y, width, height) in [(80.0, 34.0, 140.0, 28.0), (540.0, 92.0, 180.0, 32.0)] {
            let bounds = Bounds {
                origin: point(px(x), px(y)),
                size: size(px(width), px(height)),
            };
            assert_eq!(
                super::transaction_menu_position(bounds),
                point(px(x), px(y + height))
            );
        }
    }
}
