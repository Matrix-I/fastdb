//! "Choose SQL script" dialog, shown when the latest script of a connection is already
//! open: search, a New script button, scripts of all connections, and the list by time.

use gpui::prelude::FluentBuilder;

use super::*;

/// Rows of the script list shown at once
const VISIBLE_ROWS: usize = 14;

/// State of the open script chooser
pub struct ScriptChooser {
    /// Connection the dialog was opened for
    pub connection_id: String,
    pub query: String,
    pub caret: usize,
    pub anchor: Option<usize>,
    pub history: LineHistory,
    pub all_connections: bool,
    /// Position of the selection within `visible()`
    pub selected: usize,
    /// Scripts listed for the current checkbox state, newest first
    pub scripts: Vec<ScriptFile>,
}

impl ScriptChooser {
    pub fn new(connection_id: String, scripts: Vec<ScriptFile>) -> Self {
        Self {
            connection_id,
            query: String::new(),
            caret: usize::MAX,
            anchor: None,
            history: LineHistory::new(),
            all_connections: false,
            selected: 0,
            scripts,
        }
    }

    /// Indices into `scripts` matching the search text
    pub fn visible(&self) -> Vec<usize> {
        let matching = filter_scripts(&self.scripts, &self.query);
        self.scripts
            .iter()
            .enumerate()
            .filter(|(_, s)| matching.iter().any(|m| std::ptr::eq(*m, *s)))
            .map(|(i, _)| i)
            .collect()
    }

    /// After the search text changed, the selection restarts at the first match
    pub fn query_edited(&mut self) {
        self.selected = 0;
    }

    pub fn move_selection(&mut self, down: bool) {
        let len = self.visible().len();
        self.selected = if down { (self.selected + 1).min(len.saturating_sub(1)) } else { self.selected.saturating_sub(1) };
    }

    /// Script under the selection, if any
    pub fn selected_script(&self) -> Option<&ScriptFile> {
        self.visible().get(self.selected).map(|&i| &self.scripts[i])
    }
}

impl Workspace {
    pub(super) fn open_script_chooser(&mut self, conn_id: &str) {
        let scripts = self.script_store.list(Some(conn_id));
        self.script_chooser = Some(ScriptChooser::new(conn_id.to_string(), scripts));
        self.cursor_visible = true;
    }

    /// Opens a script picked in the chooser and closes it
    fn choose_script(&mut self, script: ScriptFile, cx: &mut Context<Self>) {
        self.script_chooser = None;
        self.open_script_file(&script, cx);
    }

    fn choose_new_script(&mut self, cx: &mut Context<Self>) {
        let Some(chooser) = self.script_chooser.take() else { return };
        match self.script_store.create(&chooser.connection_id) {
            Ok(script) => self.open_script_file(&script, cx),
            Err(e) => tracing::warn!("{e:#}"),
        }
    }

    fn toggle_all_connections(&mut self) {
        let Some(chooser) = self.script_chooser.as_mut() else { return };
        chooser.all_connections = !chooser.all_connections;
        let filter = (!chooser.all_connections).then_some(chooser.connection_id.as_str());
        chooser.scripts = self.script_store.list(filter);
        chooser.selected = 0;
    }

