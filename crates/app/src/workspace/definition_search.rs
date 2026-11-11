mod data;

use super::script_tools::tool;
use super::*;
use db_wire::objects::{
    DEFINITION_INPUT_LIMIT, DEFINITION_SEARCH_LIMIT, DefinitionHit, DefinitionKind,
};
use gpui::prelude::FluentBuilder;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Schema,
    TableFilter,
    Needle,
}

struct Input {
    text: String,
    caret: usize,
    anchor: Option<usize>,
    history: LineHistory,
}

impl Input {
    fn new(text: &str) -> Self {
        Self {
            text: text.into(),
            caret: text.chars().count(),
            anchor: None,
            history: LineHistory::new(),
        }
    }

    fn edit(&mut self, key: &Keystroke, cx: &mut Context<Workspace>) {
        handle_line_key(
            LineInput {
                text: &mut self.text,
                caret: &mut self.caret,
                anchor: &mut self.anchor,
                history: &mut self.history,
                digits_only: false,
            },
            key,
            cx,
        );
    }
}

pub(super) struct DefinitionSearch {
    owner: String,
    driver: Arc<dyn DatabaseDriver>,
    generation: u64,
    schema: Input,
    needle: Input,
    field: Field,
    message: String,
    hits: Vec<DefinitionHit>,
    data: Option<data::TableSearch>,
    source: Option<(SharedString, SharedString)>,
    cancel: Option<flume::Sender<()>>,
    task: Option<Task<()>>,
}

impl DefinitionSearch {
    fn new(owner: String, driver: Arc<dyn DatabaseDriver>, generation: u64) -> Self {
        Self {
            owner,
            driver,
            generation,
            schema: Input::new("public"),
            needle: Input::new(""),
            field: Field::Needle,
            message: "Enter text to search object definitions.".into(),
            hits: Vec::new(),
            data: None,
            source: None,
            cancel: None,
            task: None,
        }
    }

    fn complete(
        &mut self,
        connections: &connection_state::ConnectionManager,
        result: anyhow::Result<Option<Vec<DefinitionHit>>>,
    ) {
        self.task = None;
        let cancelled = self.cancel.take().is_none();
        self.message = if !self.is_current(connections) {
            "Connection or catalog changed. Reopen search.".into()
        } else {
            match result {
                Ok(_) if cancelled => "Definition search cancelled.".into(),
                Ok(Some(mut hits)) => {
                    let message = match hits.len() {
                        0 => "No matching definitions in this schema.".into(),
                        count if count > DEFINITION_SEARCH_LIMIT => {
                            format!(
                                "First {DEFINITION_SEARCH_LIMIT} matches. Refine the search for more."
                            )
                        }
                        count => format!(
                            "{count} matching definitions. Select a match to inspect its source."
                        ),
                    };
                    hits.truncate(DEFINITION_SEARCH_LIMIT);
                    self.hits = hits;
                    message
                }
                Ok(None) => "Definition search is unavailable for this provider.".into(),
                Err(error) => error.to_string(),
            }
        };
    }

    fn is_current(&self, connections: &connection_state::ConnectionManager) -> bool {
        let session = connections.active();
        connections.active_connection_id.as_ref() == Some(&self.owner)
            && session.metadata_generation == self.generation
            && session
                .driver
                .as_ref()
                .is_some_and(|driver| Arc::ptr_eq(driver, &self.driver))
    }

    fn input_mut(&mut self, field: Field) -> &mut Input {
        match field {
            Field::Schema => &mut self.schema,
            Field::TableFilter => {
                &mut self
                    .data
                    .as_mut()
                    .expect("table picker requires data mode")
                    .filter
            }
            Field::Needle => &mut self.needle,
        }
    }
}

