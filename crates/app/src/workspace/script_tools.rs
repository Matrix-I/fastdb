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

impl ScriptAction {
    pub(super) fn cancel_selection(&mut self) -> bool {
        !self.delete
            && ui_editor::line_input::clear_selection(
                &mut self.caret,
                &mut self.anchor,
                self.name.chars().count(),
            )
    }
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

struct ScriptToolTip(String);

impl Render for ScriptToolTip {
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

fn rail_symbol(label: &str) -> &'static str {
    match label {
        "Execute" => "▶",
        "Cancel execution" => "■",
        "Run script" => "⇉",
        "Execution log" => "☷",
        label if label.starts_with("Timeout:") => "◷",
        "Begin" => "▷",
        "Commit" => "✓",
        "Rollback" => "↶",
        "Isolation…" => "◇",
        "Transaction: Auto" | "Transaction" => "Ⓐ",
        "Transaction: Manual" => "Ⓜ",
        label if label.starts_with("Autocommit") => "Ⓐ",
        "Database…" => "▤",
        "Schema…" => "▦",
        "Format" => "≡",
        "Snippets…" => "{}",
        "Import SQL copy…" => "↥",
        "Save SQL copy…" => "↧",
        "Find / Replace" => "⌕",
        "Comment / Uncomment" => "--",
        "Indent" => "⇥",
        "Outdent" => "⇤",
        "History" => "↺",
        "Explain" => "⑂",
        "Analyze (executes SQL)" => "⚡",
        "Rename" => "✎",
        "Delete" => "×",
        _ => "?",
    }
}

fn rail_explanation(label: &str) -> String {
    if label.starts_with("Timeout:") {
        return format!("{label} — click to cycle off, 5s, 30s, 60s and 300s for the next run");
    }
    if label.starts_with("Autocommit") {
        return format!("{label} — choose the transaction mode for this SQL tab");
    }
    match label {
        "Execute" => "Execute selected SQL or the statement at the caret (Cmd/Ctrl+Enter)",
        "Cancel execution" => "Request cancellation of the running operation",
        "Run script" => "Run all statements in order (Cmd/Ctrl+Shift+Enter)",
        "Execution log" => "Show or hide recent statement outcomes",
        "Begin" => "Begin a transaction on this SQL tab's session",
        "Commit" => "Commit the transaction on this SQL tab's session",
        "Rollback" => "Roll back the transaction on this SQL tab's session",
        "Isolation…" => "Choose the session transaction isolation level",
        "Transaction: Auto" | "Transaction" => {
            "Transaction mode: Auto-commit — click to choose Auto-Commit, Manual Commit or isolation level"
        }
        "Transaction: Manual" => {
            "Transaction mode: Manual Commit — click to choose Auto-Commit, Manual Commit or isolation level"
        }
        "Database…" => "Choose a database for a new SQL tab",
        "Schema…" => "Choose the current schema on this SQL tab's session",
        "Format" => "Format selected SQL or the whole script (Cmd/Ctrl+Shift+F)",
        "Snippets…" => "Browse SQL templates and insert one at the caret",
        "Import SQL copy…" => "Import a SQL file into a new autosaved script",
        "Save SQL copy…" => "Save a copy of the current SQL text to a file",
        "Find / Replace" => "Find and replace literal text in this script",
        "Comment / Uncomment" => "Toggle line comments on the current or selected lines",
        "Indent" => "Indent the current or selected lines by four spaces",
        "Outdent" => "Remove one level of indentation from the current or selected lines",
        "History" => "Show recent queries for this connection and insert a previous query",
        "Explain" => "Show the PostgreSQL query plan without executing SQL",
        "Analyze (executes SQL)" => {
            "Analyze: executes SQL, including writes, and shows actual timings"
        }
        "Rename" => "Rename the current script file",
        "Delete" => "Delete script: confirm moving the file to .trash",
        _ => label,
    }
    .to_owned()
}

pub(super) fn rail_tool(label: &'static str) -> Stateful<Div> {
    let mut btn = div()
        .id(label)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(28.0))
        .h(px(28.0))
        .rounded_xs()
        .cursor_pointer();

