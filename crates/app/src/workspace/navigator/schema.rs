use super::*;
impl Workspace {
    pub(super) fn append_schema_node(
        &self,
        mut nav_tree: Stateful<Div>,
        schema_name: &String,
        s_tables: &[TableSummary],
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let theme = &self.theme;

        let is_schema_exp = self
            .connections
            .navigator()
            .nav_schema_expanded
            .contains(schema_name);
        let s_name_clone = schema_name.clone();
        let schema_id = format!("nav-schema-{}", schema_name);

        // Level 4: Schema item (e.g. public)
        nav_tree = nav_tree.child(
            div()
                .id(ElementId::Name(schema_id.into()))
                .flex()
                .items_center()
                .h(px(22.0))
                .pl(px(50.0))
                .rounded_sm()
                .hover(|s| s.bg(theme.bg_surface_hover))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(11.5))
                        .child(nav_chevron(
                            format!("nav-chev-schema-{}", schema_name),
                            if is_schema_exp { "▾" } else { "▸" },
                            12.0,
                            10.0,
                            theme.text_muted,
                            theme.text_primary,
                            cx,
                            move |this, _| {
                                toggle_key(
                                    &mut this.connections.navigator_mut().nav_schema_expanded,
                                    &s_name_clone,
                                )
                            },
                        ))
                        .child(img("icons/schema.svg").w(px(16.0)).h(px(16.0)))
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(schema_name.clone()),
                        ),
                ),
        );

        if is_schema_exp {
            // Level 5: "Tables" Folder under this Schema
            let is_tables_exp = self
                .connections
                .navigator()
                .nav_tables_expanded
                .contains(schema_name);
            let s_name_for_tables = schema_name.clone();
            let tables_folder_id = format!("nav-tables-folder-{}", schema_name);

            nav_tree = nav_tree.child(
                div()
                    .id(ElementId::Name(tables_folder_id.into()))
                    .flex()
                    .items_center()
                    .h(px(22.0))
                    .pl(px(62.0))
                    .rounded_sm()
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .text_size(px(11.5))
                            .child(nav_chevron(
                                format!("nav-chev-tables-{}", schema_name),
                                if is_tables_exp { "▾" } else { "▸" },
                                12.0,
                                10.0,
                                theme.text_muted,
                                theme.text_primary,
                                cx,
                                move |this, _| {
                                    toggle_key(
                                        &mut this.connections.navigator_mut().nav_tables_expanded,
                                        &s_name_for_tables,
                                    )
                                },
                            ))
                            .child(img("icons/folder_table.svg").w(px(16.0)).h(px(16.0)))
                            .child("Tables"),
                    ),
            );

            if is_tables_exp {
                // Level 6: Table items under this schema
                for t in s_tables {
                    nav_tree = self.append_table_node(nav_tree, t, cx);
                }
            }
        }

        nav_tree
    }
}
