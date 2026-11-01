//! Editor toolbar commands and script file operations, outside the render path.
use super::*;
use std::rc::Rc;

pub(super) struct ScriptAction {
    tab_id: u64,
    delete: bool,
    name: String,
    caret: usize,
    anchor: Option<usize>,
    history: LineHistory,
    error: Option<String>,
}

pub(super) fn tool(label: &'static str) -> Stateful<Div> {
    div()
        .flex_none()
        .id(label)
        .px_2()
        .py_1()
        .rounded_xs()
        .text_size(px(11.0))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(0x30363d)))
        .child(label)
}

impl Workspace {
    pub(super) fn format_script(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self
            .tabs
            .get_mut(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy)
        else {
            return;
        };
        let selection = tab
            .editor
            .buffer
            .selection()
            .unwrap_or(0..tab.editor.buffer.len());
        let source = tab.editor.buffer.text();
        match sql_intel::format_sql(&source[selection.clone()]) {
            Ok(formatted) if formatted != source[selection.clone()] => {
                tab.editor
                    .buffer
                    .replace_range(selection.start, selection.end, &formatted);
                tab.editor.hide_completions();
                tab.editor.caret_moved = true;
                let (id, revision) = (tab.id, tab.editor.buffer.revision());
                self.schedule_autosave(id, revision, cx);
            }
            Err(error) => tab.last_error = Some(error),
            _ => {}
        }
        cx.notify();
    }

    pub(super) fn toggle_history(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active_tab).filter(|t| t.is_script()) else {
            return;
        };
        tab.show_history = !tab.show_history;
        if !tab.show_history {
            cx.notify();
            return;
        }
        let id = tab.id;
        let receiver = self.query_history.list(tab.connection_id.clone().unwrap());
        cx.spawn(async move |this, cx| {
            let result = receiver
                .recv_async()
                .await
                .unwrap_or_else(|_| Err("History worker stopped".into()));
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                    match result {
                        Ok(entries) => tab.query_history = Arc::new(entries),
                        Err(error) => tab.last_error = Some(error),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn recall_history(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active_tab).filter(|t| !t.file_busy) else {
            return;
        };
        if let Some(sql) = tab.query_history.get(index).cloned() {
            tab.editor.insert_text(&sql);
            tab.editor.hide_completions();
            tab.editor_focused = true;
            tab.show_history = false;
            let (id, revision) = (tab.id, tab.editor.buffer.revision());
            self.schedule_autosave(id, revision, cx);
            cx.notify();
        }
    }

    pub(super) fn open_script_action(&mut self, delete: bool, cx: &mut Context<Self>) {
        if self.operations.is_running() {
            return;
        }
        if let Some(tab) = self
            .tabs
            .get(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy)
        {
            self.script_action = Some(ScriptAction {
                tab_id: tab.id,
                delete,
                name: tab.title.clone(),
                caret: usize::MAX,
                anchor: Some(0),
                history: LineHistory::new(),
                error: None,
            });
            cx.notify();
        }
    }

    pub(super) fn handle_script_action_key(&mut self, key: &Keystroke, cx: &mut Context<Self>) {
        if key.key == "escape" {
            self.script_action = None;
        } else if matches!(key.key.as_str(), "enter" | "return") {
            self.apply_script_action(cx);
        } else if let Some(action) = self.script_action.as_mut().filter(|a| !a.delete) {
            handle_line_key(
                LineInput {
                    text: &mut action.name,
                    caret: &mut action.caret,
                    anchor: &mut action.anchor,
                    digits_only: false,
                    history: &mut action.history,
                },
                key,
                cx,
            );
        }
        cx.notify();
    }

    fn apply_script_action(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self.script_action.as_mut() else {
            return;
        };
        let Some(tab) = self.tabs.iter_mut().find(|t| t.id == action.tab_id) else {
            self.script_action = None;
            return;
        };
        let Some(path) = tab.script_path.clone() else {
            return;
        };
        let target = if action.delete {
            ScriptStore::archive_target(&path)
        } else {
            match ScriptStore::rename_target(&path, &action.name) {
                Ok(target) => target,
                Err(error) => {
                    action.error = Some(error.to_string());
                    cx.notify();
                    return;
                }
            }
        };
        if target == path {
            self.script_action = None;
            cx.notify();
            return;
        }
        let id = tab.id;
        let delete = action.delete;
        let text = tab.editor.buffer.text();
        let revision = tab.editor.buffer.revision();
        tab.file_busy = true;
        // Queue the final snapshot after older writes; mutation follows it on the same
        // writer. Editor input/autosave are blocked until the new path is installed.
        let moved = self
            .save_queue
            .move_script(path, target.clone(), text.clone());
        self.script_action = None;
        cx.spawn(async move |this, cx| {
            let result = moved
                .recv_async()
                .await
                .unwrap_or_else(|_| Err("File writer stopped".into()));
            let _ = this.update(cx, |this, cx| {
                if let Some(index) = this.tabs.iter().position(|t| t.id == id) {
                    let tab = &mut this.tabs[index];
                    tab.file_busy = false;
                    match result {
                        Ok(()) => {
                            tab.mark_saved(revision, text);
                            if delete {
                                this.close_tab(index, cx);
                            } else {
                                tab.title = target.file_stem().unwrap().to_string_lossy().into();
                                tab.script_path = Some(target);
                            }
                        }
                        Err(error) => tab.last_error = Some(error),
                    }
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_script_action(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let action = self.script_action.as_ref().unwrap();
        let theme = self.theme.clone();
        let weak = cx.entity().downgrade();
        let pick: CaretPick = Rc::new(move |index, kind, _, cx| {
            let _ = weak.update(cx, |this, cx| {
                if let Some(action) = this.script_action.as_mut() {
                    apply_pick(&mut action.caret, &mut action.anchor, index, kind, true);
                    cx.notify();
                }
            });
        });
        let mut panel = div()
            .w(px(440.0))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border)
            .text_color(theme.text_primary)
            .child(if action.delete {
                "Delete script?"
            } else {
                "Rename script"
            });
        if action.delete {
            panel = panel.child(div().text_size(px(12.0)).child(format!("Move {} to the connection's scripts/.trash folder? You can restore its .sql file manually.", action.name)));
        } else {
            panel = panel.child(
                div()
                    .px_2()
                    .py_1()
                    .border_1()
                    .border_color(theme.border_focused)
                    .child(render_field_content(
                        &action.name,
                        "Script name",
                        true,
                        action.anchor,
                        action.caret,
                        true,
                        &theme,
                        &pick,
                    )),
            );
        }
        if let Some(error) = &action.error {
            panel = panel.child(
                div()
                    .text_color(rgb(0xf85149))
                    .text_size(px(12.0))
                    .child(error.clone()),
            );
        }
        panel = panel.child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(tool("Cancel").on_click(cx.listener(|this, _, _, cx| {
                    this.script_action = None;
                    cx.notify();
                })))
                .child(
                    tool(if action.delete { "Delete" } else { "Rename" })
                        .on_click(cx.listener(|this, _, _, cx| this.apply_script_action(cx))),
                ),
        );
        div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000088))
            .occlude()
            .child(panel)
    }
}