    match label {
        "Execute" => {
            btn = btn
                .bg(rgb(0x238636))
                .hover(|s| s.bg(rgb(0x2ea043)))
                .text_size(px(12.0))
                .text_color(rgb(0xffffff));
        }
        "Cancel execution" => {
            btn = btn
                .bg(rgb(0xda3633))
                .hover(|s| s.bg(rgb(0xf85149)))
                .text_size(px(12.0))
                .text_color(rgb(0xffffff));
        }
        _ => {
            btn = btn
                .text_size(px(13.5))
                .text_color(rgb(0xc9d1d9))
                .hover(|s| s.bg(rgb(0x30363d)).text_color(rgb(0xffffff)));
        }
    }

    btn.tooltip(move |_, cx| cx.new(|_| ScriptToolTip(rail_explanation(label))).into())
        .child(rail_symbol(label))
}

impl Workspace {
    #[allow(dead_code)]
    pub(super) fn edit_script_lines(
        &mut self,
        command: super::script_lines::LineCommand,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .tabs
            .get_mut(self.active_tab)
            .filter(|t| t.is_script() && !t.file_busy)
        else {
            return;
        };
        let before = tab.editor.buffer.revision();
        super::script_lines::apply(&mut tab.editor.buffer, command);
        tab.editor.hide_completions();
        tab.editor.caret_moved = true;
        tab.editor_focused = true;
        if before != tab.editor.buffer.revision() {
            let (id, revision) = (tab.id, tab.editor.buffer.revision());
            self.schedule_autosave(id, revision, cx);
        }
        cx.notify();
    }

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

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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
                ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
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

#[cfg(test)]
mod rail_tests {
    use super::{rail_explanation, rail_symbol};

    #[test]
    fn hover_explanations_include_state_and_execution_warning() {
        let timeout = rail_explanation("Timeout: 30s");
        assert!(timeout.contains("Timeout: 30s"));
        assert!(timeout.contains("click to cycle"));
        assert!(rail_explanation("Autocommit: off…").contains("off"));
        assert!(rail_explanation("Execute").contains("statement at the caret"));
        assert!(rail_explanation("Analyze (executes SQL)").contains("including writes"));
        assert!(rail_explanation("Delete").contains("confirm"));
    }

    #[test]
    fn dynamic_controls_keep_their_icons() {
        for label in [
            "Timeout: off",
            "Timeout: 5s",
            "Timeout: 30s",
            "Timeout: 60s",
            "Timeout: 300s",
        ] {
            assert_eq!(rail_symbol(label), "◷");
        }
        for label in ["Autocommit…", "Autocommit: on…", "Autocommit: off…"] {
            assert_eq!(rail_symbol(label), "Ⓐ");
        }
        assert_ne!(rail_symbol("Execute"), rail_symbol("Cancel execution"));
        assert_ne!(
            rail_symbol("Explain"),
            rail_symbol("Analyze (executes SQL)")
        );
    }

    #[test]
    fn every_rail_symbol_has_a_descriptive_explanation() {
        let labels = [
            "Execute",
            "Cancel execution",
            "Run script",
            "Execution log",
            "Timeout: off",
            "Begin",
            "Commit",
            "Rollback",
            "Isolation…",
            "Autocommit…",
            "Transaction: Auto",
            "Transaction: Manual",
            "Database…",
            "Schema…",
            "Format",
            "Snippets…",
            "Import SQL copy…",
            "Save SQL copy…",
            "Find / Replace",
            "Comment / Uncomment",
            "Indent",
            "Outdent",
            "History",
            "Explain",
            "Analyze (executes SQL)",
            "Rename",
            "Delete",
        ];
        for label in labels {
            let symbol = rail_symbol(label);
            assert_ne!(symbol, "?", "Label {label} has no rail symbol");
            let explanation = rail_explanation(label);
            assert!(!explanation.is_empty(), "Explanation for {label} is empty");
            if !label.starts_with("Timeout:") && !label.starts_with("Autocommit") {
                assert_ne!(explanation, label, "Label {label} fell back to raw label");
            }
        }
    }
}
