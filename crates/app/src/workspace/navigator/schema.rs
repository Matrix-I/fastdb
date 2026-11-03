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
            for group in RelationGroup::ALL {
                let key = group.expansion_key(schema_name);
                let expanded = self
                    .connections
                    .navigator()
                    .nav_tables_expanded
                    .contains(&key);
                let toggle = key.clone();
                nav_tree = nav_tree.child(
                    div()
                        .id(ElementId::Name(format!("nav-relations-{key}").into()))
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
                                    format!("nav-chev-relations-{key}"),
                                    if expanded { "▾" } else { "▸" },
                                    12.0,
                                    10.0,
                                    theme.text_muted,
                                    theme.text_primary,
                                    cx,
                                    move |this, _| {
                                        toggle_key(
                                            &mut this
                                                .connections
                                                .navigator_mut()
                                                .nav_tables_expanded,
                                            &toggle,
                                        )
                                    },
                                ))
                                .child(img("icons/folder_table.svg").w(px(16.0)).h(px(16.0)))
                                .child(group.label()),
                        ),
                );
                if expanded {
                    let mut found = false;
                    for table in s_tables
                        .iter()
                        .filter(|t| RelationGroup::from_type(&t.table_type) == group)
                    {
                        found = true;
                        nav_tree = self.append_table_node(nav_tree, table, cx);
                    }
                    if !found {
                        nav_tree = nav_tree.child(
                            div()
                                .pl(px(78.0))
                                .py_1()
                                .text_size(px(11.0))
                                .text_color(theme.text_muted)
                                .child("No loaded objects in this category"),
                        );
                    }
                }
            }
        }

        nav_tree
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RelationGroup {
    Tables,
    Views,
    MaterializedViews,
    Other,
}
pub(super) fn table_folder_key(schema: &str) -> String {
    RelationGroup::Tables.expansion_key(schema)
}
impl RelationGroup {
    const ALL: [Self; 4] = [
        Self::Tables,
        Self::Views,
        Self::MaterializedViews,
        Self::Other,
    ];
    fn from_type(value: &str) -> Self {
        match value.trim().to_ascii_uppercase().as_str() {
            "BASE TABLE" | "TABLE" | "PARTITIONED TABLE" | "FOREIGN TABLE" => Self::Tables,
            "VIEW" => Self::Views,
            "MATERIALIZED VIEW" => Self::MaterializedViews,
            _ => Self::Other,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::Tables => "Tables",
            Self::Views => "Views",
            Self::MaterializedViews => "Materialized Views",
            Self::Other => "Other Relations",
        }
    }
    fn expansion_key(self, schema: &str) -> String {
        format!("{}:{}:{}", schema.len(), schema, self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::RelationGroup;
    #[test]
    fn relation_folders_preserve_native_kinds_and_independent_expansion() {
        use RelationGroup::*;
        for (kind, expected) in [
            ("BASE TABLE", Tables),
            ("table", Tables),
            ("PARTITIONED TABLE", Tables),
            ("VIEW", Views),
            (" materialized view ", MaterializedViews),
            ("provider extension", Other),
            ("", Other),
        ] {
            assert_eq!(RelationGroup::from_type(kind), expected);
        }
        let keys: std::collections::HashSet<_> = RelationGroup::ALL
            .into_iter()
            .map(|g| g.expansion_key("public"))
            .collect();
        assert_eq!(keys.len(), 4);
        let initial = crate::workspace::connection_state::NavigatorState::default();
        assert!(
            initial
                .nav_tables_expanded
                .contains(&Tables.expansion_key("public"))
        );
        assert!(
            !initial
                .nav_tables_expanded
                .contains(&Views.expansion_key("public"))
        );
        assert_ne!(
            Tables.expansion_key("6:public:Views"),
            Views.expansion_key("public")
        );
        assert_ne!(Views.expansion_key("a:b"), Views.expansion_key("a"));
    }
}
