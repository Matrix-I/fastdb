use super::session_controls::{SessionSetting, isolation_levels};
use super::*;

type MenuAction = Box<dyn Fn(&mut Workspace, &mut Context<Workspace>) + 'static>;

#[derive(Clone, Debug)]
pub struct TransactionMenu {
    pub tab_idx: usize,
    pub tab_id: u64,
    pub position: Point<Pixels>,
}

impl Workspace {
    pub(super) fn open_transaction_menu(&mut self, tab_idx: usize, position: Point<Pixels>) {
        if let Some(tab) = self.tabs.get(tab_idx) {
            self.menus.tx_menu = Some(TransactionMenu {
                tab_idx,
                tab_id: tab.id,
                position,
            });
        }
    }

    pub(super) fn render_transaction_menu(
        &mut self,
        menu: TransactionMenu,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let tab_idx = menu.tab_idx;
        let tab_id = menu.tab_id;

        let Some(tab) = self.tabs.get(tab_idx).filter(|t| t.id == tab_id) else {
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

        // Commit / Rollback actions
        panel = panel
            .child(item(
                "tx-menu-commit",
                "Commit".into(),
                false,
                supports_tx && !self.operations.is_running(),
                Box::new(move |this, cx| this.transaction_command("COMMIT", cx)),
                cx,
            ))
            .child(item(
                "tx-menu-rollback",
                "Rollback".into(),
                false,
                supports_tx && !self.operations.is_running(),
                Box::new(move |this, cx| this.transaction_command("ROLLBACK", cx)),
                cx,
            ))
            .child(separator());

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
                "Manual Commit (None)".into(),
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
                .position(point(px(38.0), menu.position.y))
                .snap_to_window()
                .child(panel),
        )
        .with_priority(1)
        .into_any_element()
    }
}
