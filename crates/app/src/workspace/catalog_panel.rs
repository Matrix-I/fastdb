use super::metadata_state::MetadataState;
use super::script_tools::tool;
use super::*;
use db_wire::catalog::{CatalogRow, CatalogSection};
use db_wire::metadata::{MetadataFeature, MetadataSupport};
use gpui::prelude::FluentBuilder;

#[derive(Clone)]
pub(super) enum CatalogState {
    Loading,
    Ready(Arc<Vec<CatalogRow>>),
    Unavailable(MetadataSupport),
    Failed(MetadataState),
}
#[derive(Default)]
pub(super) struct CatalogCache {
    pub generation: u64,
    pub sections: HashMap<CatalogSection, CatalogState>,
}
impl CatalogCache {
    fn advance_generation(&mut self, generation: u64) {
        if self.generation != generation {
            self.generation = generation;
            self.sections.clear();
        }
    }
}
impl PropertySection {
    pub(super) fn catalog(self) -> Option<CatalogSection> {
        Some(match self {
            Self::Dependencies => CatalogSection::Dependencies,
            Self::References => CatalogSection::References,
            Self::Partitions => CatalogSection::Partitions,
            Self::Triggers => CatalogSection::Triggers,
            Self::Rules => CatalogSection::Rules,
            Self::Policies => CatalogSection::Policies,
            Self::Permissions => CatalogSection::Permissions,
            _ => return None,
        })
    }
}
impl Workspace {
    pub(super) fn ensure_catalog_section(&mut self, index: usize, cx: &mut Context<Self>) {
        let generation = self.tab_session(index).metadata_generation;
        let tab = &mut self.tabs[index];
        tab.catalog.advance_generation(generation);
        if let Some(section) = tab.active_property_section.catalog()
            && !tab.catalog.sections.contains_key(&section)
        {
            let id = tab.id;
            self.load_catalog_section(id, section, cx);
        }
    }
    fn load_catalog_section(&mut self, id: u64, section: CatalogSection, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let generation = self.tab_session(index).metadata_generation;
        let driver = self.tab_session(index).driver.clone();
        let tab = &mut self.tabs[index];
        let Some(driver) = driver else {
            tab.catalog.sections.insert(
                section,
                CatalogState::Failed(MetadataState::Failed(
                    "Disconnected; reconnect and Retry".into(),
                )),
            );
            return;
        };
        let support = driver
            .metadata_capabilities()
            .support(MetadataFeature::Catalog(section));
        if support != MetadataSupport::Available {
            tab.catalog
                .sections
                .insert(section, CatalogState::Unavailable(support));
            return;
        }
        let owner = tab.connection_id.clone().unwrap_or_default();
        let schema = tab.schema_name.clone().unwrap_or_default();
        let table = tab.table_name.clone().unwrap_or_default();
        tab.catalog.sections.insert(section, CatalogState::Loading);
        let expected = driver.clone();
        cx.spawn(async move |this, cx| {
            let result = db_wire::run_on_shared_runtime(async move {
                tokio::time::timeout(
                    std::time::Duration::from_secs(20),
                    driver.load_catalog_section(&schema, &table, section),
                )
                .await
                .map_err(|_| anyhow::anyhow!("Catalog load timed out"))?
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                let session = this.connections.session(&owner);
                if session.metadata_generation != generation
                    || !session
                        .driver
                        .as_ref()
                        .is_some_and(|d| Arc::ptr_eq(d, &expected))
                {
                    return;
                }
                let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) else {
                    return;
                };
                if tab.catalog.generation != generation {
                    return;
                }
                tab.catalog.sections.insert(
                    section,
                    match result {
                        Ok(Some(rows)) => CatalogState::Ready(Arc::new(rows)),
                        Ok(None) => CatalogState::Unavailable(MetadataSupport::NotImplemented),
                        Err(error) => CatalogState::Failed(MetadataState::from_error(&error)),
                    },
                );
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn render_catalog_section(
        &self,
        index: usize,
        section: CatalogSection,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tab = &self.tabs[index];
        let id = tab.id;
        let owner = tab.connection_id.clone().unwrap_or_default();
        let theme = &self.theme;
        let state = tab
            .catalog
            .sections
            .get(&section)
            .cloned()
            .unwrap_or(CatalogState::Loading);
        let mut panel = div()
            .flex()
            .flex_col()
            .flex_none()
            .w_full()
            .min_w(px(650.0))
            .text_size(px(11.5))
            .text_color(theme.text_primary)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_2()
                    .bg(theme.bg_grid_header)
                    .child(section.label())
                    .when(
                        !matches!(&state, CatalogState::Loading | CatalogState::Unavailable(_)),
                        |header| {
                            header.child(tool("Reload section").on_click(cx.listener(
                                move |this, _, _, cx| {
                                    if !this.tabs.iter().find(|t| t.id == id).is_some_and(|t| {
                                        matches!(
                                            t.catalog.sections.get(&section),
                                            Some(CatalogState::Loading)
                                        )
                                    }) {
                                        this.load_catalog_section(id, section, cx);
                                        cx.notify();
                                    }
                                },
                            )))
                        },
                    ),
            );
        match state {
            CatalogState::Loading => {
                panel = panel.child(div().p_4().child("Loading catalog metadata…"))
            }
            CatalogState::Unavailable(support) => {
                panel = panel.child(
                    div()
                        .p_4()
                        .child(support.explanation().unwrap_or("Metadata unavailable")),
                )
            }
            CatalogState::Failed(error) => {
                panel = panel.child(
                    div().p_4().text_color(rgb(0xf85149)).child(
                        error
                            .message()
                            .unwrap_or_else(|| "Metadata unavailable".into()),
                    ),
                )
            }
            CatalogState::Ready(rows) => {
                panel=panel.child(div().p_2().text_color(theme.text_secondary)
                    .child(if section==CatalogSection::Permissions{
                        "Stored/default ACLs only. Role membership, effective privileges and RLS access are not computed; this view is read-only."
                    }else{"Read-only catalog definitions. Dependency/reference lists are direct links, including internal catalog objects."}));
                if rows.is_empty() {
                    panel = panel.child(
                        div()
                            .p_4()
                            .child(format!("{} loaded: no objects found.", section.label())),
                    );
                }
                for (i, row) in rows.iter().enumerate() {
                    let definition = row.definition.clone();
                    let mut header = div()
                        .flex()
                        .gap_3()
                        .items_center()
                        .p_2()
                        .bg(theme.bg_surface)
                        .child(row.name.clone())
                        .child(row.kind.clone())
                        .child(
                            tool("Copy source").on_click(cx.listener(move |_, _, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    definition.clone(),
                                ));
                            })),
                        );
                    if let Some((schema, table)) = &row.relation {
                        let schema = schema.clone();
                        let table = table.clone();
                        let owner = owner.clone();
                        header = header.child(tool("Open relation").on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.query_table(&owner, &schema, &table, cx);
                            },
                        )));
                    }
                    let mut entry = div()
                        .id(("catalog-entry", i))
                        .flex()
                        .flex_col()
                        .flex_none()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(header);
                    for line in row.definition.lines() {
                        entry = entry.child(
                            div()
                                .px_3()
                                .py_1()
                                .font_family("monospace")
                                .child(line.to_string()),
                        );
                    }
                    panel = panel.child(entry);
                }
            }
        }
        panel.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{CatalogCache, CatalogState};
    use db_wire::catalog::CatalogSection;
    use std::sync::Arc;
    #[test]
    fn refresh_invalidates_ready_and_inflight_sections() {
        let mut cache = CatalogCache::default();
        cache.sections.insert(
            CatalogSection::Rules,
            CatalogState::Ready(Arc::new(Vec::new())),
        );
        cache
            .sections
            .insert(CatalogSection::Policies, CatalogState::Loading);
        cache.advance_generation(0);
        assert_eq!(cache.sections.len(), 2);
        cache.advance_generation(1);
        assert!(cache.sections.is_empty());
        assert_eq!(cache.generation, 1);
    }
}
