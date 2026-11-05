use super::*;
use gpui::prelude::FluentBuilder;

impl Workspace {
    /// DBeaver-style database navigator tree in the left sidebar
    pub(super) fn render_navigator(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let theme = &theme;
        div()
            .flex()
            .flex_col()
            .relative()
            .flex_none()
            .w(px(self.layout.navigator_width))
            .h_full()
            .bg(theme.bg_sidebar)
            .border_r_1()
            .border_color(theme.border)
            .p_2()
            .gap_2()
            // Sidebar Header
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_2()
                    .py_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_size(px(11.0))
                            .font_weight(FontWeight::BOLD)
                            .text_color(theme.text_secondary)
                            .child(img("icons/database_navigator.svg").w(px(14.0)).h(px(14.0)))
                            .child("DATABASE NAVIGATOR"),
                    )
                    .child(
                        div().flex().items_center().gap_1().child(
                            div()
                                .id("btn-open-connect-dialog")
                                .flex()
                                .items_center()
                                .gap_1()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(theme.bg_surface)
                                .hover(|s| s.bg(theme.bg_surface_hover))
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_new_connection_dialog();
                                    cx.notify();
                                }))
                                .child(
                                    div()
                                        .text_size(px(13.0))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(theme.text_accent)
                                        .child("+"),
                                )
                                .child(img("icons/connect.svg").w(px(12.0)).h(px(12.0))),
                        ),
                    ),
            )
            .when(self.connections.active().driver.is_some(), |panel| {
                panel.child(script_tools::tool("Search objects…").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.open_object_search(cx);
                        window.focus(&this.focus_handle);
                    },
                )))
            })
            .when(self.active_reveal_target().is_some(), |panel| {
                panel.child(script_tools::tool("Reveal active object").on_click(
                    cx.listener(|this, _, window, cx| this.reveal_active_object(window, cx)),
                ))
            })
            .when(self.connections.active().driver.is_some(), |panel| {
                panel.child(
                    script_tools::tool("Browse catalog objects…")
                        .on_click(cx.listener(|this, _, _, cx| this.open_object_browser(cx))),
                )
            })
            // Tree View Container with vertical scroll
            .child({
                let mut nav_tree = div()
                    .id("dbeaver-nav-tree")
                    .flex()
                    .flex_col()
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.navigator_scroll)
                    .gap_0p5();

                if self.connections.saved_connections.is_empty()
                    && !self.connections.active().driver.is_some()
                {
                    nav_tree = nav_tree.child(
                        div()
                            .p_4()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .text_size(px(11.0))
                            .text_color(theme.text_muted)
                            .child("No database connections")
                            .child(
                                div()
                                    .id("btn-sidebar-connect-prompt")
                                    .px_3()
                                    .py_1p5()
                                    .rounded_md()
                                    .bg(theme.bg_surface_selected)
                                    .text_color(theme.text_accent)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.open_new_connection_dialog();
                                        cx.notify();
                                    }))
                                    .flex()
                                    .items_center()
                                    .gap_1p5()
                                    .child(img("icons/connect.svg").w(px(14.0)).h(px(14.0)))
                                    .child("Add Connection"),
                            ),
                    );
                } else {
                    let mut groups = std::collections::BTreeMap::<_, Vec<_>>::new();
                    for conn in &self.connections.saved_connections {
                        groups
                            .entry(conn.appearance.group.as_deref())
                            .or_default()
                            .push(conn);
                    }
                    for (name, connections) in groups {
                        if let Some(name) = name {
                            nav_tree = nav_tree.child(
                                div()
                                    .pl_2()
                                    .pt_1()
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.text_secondary)
                                    .child(name.to_owned()),
                            );
                        }
                        for conn in connections {
                            nav_tree = self.append_connection_node(nav_tree, conn, cx);
                        }
                    }
                }

                nav_tree
            })
            .child(self.render_panel_splitter(PanelSplitter::Navigator, cx))
    }
}

impl Workspace {
    /// Connection node action: expand/collapse the active connection, otherwise
    /// switch to it (reusing it when already open) or connect.
    fn toggle_or_connect(&mut self, conn_id: &str, cx: &mut Context<Self>) {
        let is_active = self.connections.active_connection_id.as_deref() == Some(conn_id);
        if self.connections.active().driver.is_some() && is_active {
            self.connections.navigator_mut().nav_connection_expanded =
                !self.connections.navigator().nav_connection_expanded;
        } else if !self.form.is_connecting {
            self.connect_saved_by_id(conn_id, cx);
        }
    }
}

/// Expand/collapse arrow of a navigator node. It is the only click target that
/// toggles a node; the row itself handles selection and double-click.
#[allow(clippy::too_many_arguments)]
fn nav_chevron(
    id: impl Into<SharedString>,
    glyph: &'static str,
    width: f32,
    text_size: f32,
    color: Rgba,
    hover_color: Rgba,
    cx: &mut Context<Workspace>,
    on_toggle: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(id.into()))
        .flex()
        .justify_center()
        .w(px(width))
        .text_size(px(text_size))
        .text_color(color)
        .hover(move |s| s.text_color(hover_color))
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            on_toggle(this, cx);
            cx.notify();
        }))
        .child(glyph)
}

fn toggle_key(set: &mut HashSet<String>, key: &str) {
    if !set.remove(key) {
        set.insert(key.to_string());
    }
}

mod connection;
mod details;
mod schema;
pub(super) use schema::RelationGroup;
mod table;

pub(super) fn table_folder_key(schema: &str) -> String {
    schema::table_folder_key(schema)
}
