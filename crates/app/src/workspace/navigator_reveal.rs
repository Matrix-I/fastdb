use super::connection_state::NavigatorState;
use super::navigator::RelationGroup;
use super::*;

fn relation_for_tab<'a>(tab: &TabState, tables: &'a [TableSummary]) -> Option<&'a TableSummary> {
    if tab.is_script() {
        return None;
    }
    tables.iter().find(|table| {
        tab.schema_name.as_ref() == Some(&table.schema)
            && tab.table_name.as_ref() == Some(&table.name)
    })
}

fn reveal_relation(state: &mut NavigatorState, relation: &TableSummary) {
    state.nav_connection_expanded = true;
    state.nav_databases_expanded = true;
    state.nav_database_expanded = true;
    state.nav_schemas_expanded = true;
    state.nav_schema_expanded.insert(relation.schema.clone());
    state
        .nav_tables_expanded
        .insert(RelationGroup::from_type(&relation.table_type).expansion_key(&relation.schema));
    state.selected_relation = Some((relation.schema.clone(), relation.name.clone()));
}

impl Workspace {
    pub(super) fn active_reveal_target(&self) -> Option<(String, TableSummary)> {
        let tab = self.tabs.get(self.active_tab)?;
        let owner = tab.connection_id.as_ref()?;
        let session = self.connections.sessions.get(owner)?;
        session.driver.as_ref()?;
        relation_for_tab(tab, &session.tables).map(|relation| (owner.clone(), relation.clone()))
    }

    pub(super) fn reveal_active_object(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((owner, relation)) = self.active_reveal_target() else {
            return;
        };
        if !self.activate_live_connection(&owner, cx) {
            return;
        }
        reveal_relation(self.connections.navigator_mut(), &relation);
        self.layout.focus_data = false;
        let anchor = self.navigator_anchor.clone();
        let workspace = cx.entity().downgrade();
        // Expansion must lay out the target before ScrollAnchor reads its origin.
        window.on_next_frame(move |window, cx| {
            anchor.scroll_to(window, cx);
            window.on_next_frame(move |_, cx| {
                let _ = workspace.update(cx, |_, cx| cx.notify());
            });
        });
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{NavigatorState, RelationGroup, relation_for_tab, reveal_relation};
    use crate::workspace::TabState;
    use db_wire::TableSummary;
    use sql_intel::SchemaCache;
    use std::sync::Arc;
    use ui_core::theme::Theme;

    fn relation(schema: &str, kind: &str) -> TableSummary {
        TableSummary {
            schema: schema.into(),
            name: "orders".into(),
            table_type: kind.into(),
            estimated_rows: None,
        }
    }

    #[test]
    fn reveal_expands_native_folder_and_selects_qualified_identity() {
        for kind in ["BASE TABLE", "VIEW", "MATERIALIZED VIEW", "extension"] {
            let mut nav = NavigatorState::default();
            let target = relation("sales", kind);
            reveal_relation(&mut nav, &target);
            assert!(
                nav.nav_connection_expanded
                    && nav.nav_databases_expanded
                    && nav.nav_database_expanded
                    && nav.nav_schemas_expanded
            );
            assert!(nav.nav_schema_expanded.contains("sales"));
            assert!(
                nav.nav_tables_expanded
                    .contains(&RelationGroup::from_type(kind).expansion_key("sales"))
            );
            assert_eq!(
                nav.selected_relation,
                Some(("sales".into(), "orders".into()))
            );
            reveal_relation(&mut nav, &relation("archive", kind));
            assert_eq!(
                nav.selected_relation,
                Some(("archive".into(), "orders".into()))
            );
            assert!(nav.nav_schema_expanded.contains("sales"));
        }
    }

    #[test]
    fn resolves_same_named_relations_by_schema_and_rejects_missing_objects_and_scripts() {
        let tables = [relation("sales", "VIEW"), relation("archive", "BASE TABLE")];
        let mut tab = TabState::table(
            1,
            "archive",
            "orders",
            Theme::dark(),
            Arc::new(SchemaCache::new().unwrap()),
        );
        assert_eq!(
            relation_for_tab(&tab, &tables).unwrap().table_type,
            "BASE TABLE"
        );
        tab.schema_name = Some("missing".into());
        assert!(relation_for_tab(&tab, &tables).is_none());
        tab.schema_name = Some("sales".into());
        tab.table_name = None;
        assert!(tab.is_script());
        assert!(relation_for_tab(&tab, &tables).is_none());
    }
}
