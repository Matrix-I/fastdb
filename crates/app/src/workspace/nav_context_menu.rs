use super::*;

/// Right-click menu of a connection node in the navigator
#[derive(Debug, Clone)]
pub struct NavContextMenu {
    pub conn_id: String,
    /// Window position of the right click; the menu opens there
    pub position: Point<Pixels>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavMenuAction {
    EditConnection,
    Connect,
    Disconnect,
    Delete,
    Refresh,
}

impl Workspace {
    pub(super) fn open_nav_context_menu(&mut self, conn_id: &str, position: Point<Pixels>) {
        self.nav_context_menu = Some(NavContextMenu { conn_id: conn_id.to_string(), position });
    }

    pub(super) fn render_nav_context_menu(&mut self, menu: NavContextMenu, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let is_active = self.active_connection_id.as_deref() == Some(menu.conn_id.as_str());
        let is_connected = self.is_connected && is_active;

        let item = |id: &'static str, icon: &'static str, label: &'static str, enabled: bool, action: NavMenuAction, cx: &mut Context<Self>| {
            let conn_id = menu.conn_id.clone();
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
                        .h(px(14.0))
                        .opacity(if enabled { 1.0 } else { 0.4 })
                        .child(img(icon).w(px(14.0)).h(px(14.0))),
                )
                .child(label);
            if enabled {
                row = row
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.nav_context_menu = None;
                        this.run_nav_menu_action(action, &conn_id, cx);
                        cx.notify();
                    }));
            }
            row
        };
        let separator = || div().h(px(1.0)).my_1().mx_1().bg(theme.border);

        let panel = div()
            .id("nav-context-menu")
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
                this.nav_context_menu = None;
                cx.notify();
            }))
            .child(item("nav-menu-edit", "icons/edit.svg", "Edit Connection", true, NavMenuAction::EditConnection, cx))
            .child(separator())
            .child(item("nav-menu-connect", "icons/connect.svg", "Connect", !is_connected && !self.is_connecting, NavMenuAction::Connect, cx))
            .child(item("nav-menu-disconnect", "icons/disconnect.svg", "Disconnect", is_connected, NavMenuAction::Disconnect, cx))
            .child(separator())
            .child(item("nav-menu-delete", "icons/delete.svg", "Delete", true, NavMenuAction::Delete, cx))
            .child(separator())
            .child(item("nav-menu-refresh", "icons/refresh.svg", "Refresh", is_connected && !self.is_connecting, NavMenuAction::Refresh, cx));

        deferred(anchored().position(menu.position).snap_to_window().child(panel)).with_priority(1)
    }

    fn run_nav_menu_action(&mut self, action: NavMenuAction, conn_id: &str, cx: &mut Context<Self>) {
        match action {
            NavMenuAction::EditConnection => self.edit_saved_connection(Some(conn_id)),
            NavMenuAction::Connect => self.connect_saved_by_id(conn_id, cx),
            NavMenuAction::Disconnect => self.disconnect(),
            NavMenuAction::Delete => self.delete_saved_connection(conn_id),
            NavMenuAction::Refresh => self.refresh_connection(cx),
        }
    }
}