fn hit_title(hit: &DefinitionHit, provider: DbDriverKind) -> String {
    let object = &hit.object;
    let identity = format!(
        "{}.{}",
        db_wire::sql::quote_ident(provider, &object.schema),
        db_wire::sql::quote_ident(provider, &object.name)
    );
    let label = match hit.kind {
        DefinitionKind::Table => "Table",
        DefinitionKind::View => "View",
        DefinitionKind::MaterializedView => "Materialized view",
        DefinitionKind::Routine if provider == DbDriverKind::MySql => {
            let label = if object.qualifier == "PROCEDURE" {
                "Procedure"
            } else {
                "Function"
            };
            return format!("{label}  {identity}");
        }
        DefinitionKind::Routine => return format!("Routine  {identity}({})", object.qualifier),
        DefinitionKind::Trigger => "Trigger",
        DefinitionKind::Rule => "Rule",
        DefinitionKind::Index => "Index",
        DefinitionKind::Constraint => "Constraint",
        DefinitionKind::Policy => "Policy",
        DefinitionKind::Statistics => "Statistics",
    };
    if object.qualifier.is_empty() {
        format!("{label}  {identity}")
    } else {
        let relation = db_wire::sql::quote_ident(provider, &object.qualifier);
        format!("{label}  {identity} on {relation}")
    }
}

impl Workspace {
    pub(super) fn open_definition_search(&mut self, cx: &mut Context<Self>) {
        let session = self.connections.active();
        let (Some(owner), Some(driver)) = (&self.connections.active_connection_id, &session.driver)
        else {
            return;
        };
        if !matches!(
            session.kind,
            DbDriverKind::Postgres | DbDriverKind::Sqlite | DbDriverKind::MySql
        ) {
            return;
        }
        let mut search =
            DefinitionSearch::new(owner.clone(), driver.clone(), session.metadata_generation);
        match session.kind {
            DbDriverKind::Sqlite => search.schema = Input::new("main"),
            DbDriverKind::MySql => {
                let database = self
                    .connections
                    .saved_connections
                    .iter()
                    .find(|connection| &connection.id == owner)
                    .map_or("", |connection| connection.database.as_str());
                search.schema = Input::new(database);
            }
            _ => {}
        }
        self.definition_search = Some(search);
        cx.notify();
    }