    /// Keys while the chooser is open: arrows move, Enter opens, Escape closes, the rest search
    pub(super) fn handle_script_chooser_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        let Some(chooser) = self.script_chooser.as_mut() else { return };
        match keystroke.key.as_str() {
            "escape" => self.script_chooser = None,
            "up" => chooser.move_selection(false),
            "down" => chooser.move_selection(true),
            "enter" | "return" => {
                if let Some(script) = chooser.selected_script().cloned() {
                    self.choose_script(script, cx);
                }
            }
            _ => {
                let before = chooser.query.clone();
                let input = LineInput {
                    text: &mut chooser.query,
                    caret: &mut chooser.caret,
                    anchor: &mut chooser.anchor,
                    digits_only: false,
                    history: &mut chooser.history,
                };
                if !handle_line_key(input, keystroke, cx) {
                    return;
                }
                if chooser.query != before {
                    chooser.query_edited();
                }
                self.cursor_visible = true;
            }
        }
        cx.notify();
    }

    /// Click handler for the characters of the chooser's search box
    fn chooser_caret_pick(&self, cx: &mut Context<Self>) -> CaretPick {
        let workspace = cx.entity().downgrade();
        std::rc::Rc::new(move |index, kind, _, cx| {
            let _ = workspace.update(cx, |this, cx| {
                if let Some(chooser) = this.script_chooser.as_mut() {
                    apply_pick(&mut chooser.caret, &mut chooser.anchor, index, kind, true);
                    this.cursor_visible = true;
                    cx.notify();
                }
            });
        })
    }

    pub(super) fn render_script_chooser(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let theme = &theme;
        let pick = self.chooser_caret_pick(cx);
        let connection_name = |id: &str| {
            self.saved_connections.iter().find(|c| c.id == id).map(|c| c.name.clone()).unwrap_or_else(|| id.to_string())
        };
        let Some(chooser) = self.script_chooser.as_ref() else { return div().into_any_element() };
        let title = format!("Choose SQL script for '{}'", connection_name(&chooser.connection_id));
        let all = chooser.all_connections;
        let visible = chooser.visible();
        let first = chooser.selected.saturating_sub(VISIBLE_ROWS - 1);

        let cell = |w: f32| div().flex_none().w(px(w)).overflow_hidden().whitespace_nowrap().text_ellipsis();
        let header = div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(24.0))
            .px_2()
            .bg(theme.bg_surface_hover)
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.text_secondary)
            .child(cell(170.0).child("Script"))
            .child(cell(150.0).child("Time"))
            .child(div().flex_1().child("Info"))
            .when(all, |d| d.child(cell(110.0).child("Connection")));

        let mut rows = div().flex().flex_col().h(px(VISIBLE_ROWS as f32 * 24.0)).overflow_hidden();
        for (pos, &i) in visible.iter().enumerate().skip(first).take(VISIBLE_ROWS) {
            let script = &chooser.scripts[i];
            let is_sel = pos == chooser.selected;
            let time = chrono::DateTime::<chrono::Local>::from(script.modified).format("%Y-%m-%d %H:%M:%S").to_string();
            let clicked = script.clone();
            rows = rows.child(
                div()
                    .id(("script-chooser-row", pos))
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_2()
                    .h(px(24.0))
                    .px_2()
                    .rounded_sm()
                    .text_size(px(12.0))
                    .text_color(theme.text_primary)
                    .cursor_pointer()
                    .when(is_sel, |d| d.bg(theme.bg_surface_selected))
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        if event.click_count() >= 2 {
                            this.choose_script(clicked.clone(), cx);
                        } else if let Some(chooser) = this.script_chooser.as_mut() {
                            chooser.selected = pos;
                        }
                        cx.notify();
                    }))
                    .child(cell(170.0).flex().items_center().gap_1p5().child(img("icons/sql_script.svg").w(px(13.0)).h(px(13.0))).child(script.name.clone()))
                    .child(cell(150.0).text_color(theme.text_secondary).child(time))
                    .child(div().flex_1().overflow_hidden().whitespace_nowrap().text_ellipsis().text_color(theme.text_muted).child(script.preview.clone()))
                    .when(all, |d| d.child(cell(110.0).text_color(theme.text_secondary).child(connection_name(&script.connection_id)))),
            );
        }
        if visible.is_empty() {
            rows = rows.child(div().p_3().text_size(px(12.0)).text_color(theme.text_muted).child("No scripts match."));
        }

        let search = div()
            .flex()
            .items_center()
            .h(px(28.0))
            .px_2()
            .bg(theme.bg_editor)
            .border_1()
            .border_color(rgb(0x388bfd))
            .rounded_xs()
            .cursor_text()
            .text_size(px(12.0))
            .child(render_field_content(
                &chooser.query,
                "Enter a part of script name here",
                true,
                chooser.anchor,
                chooser.caret,
                self.cursor_visible,
                theme,
                &pick,
            ));

        let actions = div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .id("btn-script-chooser-new")
                    .px_3()
                    .py_1()
                    .rounded_sm()
                    .bg(theme.bg_surface_hover)
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_selected))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.choose_new_script(cx);
                        cx.notify();
                    }))
                    .child("New script"),
            )
            .child(
                div()
                    .id("chk-script-chooser-all")
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.toggle_all_connections();
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(14.0))
                            .h(px(14.0))
                            .rounded_xs()
                            .border_1()
                            .border_color(theme.border_focused)
                            .text_size(px(10.0))
                            .when(all, |d| d.bg(rgb(0x388bfd)).text_color(rgb(0xffffff)).child("✓")),
                    )
                    .child("Show scripts for all connections"),
            );

        div()
            .absolute()
            .inset_0()
            .bg(rgba(0x00000088))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("script-chooser")
                    .occlude()
                    .w(px(if all { 760.0 } else { 640.0 }))
                    .bg(theme.bg_surface)
                    .border_1()
                    .border_color(theme.border_focused)
                    .rounded_lg()
                    .shadow_lg()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.script_chooser = None;
                        cx.notify();
                    }))
                    .child(div().text_size(px(13.0)).font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(search)
                    .child(actions)
                    .child(div().flex().flex_col().border_1().border_color(theme.border).rounded_sm().child(header).child(rows)),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::ScriptChooser;
    use crate::workspace::ScriptFile;
    use std::time::SystemTime;

    fn script(name: &str, preview: &str) -> ScriptFile {
        ScriptFile {
            path: format!("/tmp/{name}.sql").into(),
            connection_id: "pg".into(),
            name: name.into(),
            modified: SystemTime::now(),
            preview: preview.into(),
        }
    }

    fn chooser() -> ScriptChooser {
        ScriptChooser::new("pg".into(), vec![script("Script-3", "explain x"), script("Script-2", "select 1"), script("Script-1", "")])
    }

    #[test]
    fn selection_moves_within_the_filtered_list() {
        let mut c = chooser();
        assert_eq!(c.selected_script().unwrap().name, "Script-3");
        c.move_selection(true);
        c.move_selection(true);
        c.move_selection(true);
        assert_eq!(c.selected_script().unwrap().name, "Script-1");
        c.move_selection(false);
        assert_eq!(c.selected_script().unwrap().name, "Script-2");
    }

    #[test]
    fn typing_a_query_filters_and_resets_the_selection() {
        let mut c = chooser();
        c.move_selection(true);
        c.query = "SELECT".into();
        c.query_edited();
        assert_eq!(c.visible(), vec![1]);
        assert_eq!(c.selected_script().unwrap().name, "Script-2");
        c.query = "zzz".into();
        c.query_edited();
        assert!(c.selected_script().is_none());
    }
}
