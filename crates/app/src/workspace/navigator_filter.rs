use super::*;
use gpui::prelude::FluentBuilder;

pub(super) const RESULT_LIMIT: usize = 100;

#[derive(Clone)]
struct RelationHit {
    schema: String,
    name: String,
    kind: String,
}

pub(super) struct NavigatorFilter {
    pub(super) mode: NavigatorFilterMode,
    pub(super) query: String,
    pub(super) caret: usize,
    pub(super) anchor: Option<usize>,
    pub(super) history: LineHistory,
    pub(super) menu_open: bool,
    pub(super) menu_selected: usize,
    pub(super) field_focused: bool,
    pub(super) files: Vec<ScriptFile>,
    pub(super) files_loading: bool,
    pub(super) files_request: u64,
    files_task: Option<Task<()>>,
    relations: Vec<RelationHit>,
    relations_loading: bool,
    relations_request: u64,
    relations_task: Option<Task<()>>,
    relations_owner: Option<String>,
    relations_generation: Option<u64>,
}

impl Default for NavigatorFilter {
    fn default() -> Self {
        Self {
            mode: NavigatorFilterMode::default(),
            query: String::new(),
            caret: usize::MAX,
            anchor: None,
            history: LineHistory::new(),
            menu_open: false,
            menu_selected: 2,
            field_focused: false,
            files: Vec::new(),
            files_loading: false,
            files_request: 0,
            files_task: None,
            relations: Vec::new(),
            relations_loading: false,
            relations_request: 0,
            relations_task: None,
            relations_owner: None,
            relations_generation: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum NavigatorFilterMode {
    Connections,
    CatalogsAndSchemas,
    #[default]
    TablesAndObjects,
    Files,
}

impl NavigatorFilterMode {
    pub(super) const ALL: [Self; 4] = [
        Self::Connections,
        Self::CatalogsAndSchemas,
        Self::TablesAndObjects,
        Self::Files,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Connections => "Filter connections",
            Self::CatalogsAndSchemas => "Filter catalogs and schemas",
            Self::TablesAndObjects => "Filter tables, procedures, etc.",
            Self::Files => "Filter files",
        }
    }

    fn placeholder(self) -> &'static str {
        match self {
            Self::Connections => "Search connections…",
            Self::CatalogsAndSchemas => "Search catalogs and schemas…",
            Self::TablesAndObjects => "Search database objects…",
            Self::Files => "Search saved SQL files…",
        }
    }

    pub(super) fn tooltip(self) -> &'static str {
        match self {
            Self::Connections => "Filter connections by name (click to choose a filter)",
            Self::CatalogsAndSchemas => {
                "Filter catalogs and schemas by name (click to choose a filter)"
            }
            Self::TablesAndObjects => {
                "Filter tables and database objects by name (click to choose a filter)"
            }
            Self::Files => "Filter saved SQL files by name (click to choose a filter)",
        }
    }
}

pub(super) fn matches_text(query: &str, parts: &[&str]) -> bool {
    let query = query.trim();
    query.is_empty()
        || parts
            .iter()
            .any(|part| part.to_lowercase().contains(&query.to_lowercase()))
}

fn active_database_name<'a>(
    connections: impl IntoIterator<Item = (&'a str, &'a str)>,
    active_id: Option<&str>,
) -> &'a str {
    active_id
        .and_then(|active_id| {
            connections
                .into_iter()
                .find(|(connection_id, _)| *connection_id == active_id)
                .map(|(_, database)| database)
        })
        .unwrap_or_default()
}

fn relation_snapshot_is_current(
    active_id: Option<&str>,
    owner: &str,
    current_generation: Option<u64>,
    expected_generation: u64,
    same_schema_tree: bool,
) -> bool {
    active_id == Some(owner) && current_generation == Some(expected_generation) && same_schema_tree
}

pub(super) fn matching_files<'a>(files: &'a [ScriptFile], query: &str) -> Vec<&'a ScriptFile> {
    files
        .iter()
        .filter(|file| matches_text(query, &[&file.name]))
        .take(RESULT_LIMIT)
        .collect()
}

struct NavigatorFilterTooltip(String);

impl Render for NavigatorFilterTooltip {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_sm()
            .bg(rgb(0x21262d))
            .border_1()
            .border_color(rgb(0x30363d))
            .shadow_lg()
            .text_color(rgb(0xf0f6fc))
            .text_size(px(11.5))
            .child(self.0.clone())
    }
}

impl Workspace {
    pub(super) fn navigator_relation_filter_active(&self) -> bool {
        self.navigator_filter.mode == NavigatorFilterMode::TablesAndObjects
            && !self.navigator_filter.query.trim().is_empty()
    }

