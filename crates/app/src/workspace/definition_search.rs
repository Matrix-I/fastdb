mod data;

use super::script_tools::tool;
use super::*;
use db_wire::objects::DEFINITION_INPUT_LIMIT;
use gpui::prelude::FluentBuilder;

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
    schema: String,
    needle: Input,
    message: String,
    data: data::TableSearch,
    source: Option<(SharedString, SharedString)>,
    cancel: Option<flume::Sender<()>>,
    task: Option<Task<()>>,
}

impl DefinitionSearch {
    fn is_current(&self, connections: &connection_state::ConnectionManager) -> bool {
        let session = connections.active();
        connections.active_connection_id.as_ref() == Some(&self.owner)
            && session.metadata_generation == self.generation
            && session
                .driver
                .as_ref()
                .is_some_and(|driver| Arc::ptr_eq(driver, &self.driver))
    }
}

impl Workspace {
    pub(super) fn definition_search_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        let Some(search) = &mut self.definition_search else {
            return;
        };
        match key.key.as_str() {
            "escape" => self.definition_search = None,
            "w" if key.modifiers.platform || key.modifiers.control => self.definition_search = None,
            "enter" | "return" => self.run_table_data_search(cx),
            "tab" => {}
            _ if search.task.is_none() => {
                let input = &mut search.needle;
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
                    search.data.hits.clear();
                    search.data.errors.clear();
                    search.source = None;
                    search.message = "Press Enter to search.".into();
                }
            }
            _ => {}
        }
        cx.notify();
    }

    fn render_definition_field(&self, search: &DefinitionSearch, cx: &Context<Self>) -> Div {
        let input = &search.needle;
        let weak = cx.entity().downgrade();
        let pick: CaretPick = std::rc::Rc::new(move |index, kind, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(search) = &mut this.definition_search
                    && search.task.is_none()
                {
                    let input = &mut search.needle;
                    apply_pick(&mut input.caret, &mut input.anchor, index, kind, true);
                    cx.notify();
                }
            });
        });
        let focused = search.task.is_none();
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
                "Cell contains…",
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
        } else {
            search
                .data
                .hits
                .iter()
                .map(|row| format!("{} · {} · {}", row.table, row.hit.column, row.hit.preview))
                .collect()
        };
        let source = search.source.clone().filter(|_| current);
        let failures = (current && !search.data.errors.is_empty()).then_some(&search.data);
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
                    this.open_data_search_match(index, cx);
                }))
        });
        let action = if search.task.is_none() {
            tool("Search").on_click(cx.listener(|this, _, _, cx| this.run_table_data_search(cx)))
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
                    .child(format!(
                        "Search table data · {connection_name} · {}.{}",
                        search.schema, search.data.table
                    ))
                    .child(self.render_definition_field(search, cx))
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
    use super::{DefinitionSearch, Input, connection_state, data::TableSearch};
    use db_wire::{DatabaseDriver, SqliteDriver};
    use std::sync::Arc;

    #[test]
    fn table_search_rejects_changed_metadata_or_driver() {
        let driver: Arc<dyn DatabaseDriver> = Arc::new(SqliteDriver::new_memory());
        let mut connections =
            connection_state::ConnectionManager::new(vec![], Some("owner".into()));
        connections.session_mut("owner").driver = Some(driver.clone());
        let search = DefinitionSearch {
            owner: "owner".into(),
            driver,
            generation: 0,
            schema: "main".into(),
            needle: Input::new("example"),
            message: String::new(),
            data: TableSearch {
                table: "records".into(),
                hits: Vec::new(),
                errors: Vec::new(),
            },
            source: None,
            cancel: None,
            task: None,
        };

        assert!(search.is_current(&connections));
        connections.session_mut("owner").metadata_generation += 1;
        assert!(!search.is_current(&connections));

        connections.session_mut("owner").metadata_generation = 0;
        assert!(search.is_current(&connections));
        connections.session_mut("owner").driver = Some(Arc::new(SqliteDriver::new_memory()));
        assert!(!search.is_current(&connections));
    }
}