    fn run_definition_search(&mut self, cx: &mut Context<Self>) {
        let Some(search) = &mut self.definition_search else {
            return;
        };
        if search.data.is_some() {
            self.run_table_data_search(cx);
            return;
        }
        if search.task.is_some() {
            return;
        }
        search.hits.clear();
        search.source = None;
        if !search.is_current(&self.connections) {
            search.message = "Connection or catalog changed. Reopen search.".into();
        } else if search.schema.text.is_empty() {
            search.message = "Enter an exact schema name.".into();
        } else if search.needle.text.trim().is_empty()
            || search.needle.text.len() > DEFINITION_INPUT_LIMIT
        {
            search.message =
                format!("Enter search text up to {DEFINITION_INPUT_LIMIT} UTF-8 bytes.");
        } else {
            let (cancel, signal) = flume::bounded(1);
            search.cancel = Some(cancel);
            search.message = "Searching definitions…".into();
            let (driver, schema, needle) = (
                search.driver.clone(),
                search.schema.text.clone(),
                search.needle.text.clone(),
            );
            search.task = Some(cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        db_wire::get_runtime()
                            .block_on(driver.search_definitions(&schema, &needle, signal))
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if let Some(search) = &mut this.definition_search {
                        search.complete(&this.connections, result);
                    }
                    cx.notify();
                });
            }));
        }
        cx.notify();
    }

    fn inspect_definition(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(search) = &mut self.definition_search else {
            return;
        };
        if !search.is_current(&self.connections) {
            search.hits.clear();
            search.source = None;
            search.message = "Connection or catalog changed. Reopen search.".into();
        } else if let Some(hit) = search.hits.get(index) {
            let source = hit
                .source
                .as_deref()
                .unwrap_or("Definition exceeds the 64 KiB preview limit.");
            search.source = Some((
                hit_title(hit, self.connections.active().kind).into(),
                source.to_owned().into(),
            ));
        }
        cx.notify();
    }

    pub(super) fn definition_search_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(search) = &mut self.definition_search else {
            return;
        };
        match key.key.as_str() {
            "escape" => self.definition_search = None,
            "w" if key.modifiers.platform || key.modifiers.control => self.definition_search = None,
            "enter" | "return" => self.run_definition_search(cx),
            "tab" if search.task.is_none() => {
                search.field = match (
                    search.field,
                    search.data.as_ref().map(data::TableSearch::uses_picker),
                ) {
                    (Field::Schema, Some(true)) => Field::TableFilter,
                    (Field::Needle, Some(false)) => Field::Needle,
                    (Field::Needle, _) => Field::Schema,
                    _ => Field::Needle,
                };
            }
            _ if search.task.is_none() => {
                let field = search.field;
                let input = search.input_mut(field);
                let previous = input.text.clone();
                let (caret, anchor) = (input.caret, input.anchor);
                input.edit(key, cx);
                if input.text.len() > DEFINITION_INPUT_LIMIT {
                    input.text = previous;
                    input.caret = caret;
                    input.anchor = anchor;
                    input.history = LineHistory::new();
                    search.message =
                        format!("Input is limited to {DEFINITION_INPUT_LIMIT} UTF-8 bytes.");
                } else if input.text != previous {
                    if let Some(data) = &mut search.data
                        && matches!(field, Field::Schema | Field::TableFilter)
                    {
                        let tables = self
                            .connections
                            .active()
                            .schema_tree
                            .get(&search.schema.text)
                            .map_or(&[][..], Vec::as_slice);
                        data.refresh_choices(tables);
                        if field == Field::Schema {
                            data.reset_selection();
                        }
                    }
                    if field != Field::TableFilter {
                        search.hits.clear();
                        if let Some(data) = &mut search.data {
                            data.hits.clear();
                            data.errors.clear();
                        }
                        search.source = None;
                        search.message = "Press Enter to search.".into();
                    }
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn render_definition_field(
        &self,
        search: &DefinitionSearch,
        field: Field,
        input: &Input,
        placeholder: &'static str,
        cx: &Context<Self>,
    ) -> Div {
        let weak = cx.entity().downgrade();
        let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(search) = &mut this.definition_search
                    && search.task.is_none()
                {
                    search.field = field;
                    let input = search.input_mut(field);
                    apply_pick(&mut input.caret, &mut input.anchor, index, kind, true);
                    cx.notify();
                }
            });
        });
        let focused = search.field == field && search.task.is_none();
        ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
            .p_2()
            .border_1()
            .border_color(if focused {
                self.theme.border_focused
            } else {
                self.theme.border
            })
            .child(render_field_content(
                &input.text,
                placeholder,
                focused,
                input.anchor,
                input.caret,
                true,
                &self.theme,
                &pick,
            ))
    }

    pub(super) fn render_definition_search(
        &self,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let search = self.definition_search.as_ref().unwrap();
        let current = search.is_current(&self.connections);
        let hits: Vec<String> = if !current {
            Vec::new()
        } else if let Some(data) = &search.data {
            data.hits
                .iter()
                .map(|row| format!("{} · {} · {}", row.table, row.hit.column, row.hit.preview))
                .collect()
        } else {
            search
                .hits
                .iter()
                .map(|hit| hit_title(hit, self.connections.active().kind))
                .collect()
        };
        let is_data = search.data.is_some();
        let picker = search
            .data
            .as_ref()
            .is_some_and(data::TableSearch::uses_picker);
        let source = search.source.clone().filter(|_| current);
        let failures = search
            .data
            .as_ref()
            .filter(|data| current && !data.errors.is_empty());
        let message = if current {
            search.message.as_str()
        } else {
            "Connection or catalog changed. Reopen search."
        };
        let rows = hits.into_iter().enumerate().map(|(index, hit)| {
            div()
                .id(("definition-hit", index))
                .p_1()
                .cursor_pointer()
                .hover(|row| row.bg(self.theme.bg_surface_hover))
                .child(hit)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if is_data {
                        this.open_data_search_match(index, cx);
                    } else {
                        this.inspect_definition(index, cx);
                    }
                }))
        });
        let action = if search.task.is_none() {
            tool("Search").on_click(cx.listener(|this, _, _, cx| this.run_definition_search(cx)))
        } else {
            tool("Cancel search").on_click(cx.listener(|this, _, _, cx| {
                if let Some(search) = &mut this.definition_search
                    && let Some(cancel) = search.cancel.take()
                {
                    drop(cancel);
                    search.message = "Cancelling search…".into();
                    cx.notify();
                }
            }))
        };
        let connection_name = self
            .connections
            .saved_connections
            .iter()
            .find(|connection| connection.id == search.owner)
            .map_or("Active connection", |connection| connection.name.as_str());
        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_4()
                    .w(px(640.))
                    .max_w_full()
                    .max_h_full()
                    .rounded_md()
                    .bg(self.theme.bg_surface)
                    .child(match search.data.as_ref().map(|data| &data.scope) {
                        Some(data::TableScope::Fixed(table)) => format!(
                            "Search table data · {connection_name} · {}.{}",
                            search.schema.text, table
                        ),
                        Some(data::TableScope::Picker(_)) => {
                            format!("Search table data · {connection_name}")
                        }
                        None => format!("Search definitions · {connection_name}"),
                    })
                    .when(!is_data || picker, |panel| {
                        panel
                            .child("Schema (exact name)")
                            .child(self.render_definition_field(
                                search,
                                Field::Schema,
                                &search.schema,
                                "Schema name",
                                cx,
                            ))
                    })
                    .when(picker && current, |panel| {
                        panel.child(self.render_data_scope(search, cx))
                    })
                    .child(self.render_definition_field(
                        search,
                        Field::Needle,
                        &search.needle,
                        if is_data {
                            "Cell contains…"
                        } else {
                            "Definition contains…"
                        },
                        cx,
                    ))
                    .child(message.to_owned())
                    .when_some(failures, |panel, data| {
                        panel.child(
                            div()
                                .id("data-search-errors")
                                .min_h_0()
                                .max_h(px(72.))
                                .overflow_y_scroll()
                                .text_color(self.theme.text_warning)
                                .child(format!("{} table requests failed", data.errors.len()))
                                .children(
                                    data.errors.iter().map(|error| div().child(error.clone())),
                                ),
                        )
                    })
                    .child(
                        div()
                            .id("definition-hits")
                            .min_h_0()
                            .max_h(px(160.))
                            .overflow_y_scroll()
                            .children(rows),
                    )
                    .when_some(source, |panel, (title, source)| {
                        panel.child(title).child(
                            div()
                                .id("definition-source")
                                .min_h_0()
                                .max_h(px(180.))
                                .overflow_y_scroll()
                                .font_family("Menlo")
                                .whitespace_normal()
                                .child(source),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(action)
                            .child(tool("Close").on_click(cx.listener(|this, _, _, cx| {
                                this.definition_search = None;
                                cx.notify();
                            }))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFINITION_SEARCH_LIMIT, DefinitionSearch, connection_state, hit_title};
    use db_wire::objects::{DefinitionHit, DefinitionKind, ObjectSummary};
    use db_wire::{DatabaseDriver, DbDriverKind};
    use std::sync::Arc;

    #[test]
    fn completed_search_caps_results_and_rejects_changed_connection() {
        let driver: Arc<dyn DatabaseDriver> = Arc::new(db_wire::SqliteDriver::new_memory());
        let mut connections =
            connection_state::ConnectionManager::new(vec![], Some("owner".into()));
        connections.session_mut("owner").driver = Some(driver.clone());
        let mut search = DefinitionSearch::new("owner".into(), driver.clone(), 0);
        let hit = DefinitionHit {
            kind: DefinitionKind::Routine,
            object: ObjectSummary {
                id: 42,
                schema: "a.b".into(),
                name: "report".into(),
                qualifier: "integer".into(),
            },
            source: Some("SELECT 42".into()),
        };
        assert_eq!(
            hit_title(&hit, DbDriverKind::Postgres),
            r#"Routine  "a.b"."report"(integer)"#
        );

        let mut procedure = hit.clone();
        procedure.object.qualifier = "PROCEDURE".into();
        assert_eq!(
            hit_title(&procedure, DbDriverKind::MySql),
            "Procedure  `a.b`.`report`"
        );

        search.cancel = Some(flume::bounded(1).0);
        search.complete(
            &connections,
            Ok(Some(vec![hit.clone(); DEFINITION_SEARCH_LIMIT + 1])),
        );
        assert_eq!(search.hits.len(), DEFINITION_SEARCH_LIMIT);
        assert!(search.message.contains("Refine"));

        search.hits.clear();
        search.complete(&connections, Ok(Some(vec![hit.clone()])));
        assert!(search.hits.is_empty());
        assert!(search.message.contains("cancelled"));

        search.cancel = Some(flume::bounded(1).0);
        connections.session_mut("owner").metadata_generation += 1;
        search.complete(&connections, Ok(Some(vec![hit])));
        assert!(search.hits.is_empty());
        assert!(search.message.contains("catalog changed"));

        connections.session_mut("owner").metadata_generation = 0;
        assert!(search.is_current(&connections));
        connections.session_mut("owner").driver =
            Some(Arc::new(db_wire::SqliteDriver::new_memory()));
        assert!(!search.is_current(&connections));
    }
}