    pub(super) fn navigator_relation_search_status(&self) -> Option<&'static str> {
        if !self.navigator_relation_filter_active() {
            return None;
        }
        if self.navigator_filter.relations_loading {
            Some("Searching database objects…")
        } else if self.navigator_filter.relations.is_empty() {
            Some("No matching database objects")
        } else {
            None
        }
    }

    pub(super) fn navigator_relation_matches(&self, schema: &str, name: &str) -> bool {
        !self.navigator_relation_filter_active()
            || self
                .navigator_filter
                .relations
                .iter()
                .any(|hit| hit.schema == schema && hit.name == name)
    }

    fn set_navigator_filter_mode(&mut self, mode: NavigatorFilterMode, cx: &mut Context<Self>) {
        let filter = &mut self.navigator_filter;
        filter.mode = mode;
        filter.menu_open = false;
        filter.menu_selected = NavigatorFilterMode::ALL
            .iter()
            .position(|candidate| *candidate == mode)
            .unwrap_or_default();
        filter.field_focused = true;
        if mode == NavigatorFilterMode::Files {
            self.refresh_navigator_files(cx);
        } else if let Some(task) = filter.files_task.take() {
            drop(task);
            filter.files_loading = false;
        }
        if mode == NavigatorFilterMode::TablesAndObjects {
            self.refresh_navigator_relations(cx);
        } else if let Some(task) = self.navigator_filter.relations_task.take() {
            drop(task);
            self.navigator_filter.relations_loading = false;
        }
        cx.notify();
    }

    fn refresh_navigator_files(&mut self, cx: &mut Context<Self>) {
        let filter = &mut self.navigator_filter;
        if let Some(task) = filter.files_task.take() {
            drop(task);
        }
        filter.files_request = filter.files_request.wrapping_add(1);
        filter.files_loading = true;
        filter.files.clear();
        let request = filter.files_request;
        let query = filter.query.clone();
        let store = self.script_store.clone();
        let task = cx.spawn(async move |this, cx| {
            let files = cx
                .background_executor()
                .spawn(async move { store.search_files(&query, RESULT_LIMIT) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let filter = &mut this.navigator_filter;
                if filter.files_request == request && filter.mode == NavigatorFilterMode::Files {
                    filter.files = files;
                    filter.files_loading = false;
                    filter.files_task = None;
                    cx.notify();
                }
            });
        });
        self.navigator_filter.files_task = Some(task);
    }

    pub(super) fn refresh_navigator_relations(&mut self, cx: &mut Context<Self>) {
        let filter = &mut self.navigator_filter;
        if let Some(task) = filter.relations_task.take() {
            drop(task);
        }
        filter.relations_request = filter.relations_request.wrapping_add(1);
        filter.relations.clear();
        let request = filter.relations_request;
        let Some(owner) = self.connections.active_connection_id.clone() else {
            filter.relations.clear();
            filter.relations_loading = false;
            filter.relations_owner = None;
            filter.relations_generation = None;
            return;
        };
        let session = self.connections.active();
        if session.driver.is_none() {
            filter.relations.clear();
            filter.relations_loading = false;
            filter.relations_owner = Some(owner);
            filter.relations_generation = Some(session.metadata_generation);
            return;
        }
        let tree = session.schema_tree.clone();
        let generation = session.metadata_generation;
        let query = filter.query.clone();
        filter.relations_loading = true;
        filter.relations_owner = Some(owner.clone());
        filter.relations_generation = Some(generation);
        let task = cx.spawn(async move |this, cx| {
            let tree_for_check = tree.clone();
            let relations = cx
                .background_executor()
                .spawn(async move {
                    let mut matches = Vec::new();
                    for (schema, tables) in tree.iter() {
                        for table in tables {
                            if matches_text(&query, &[schema, &table.name, &table.table_type]) {
                                matches.push(RelationHit {
                                    schema: schema.clone(),
                                    name: table.name.clone(),
                                    kind: table.table_type.clone(),
                                });
                                if matches.len() >= RESULT_LIMIT {
                                    return matches;
                                }
                            }
                        }
                    }
                    matches
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let current = this.connections.sessions.get(&owner);
                let metadata_is_current = current.is_some_and(|session| {
                    relation_snapshot_is_current(
                        this.connections.active_connection_id.as_deref(),
                        &owner,
                        Some(session.metadata_generation),
                        generation,
                        Arc::ptr_eq(&session.schema_tree, &tree_for_check),
                    )
                });
                let filter = &mut this.navigator_filter;
                if filter.relations_request == request
                    && filter.mode == NavigatorFilterMode::TablesAndObjects
                {
                    filter.relations = if metadata_is_current {
                        relations
                    } else {
                        Vec::new()
                    };
                    filter.relations_loading = false;
                    filter.relations_task = None;
                    cx.notify();
                }
            });
        });
        self.navigator_filter.relations_task = Some(task);
    }

    pub(super) fn navigator_filter_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) -> bool {
        let filter = &mut self.navigator_filter;
        if filter.menu_open {
            match key.key.as_str() {
                "escape" => filter.menu_open = false,
                "up" => {
                    filter.menu_selected = filter.menu_selected.saturating_sub(1);
                }
                "down" => {
                    filter.menu_selected = (filter.menu_selected + 1)
                        .min(NavigatorFilterMode::ALL.len().saturating_sub(1));
                }
                "enter" | "return" => {
                    let mode = NavigatorFilterMode::ALL[filter.menu_selected];
                    self.set_navigator_filter_mode(mode, cx);
                    return true;
                }
                _ => {}
            }
            cx.notify();
            return true;
        }
        if !filter.field_focused {
            return false;
        }
        if key.key == "escape" {
            if !ui_editor::line_input::clear_selection(
                &mut filter.caret,
                &mut filter.anchor,
                filter.query.chars().count(),
            ) {
                filter.field_focused = false;
            }
            cx.notify();
            return true;
        }
        if key.key == "enter" || key.key == "return" {
            if filter.mode == NavigatorFilterMode::TablesAndObjects {
                let query = filter.query.clone();
                filter.field_focused = false;
                if query.trim().is_empty() {
                    self.open_object_browser(cx);
                } else {
                    self.open_object_search(&query, cx);
                }
            }
            return true;
        }
        let before = filter.query.clone();
        handle_line_key(
            LineInput {
                text: &mut filter.query,
                caret: &mut filter.caret,
                anchor: &mut filter.anchor,
                history: &mut filter.history,
                digits_only: false,
            },
            key,
            cx,
        );
        if filter.query != before {
            match filter.mode {
                NavigatorFilterMode::Files => self.refresh_navigator_files(cx),
                NavigatorFilterMode::TablesAndObjects => self.refresh_navigator_relations(cx),
                _ => {}
            }
        }
        cx.notify();
        true
    }

    pub(super) fn render_navigator_filter(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let filter = &self.navigator_filter;
        let theme = &self.theme;
        let weak = cx.entity().downgrade();
        let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                let filter = &mut this.navigator_filter;
                filter.field_focused = true;
                apply_pick(&mut filter.caret, &mut filter.anchor, index, kind, true);
                cx.notify();
            });
        });
        let active_mode = filter.mode;
        let mut row = div()
            .id("navigator-search")
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .h(px(30.0))
            .w_full()
            .text_size(px(11.5))
            .text_color(theme.text_primary)
            .child(
                ui_editor::line_input::pick_surface(
                    div()
                        .flex()
                        .flex_1()
                        .min_w_0()
                        .items_center()
                        .h(px(28.0))
                        .px_2()
                        .rounded_xs()
                        .border_1()
                        .border_color(if filter.field_focused {
                            theme.border_focused
                        } else {
                            theme.border
                        })
                        .bg(theme.bg_editor),
                    usize::MAX,
                    &pick,
                )
                .id("navigator-search-input")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.navigator_filter.field_focused = true;
                        this.navigator_filter.menu_open = false;
                        cx.notify();
                    }),
                )
                .on_click(cx.listener(|_, _, _, cx| cx.stop_propagation()))
                .child(render_field_content(
                    &filter.query,
                    filter.mode.placeholder(),
                    filter.field_focused,
                    filter.anchor,
                    filter.caret,
                    true,
                    theme,
                    &pick,
                )),
            )
            .child(
                div()
                    .id("navigator-filter-mode")
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .w(px(34.0))
                    .h(px(28.0))
                    .rounded_xs()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .tooltip(move |_, cx| {
                        cx.new(|_| NavigatorFilterTooltip(active_mode.tooltip().to_owned()))
                            .into()
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.navigator_filter.menu_open = !this.navigator_filter.menu_open;
                        this.navigator_filter.field_focused = false;
                        cx.notify();
                    }))
                    .child(img("icons/filter.svg").w(px(16.0)).h(px(16.0)))
                    .child("▾"),
            );
        if filter.menu_open {
            let mut menu = div()
                .id("navigator-filter-options")
                .occlude()
                .flex()
                .flex_col()
                .absolute()
                .top(px(30.0))
                .right(px(0.0))
                .w(px(232.0))
                .p_1()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.bg_surface)
                .shadow_lg();
            for (index, mode) in NavigatorFilterMode::ALL.into_iter().enumerate() {
                let selected = filter.mode == mode;
                let hovered = filter.menu_selected == index;
                menu = menu.child(
                    div()
                        .id(ElementId::Name(
                            format!("navigator-filter-option-{index}").into(),
                        ))
                        .flex()
                        .items_center()
                        .gap_2()
                        .h(px(28.0))
                        .px_2()
                        .rounded_xs()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.bg_surface_hover))
                        .when(hovered, |item| item.bg(theme.bg_surface_hover))
                        .text_color(theme.text_primary)
                        .child(if selected { "✓" } else { " " })
                        .child(mode.label())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.set_navigator_filter_mode(mode, cx);
                        })),
                );
            }
            row = row.child(deferred(menu).with_priority(1));
        }
        row
    }

    pub(super) fn render_navigator_filter_results(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let filter = &self.navigator_filter;
        let query = filter.query.trim();
        let theme = &self.theme;
        let mut results = div()
            .id("navigator-search-results")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_0p5();

        match filter.mode {
            NavigatorFilterMode::Connections => {
                let matching: Vec<_> = self
                    .connections
                    .saved_connections
                    .iter()
                    .filter(|connection| matches_text(query, &[&connection.name]))
                    .take(RESULT_LIMIT)
                    .collect();
                for &connection in &matching {
                    let id = connection.id.clone();
                    results = results.child(
                        div()
                            .id(ElementId::Name(
                                format!("navigator-search-connection-{id}").into(),
                            ))
                            .flex()
                            .items_center()
                            .gap_2()
                            .h(px(26.0))
                            .px_2()
                            .rounded_xs()
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.bg_surface_hover))
                            .child(connection.name.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.navigator_filter.field_focused = false;
                                this.connect_saved_by_id(&id, cx);
                            })),
                    );
                }
                if matching.is_empty() {
                    results = results.child(search_empty_message(query, "No matching connections"));
                }
            }
            NavigatorFilterMode::CatalogsAndSchemas => {
                if self.connections.active_connection_id.is_none() {
                    return results
                        .child(search_empty_message(query, "Connect to a database first"));
                }
                let session = self.connections.active();
                let mut found = false;
                let active_database = active_database_name(
                    self.connections
                        .saved_connections
                        .iter()
                        .map(|connection| (connection.id.as_str(), connection.database.as_str())),
                    self.connections.active_connection_id.as_deref(),
                );
                if matches_text(query, &[active_database]) {
                    found = true;
                    let db = active_database.to_owned();
                    results = results.child(search_result_row("Database", &db, theme).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.navigator_filter.field_focused = false;
                            this.connections.navigator_mut().nav_connection_expanded = true;
                            this.connections.navigator_mut().nav_databases_expanded = true;
                            this.connections.navigator_mut().nav_database_expanded = true;
                            cx.notify();
                        }),
                    ));
                }
                for schema in session
                    .schema_tree
                    .keys()
                    .filter(|schema| matches_text(query, &[schema.as_str()]))
                    .take(RESULT_LIMIT)
                {
                    found = true;
                    let schema = schema.clone();
                    results = results.child(search_result_row("Schema", &schema, theme).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.navigator_filter.field_focused = false;
                            let nav = this.connections.navigator_mut();
                            nav.nav_connection_expanded = true;
                            nav.nav_databases_expanded = true;
                            nav.nav_database_expanded = true;
                            nav.nav_schemas_expanded = true;
                            nav.nav_schema_expanded.insert(schema.clone());
                            cx.notify();
                        }),
                    ));
                }
                if !found {
                    results = results.child(search_empty_message(
                        query,
                        "No matching catalogs or schemas",
                    ));
                }
            }
            NavigatorFilterMode::TablesAndObjects => {
                if query.is_empty() && self.connections.active().driver.is_some() {
                    results = results.child(
                        search_result_row("Catalog", "Browse catalog objects…", theme).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.navigator_filter.field_focused = false;
                                this.open_object_browser(cx);
                            }),
                        ),
                    );
                }
                let Some(connection_id) = self.connections.active_connection_id.clone() else {
                    return results
                        .child(search_empty_message(query, "Connect to a database first"));
                };
                if self.connections.active().driver.is_none() {
                    return results
                        .child(search_empty_message(query, "Connect to a database first"));
                }
                let metadata_current = filter.relations_owner.as_ref() == Some(&connection_id)
                    && filter.relations_generation
                        == Some(self.connections.active().metadata_generation);
                if filter.relations_loading {
                    results =
                        results.child(search_empty_message(query, "Searching database objects…"));
                } else if !metadata_current {
                    results = results.child(search_empty_message(
                        query,
                        "Metadata changed; select this filter again",
                    ));
                } else {
                    for relation in &filter.relations {
                        let title = format!("{}.{}", relation.schema, relation.name);
                        let schema = relation.schema.clone();
                        let name = relation.name.clone();
                        let connection_id = connection_id.clone();
                        results = results.child(
                            search_result_row(&relation.kind, &title, theme).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.navigator_filter.field_focused = false;
                                    this.query_table(&connection_id, &schema, &name, cx);
                                },
                            )),
                        );
                    }
                    if filter.relations.is_empty() {
                        results = results
                            .child(search_empty_message(query, "No matching database objects"));
                    }
                }
            }
            NavigatorFilterMode::Files => {
                let matching = matching_files(&filter.files, query);
                for &script in &matching {
                    let script = script.clone();
                    let connection = self
                        .connections
                        .saved_connections
                        .iter()
                        .find(|connection| connection.id == script.connection_id)
                        .map_or_else(
                            || script.connection_id.clone(),
                            |connection| connection.name.clone(),
                        );
                    let title = format!("{} · {connection}", script.name);
                    results = results.child(search_result_row("SQL file", &title, theme).on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.navigator_filter.field_focused = false;
                            this.open_script_file(&script, cx);
                        }),
                    ));
                }
                if filter.files_loading {
                    results =
                        results.child(search_empty_message(query, "Searching saved SQL files…"));
                } else if filter.files.is_empty() {
                    results = results.child(search_empty_message(query, "No saved SQL files"));
                } else if matching.is_empty() {
                    results = results.child(search_empty_message(query, "No matching SQL files"));
                }
            }
        }
        results
    }
}

fn search_result_row(kind: &str, title: &str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(ElementId::Name(
            format!("navigator-result-{kind}-{title}").into(),
        ))
        .flex()
        .flex_col()
        .justify_center()
        .h(px(32.0))
        .px_2()
        .rounded_xs()
        .cursor_pointer()
        .hover(|s| s.bg(theme.bg_surface_hover))
        .child(
            div()
                .text_size(px(11.5))
                .text_color(theme.text_primary)
                .child(title.to_owned()),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(theme.text_muted)
                .child(kind.to_owned()),
        )
}

fn search_empty_message(query: &str, message: &str) -> Div {
    div()
        .p_2()
        .text_size(px(11.0))
        .text_color(rgb(0x8b949e))
        .child(if query.is_empty() {
            message.to_owned()
        } else {
            format!("{message}: {query}")
        })
}

#[cfg(test)]
mod tests {
    use super::{
        NavigatorFilterMode, active_database_name, matches_text, relation_snapshot_is_current,
    };

    #[test]
    fn filter_modes_have_distinct_labels_and_tooltips() {
        let labels: std::collections::HashSet<_> = NavigatorFilterMode::ALL
            .into_iter()
            .map(NavigatorFilterMode::label)
            .collect();
        assert_eq!(labels.len(), 4);
        for mode in NavigatorFilterMode::ALL {
            assert!(mode.tooltip().contains("click"));
        }
    }

    #[test]
    fn matching_is_trimmed_case_insensitive_and_checks_any_type_field() {
        assert!(matches_text("  pUbLiC ", &["sales", "public"]));
        assert!(matches_text("view", &["sales", "monthly_summary", "VIEW"]));
        assert!(matches_text("", &["anything"]));
        assert!(!matches_text("absent", &["sales", "public"]));
    }

    #[test]
    fn catalog_filter_uses_the_active_connections_database() {
        let connections = [("first", "warehouse"), ("second", "analytics")];
        assert_eq!(
            active_database_name(connections.iter().copied(), Some("second")),
            "analytics"
        );
        assert_eq!(
            active_database_name(connections.iter().copied(), Some("missing")),
            ""
        );
    }

    #[test]
    fn relation_results_require_the_same_active_connection_and_metadata_snapshot() {
        assert!(relation_snapshot_is_current(
            Some("db-a"),
            "db-a",
            Some(3),
            3,
            true
        ));
        assert!(!relation_snapshot_is_current(
            Some("db-b"),
            "db-a",
            Some(3),
            3,
            true
        ));
        assert!(!relation_snapshot_is_current(
            Some("db-a"),
            "db-a",
            Some(4),
            3,
            true
        ));
    }
}
