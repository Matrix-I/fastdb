use super::script_tools::tool;
use super::*;
use db_wire::filter::{Criterion, FilterOp, FilterOrder, FilterSpec};
use gpui::prelude::FluentBuilder;
use std::rc::Rc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Search,
    Value(usize),
}
pub(super) struct CriteriaEditor {
    tab: u64,
    owner: String,
    kind: DbDriverKind,
    generation: u64,
    columns: Vec<String>,
    spec: FilterSpec,
    search: String,
    active: Field,
    caret: usize,
    anchor: Option<usize>,
    history: LineHistory,
    operator: Option<usize>,
    order_picker: bool,
    error: Option<String>,
    scroll: ScrollHandle,
    preview: Result<Arc<String>, String>,
    preview_display: String,
    value_scroll: ScrollHandle,
    reveal_value: std::cell::Cell<bool>,
}
impl CriteriaEditor {
    fn select_operator(&mut self, index: usize, op: FilterOp) {
        if let Some(criterion) = self.spec.criteria.get_mut(index) {
            criterion.op = op;
        }
        self.operator = None;
        if !op.needs_value() {
            self.active = Field::Search;
            self.caret = self.search.chars().count();
            self.anchor = None;
            self.history.clear();
        }
    }
    fn update_preview(&mut self) {
        self.preview = match self.spec.sql(self.kind, &self.columns) {
            Ok(sql) => {
                self.preview_display = sql.chars().take(4096).collect();
                if sql.chars().count() > 4096 {
                    self.preview_display
                        .push_str("\n… Preview truncated; Copy full SQL to inspect all text.");
                }
                Ok(Arc::new(sql))
            }
            Err(error) => {
                self.preview_display = error.to_string();
                Err(error.to_string())
            }
        };
    }
}
fn value_window(
    value: &str,
    caret: usize,
    anchor: Option<usize>,
) -> (String, usize, Option<usize>, usize, usize) {
    let total = value.chars().count();
    let caret = caret.min(total);
    let start = caret.saturating_sub(80).min(total.saturating_sub(160));
    let text = value.chars().skip(start).take(160).collect::<String>();
    let len = text.chars().count();
    (
        text,
        caret - start,
        anchor.map(|a| a.min(total).saturating_sub(start).min(len)),
        start,
        len,
    )
}
impl Workspace {
    fn criteria_changed(&mut self, cx: &mut Context<Self>) {
        if let Some(b) = &mut self.filter_builder {
            b.update_preview();
        }
        cx.notify();
    }
    pub(super) fn open_filter_builder(&mut self, cx: &mut Context<Self>) {
        let index = self.active_tab;
        let Some(tab) = self
            .tabs
            .get(index)
            .filter(|t| !t.is_script() && t.column_editor.is_none())
        else {
            return;
        };
        if self.tab_session(index).driver.is_none() {
            self.tabs[index].last_error = Some("Connect before opening filter criteria".into());
            cx.notify();
            return;
        }
        if tab.grid.changeset.is_dirty() || tab.grid.editing_cell.is_some() {
            self.tabs[index].last_error =
                Some("Save or Cancel row edits before building criteria".into());
            cx.notify();
            return;
        }
        let owner = tab.connection_id.clone().unwrap_or_default();
        let session = self.connections.session(&owner);
        let columns = session
            .table_columns
            .get(&(
                tab.schema_name.clone().unwrap_or_default(),
                tab.table_name.clone().unwrap_or_default(),
            ))
            .map(|cols| cols.iter().map(|c| c.name.clone()).collect::<Vec<_>>())
            .unwrap_or_default();
        let spec = tab
            .filter_spec
            .clone()
            .filter(|s| {
                s.sql(session.kind, &columns).ok().as_deref() == Some(tab.filter_sql.as_str())
            })
            .unwrap_or_default();
        let mut editor = CriteriaEditor {
            tab: tab.id,
            owner,
            kind: session.kind,
            generation: session.metadata_generation,
            columns,
            spec,
            search: String::new(),
            active: Field::Search,
            caret: 0,
            anchor: None,
            history: Default::default(),
            operator: None,
            order_picker: false,
            error: None,
            scroll: ScrollHandle::new(),
            preview: Err(String::new()),
            preview_display: String::new(),
            value_scroll: ScrollHandle::new(),
            reveal_value: std::cell::Cell::new(true),
        };
        editor.update_preview();
        self.filter_builder = Some(editor);
        self.column_pick = None;
        cx.notify();
    }
    fn criteria_pick(&self, field: Field, cx: &mut Context<Self>) -> CaretPick {
        let workspace = cx.entity().downgrade();
        Rc::new(move |index, kind, window, cx| {
            let _ = workspace.update(cx, |this, cx| {
                if let Some(b) = &mut this.filter_builder {
                    if matches!(field,Field::Value(i) if i>=b.spec.criteria.len()) {
                        return;
                    }
                    let active = b.active == field;
                    if !active && kind == ui_editor::line_input::PickKind::Drag {
                        return;
                    }
                    if !active {
                        b.history = Default::default();
                    }
                    b.active = field;
                    if matches!(field, Field::Value(_)) {
                        b.reveal_value.set(true);
                    }
                    apply_pick(&mut b.caret, &mut b.anchor, index, kind, active);
                    window.focus(&this.focus_handle);
                    cx.notify();
                }
            });
        })
    }
    pub(super) fn handle_filter_builder_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(b) = &mut self.filter_builder else {
            return false;
        };
        if event.keystroke.key == "escape" {
            if ui_editor::line_input::clear_selection(
                &mut b.caret,
                &mut b.anchor,
                match b.active {
                    Field::Search => b.search.chars().count(),
                    Field::Value(i) => b
                        .spec
                        .criteria
                        .get(i)
                        .map(|c| c.value.chars().count())
                        .unwrap_or(0),
                },
            ) {
                cx.notify();
                return true;
            }
            self.filter_builder = None;
            cx.notify();
            return true;
        }
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && matches!(event.keystroke.key.as_str(), "enter" | "return")
        {
            self.apply_filter_builder(cx);
            return true;
        }
        if event.keystroke.key == "tab" {
            let mut fields = vec![Field::Search];
            fields.extend(
                b.spec
                    .criteria
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.op.needs_value())
                    .map(|(i, _)| Field::Value(i)),
            );
            let current = fields.iter().position(|f| *f == b.active).unwrap_or(0);
            let next = if event.keystroke.modifiers.shift {
                (current + fields.len() - 1) % fields.len()
            } else {
                (current + 1) % fields.len()
            };
            b.active = fields[next];
            b.reveal_value.set(true);
            b.anchor = None;
            b.history = Default::default();
            b.caret = match b.active {
                Field::Search => b.search.chars().count(),
                Field::Value(i) => b.spec.criteria[i].value.chars().count(),
            };
            cx.notify();
            return true;
        }
        let text = match b.active {
            Field::Search => &mut b.search,
            Field::Value(i) => {
                let Some(c) = b.spec.criteria.get_mut(i) else {
                    return true;
                };
                &mut c.value
            }
        };
        let before = text.clone();
        let (caret, anchor) = (b.caret, b.anchor);
        if handle_line_key(
            LineInput {
                text: &mut *text,
                caret: &mut b.caret,
                anchor: &mut b.anchor,
                digits_only: false,
                history: &mut b.history,
            },
            &event.keystroke,
            cx,
        ) {
            if text.len() > if b.active == Field::Search { 256 } else { 8192 } {
                *text = before;
                b.caret = caret;
                b.anchor = anchor;
                b.history = Default::default();
                b.error = Some("Search is limited to 256 bytes and filter values to 8 KiB".into());
            } else {
                b.error = None;
            }
            b.reveal_value.set(true);
            b.update_preview();
            cx.notify();
        }
        true
    }
    fn apply_filter_builder(&mut self, cx: &mut Context<Self>) {
        let Some(b) = &self.filter_builder else {
            return;
        };
        let Some(index) = self
            .tabs
            .iter()
            .position(|t| t.id == b.tab && t.connection_id.as_deref() == Some(&b.owner))
        else {
            self.filter_builder = None;
            return;
        };
        let session = self.connections.session(&b.owner);
        let error = if self.operations.is_running() {
            Some("Wait for the active database operation".into())
        } else if session.metadata_generation != b.generation
            || session.kind != b.kind
            || session.driver.is_none()
        {
            Some("Connection metadata changed; close and reopen criteria".into())
        } else if self.tabs[index].grid.changeset.is_dirty()
            || self.tabs[index].grid.editing_cell.is_some()
        {
            Some("Save or Cancel row edits before changing filters".into())
        } else {
            None
        };
        if let Some(error) = error {
            self.filter_builder.as_mut().unwrap().error = Some(error);
            cx.notify();
            return;
        }
        let sql = match b.spec.sql(b.kind, &b.columns) {
            Ok(sql) => sql,
            Err(e) => {
                self.filter_builder.as_mut().unwrap().error = Some(e.to_string());
                cx.notify();
                return;
            }
        };
        let b = self.filter_builder.take().unwrap();
        let tab = &mut self.tabs[index];
        tab.filter_sql = sql;
        tab.filter_spec = Some(b.spec);
        tab.grid.ordering = None;
        self.active_tab = index;
        self.input.is_filter_focused = false;
        self.run_active_query(cx);
        cx.notify();
    }
    pub(super) fn navigate_filter_history(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.operations.is_running() {
            return;
        }
        let index = self.active_tab;
        if self.tab_session(index).driver.is_none() {
            return;
        }
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        if tab.grid.changeset.is_dirty() || tab.grid.editing_cell.is_some() {
            tab.last_error =
                Some("Save or Cancel row edits before navigating filter history".into());
            cx.notify();
            return;
        }
        if let Some(snapshot) = tab.filter_timeline.navigate(forward) {
            tab.filter_sql = snapshot.sql;
            tab.filter_spec = snapshot.criteria;
            tab.grid.ordering = snapshot
                .order
                .map(|(column, descending)| ui_grid::ColumnOrder { column, descending });
            tab.fetch.size = snapshot.fetch_size;
            tab.fetch.size_text = snapshot.fetch_size.to_string();
            self.run_active_query(cx);
            cx.notify();
        }
    }
    fn render_criterion_value(
        &self,
        b: &CriteriaEditor,
        i: usize,
        value: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if b.active == Field::Value(i) {
            let (display, caret, anchor, start, len) = value_window(value, b.caret, b.anchor);
            let pick = self.criteria_pick(Field::Value(i), cx);
            let pick: CaretPick = Rc::new(move |index, kind, window, cx| {
                pick(start + index.min(len), kind, window, cx)
            });
            let selection = selection_range(anchor, caret, len);
            let target = if selection.is_some() {
                caret.min(len.saturating_sub(1))
            } else {
                caret
            };
            if b.reveal_value.replace(false) {
                b.value_scroll.scroll_to_item(target);
            }
            div()
                .id(("criteria-value", i))
                .flex_1()
                .min_w(px(160.0))
                .min_h_0()
                .border_1()
                .border_color(self.theme.border_focused)
                .child(
                    ui_editor::line_input::pick_surface(div(), usize::MAX, &pick)
                        .id("criteria-value-scroll")
                        .flex()
                        .items_center()
                        .h(px(20.0))
                        .w_full()
                        .overflow_x_scroll()
                        .track_scroll(&b.value_scroll)
                        .children(render_caret_text(
                            &display,
                            Some(caret),
                            true,
                            selection,
                            &pick,
                        )),
                )
                .child(properties::grid::detail_scrollbar(
                    b.value_scroll.clone(),
                    false,
                    &self.theme,
                ))
                .into_any_element()
        } else {
            let mut summary = value.chars().take(80).collect::<String>();
            if value.chars().nth(80).is_some() {
                summary.push('…');
            }
            div()
                .id(("criteria-value", i))
                .flex_1()
                .min_w(px(160.0))
                .p_1()
                .border_1()
                .border_color(self.theme.border)
                .cursor_text()
                .child(if summary.is_empty() {
                    "Literal value".to_string()
                } else {
                    summary
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        if let Some(b) = &mut this.filter_builder
                            && let Some(c) = b.spec.criteria.get(i)
                        {
                            b.active = Field::Value(i);
                            b.caret = c.value.chars().count();
                            b.anchor = Some(0);
                            b.history = Default::default();
                            window.focus(&this.focus_handle);
                            cx.notify();
                        }
                    }),
                )
                .into_any_element()
        }
    }
    fn render_criteria_columns(&self, b: &CriteriaEditor, cx: &mut Context<Self>) -> Stateful<Div> {
        let theme = &self.theme;
        let query = b.search.to_lowercase();
        let mut candidates = div()
            .id("criteria-columns")
            .flex()
            .flex_col()
            .h(px(110.0))
            .flex_none()
            .overflow_y_scroll();
        let mut candidate_count = 0;
        for column in b
            .columns
            .iter()
            .filter(|c| c.to_lowercase().contains(&query))
            .take(100)
        {
            candidate_count += 1;
            let column = column.clone();
            let label = format!(
                "{} {column}",
                if b.order_picker {
                    "Order by"
                } else {
                    "Add criterion:"
                }
            );
            candidates = candidates.child(
                div()
                    .id(SharedString::from(column.clone()))
                    .cursor_pointer()
                    .p_1()
                    .hover(|s| s.bg(theme.bg_surface_hover))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(b) = &mut this.filter_builder {
                            if b.order_picker {
                                b.spec.order = Some(FilterOrder {
                                    column: column.clone(),
                                    descending: false,
                                });
                                b.order_picker = false;
                            } else if b.spec.criteria.len() < 32 {
                                b.spec.criteria.push(Criterion {
                                    column: column.clone(),
                                    op: FilterOp::Equal,
                                    value: String::new(),
                                });
                                b.active = Field::Value(b.spec.criteria.len() - 1);
                                b.reveal_value.set(true);
                                b.caret = 0;
                                b.anchor = None;
                                b.history = Default::default();
                            } else {
                                b.error = Some("At most 32 criteria are supported".into());
                            }
                        }
                        this.criteria_changed(cx);
                    })),
            );
        }
        if candidate_count == 0 {
            candidates = candidates.child(if b.columns.is_empty() {
                "No columns are loaded. Refresh the connection metadata."
            } else {
                "No matching columns."
            });
        }
        candidates
    }
    fn render_criteria_rows(&self, b: &CriteriaEditor, cx: &mut Context<Self>) -> Div {
        let mut body = div().flex().flex_col().gap_2();
        for (i, c) in b.spec.criteria.iter().enumerate() {
            let mut row = div()
                .id(("criterion", i))
                .flex()
                .gap_2()
                .items_center()
                .p_1()
                .child(c.column.clone())
                .child(
                    tool(c.op.label()).on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(b) = &mut this.filter_builder {
                            b.operator = if b.operator == Some(i) { None } else { Some(i) };
                        }
                        this.criteria_changed(cx);
                    })),
                )
                .child(tool("Remove").on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(b) = &mut this.filter_builder
                        && i < b.spec.criteria.len()
                    {
                        b.spec.criteria.remove(i);
                        b.active = Field::Search;
                        b.caret = b.search.chars().count();
                        b.anchor = None;
                        b.history = Default::default();
                        b.operator = None;
                    }
                    this.criteria_changed(cx);
                })));
            if c.op.needs_value() {
                row = row.child(self.render_criterion_value(b, i, &c.value, cx));
            }
            body = body.child(row);
            if b.operator == Some(i) {
                let mut menu = div().flex().flex_wrap().gap_2();
                for op in FilterOp::ALL {
                    menu = menu.child(
                        tool(op.label())
                            .id(SharedString::from(format!("op-{i}-{}", op.label())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(b) = &mut this.filter_builder {
                                    b.select_operator(i, op);
                                }
                                this.criteria_changed(cx);
                            })),
                    );
                }
                body = body.child(menu);
            }
        }
        body
    }
    pub(super) fn render_filter_builder(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let b = self.filter_builder.as_ref().unwrap();
        let theme = &self.theme;
        let search_pick = self.criteria_pick(Field::Search, cx);
        let mut body=div().flex().flex_col().gap_2().p_3().text_size(px(12.0)).text_color(theme.text_primary)
            .child("Table filter criteria · Apply replaces the filter text and executes the query")
            .child("Values are literal data. Use IS NULL / IS NOT NULL for NULL; LIKE accepts SQL patterns.")
            .child(ui_editor::line_input::pick_surface(div(),usize::MAX,&search_pick).id("criteria-search")
                .border_1().border_color(theme.border).p_2()
                .child(render_field_content(&b.search,"Search columns",b.active==Field::Search,b.anchor,b.caret,true,theme,&search_pick)))
            .child(div().flex().gap_2()
                .child(tool(if b.spec.match_all{"Match all (AND)"}else{"Match any (OR)"}).on_click(cx.listener(|this,_,_,cx|{
                    if let Some(b)=&mut this.filter_builder{b.spec.match_all= !b.spec.match_all;}this.criteria_changed(cx);
                })))
                .child(tool(if b.order_picker{"Select an ordering column"}else{"Choose order column"}).on_click(cx.listener(|this,_,_,cx|{
                    if let Some(b)=&mut this.filter_builder{b.order_picker= !b.order_picker;}this.criteria_changed(cx);
                }))));
        body = body
            .child(self.render_criteria_columns(b, cx))
            .child("Showing up to 100 matching columns; refine search for larger tables.")
            .child(self.render_criteria_rows(b, cx));
        if let Some(order) = &b.spec.order {
            body = body.child(
                div()
                    .flex()
                    .gap_2()
                    .child(format!("Order by {}", order.column))
                    .child(
                        tool(if order.descending { "DESC" } else { "ASC" }).on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(b) = &mut this.filter_builder
                                    && let Some(order) = &mut b.spec.order
                                {
                                    order.descending = !order.descending;
                                }
                                this.criteria_changed(cx);
                            },
                        )),
                    )
                    .child(tool("Remove order").on_click(cx.listener(|this, _, _, cx| {
                        if let Some(b) = &mut this.filter_builder {
                            b.spec.order = None;
                        }
                        this.criteria_changed(cx);
                    }))),
            );
        }
        if let Some(error) = &b.error {
            body = body.child(div().text_color(rgb(0xf85149)).child(error.clone()));
        }
        let preview = &b.preview;
        let mut preview_view = div().font_family("monospace");
        let chars = b.preview_display.chars().collect::<Vec<_>>();
        for chunk in chars.chunks(90) {
            preview_view = preview_view.child(chunk.iter().collect::<String>());
        }
        body = body.child(preview_view);
        if let Ok(sql) = preview {
            let sql = sql.clone();
            body = body.child(
                tool("Copy full SQL").on_click(cx.listener(move |_, _, _, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string((*sql).clone()));
                })),
            );
        }
        let can_apply = preview.is_ok() && !self.operations.is_running();
        let buttons = div()
            .flex()
            .gap_3()
            .p_3()
            .when(can_apply, |row| {
                row.child(
                    tool("Apply and execute")
                        .on_click(cx.listener(|this, _, _, cx| this.apply_filter_builder(cx))),
                )
            })
            .when(!can_apply, |row| {
                row.child("Fix validation errors or wait for the active operation")
            })
            .child(tool("Cancel").on_click(cx.listener(|this, _, _, cx| {
                this.filter_builder = None;
                this.criteria_changed(cx);
            })));
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x00000088))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(760.0))
                    .max_w(relative(0.95))
                    .h(relative(0.85))
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .bg(theme.bg_surface)
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .child(
                                div()
                                    .id("criteria-scroll")
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_scroll()
                                    .track_scroll(&b.scroll)
                                    .child(body),
                            )
                            .child(properties::grid::detail_scrollbar(
                                b.scroll.clone(),
                                true,
                                theme,
                            )),
                    )
                    .child(properties::grid::detail_scrollbar(
                        b.scroll.clone(),
                        false,
                        theme,
                    ))
                    .child(buttons),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CriteriaEditor, Criterion, DbDriverKind, Field, FilterOp, FilterSpec, ScrollHandle,
        value_window,
    };
    #[test]
    fn null_operator_cannot_undo_a_value_into_column_search() {
        for op in [FilterOp::IsNull, FilterOp::IsNotNull] {
            let mut editor = CriteriaEditor {
                tab: 0,
                owner: String::new(),
                kind: DbDriverKind::Sqlite,
                generation: 0,
                columns: vec!["name".into()],
                spec: FilterSpec {
                    criteria: vec![Criterion {
                        column: "name".into(),
                        op: FilterOp::Equal,
                        value: "secret value".into(),
                    }],
                    ..Default::default()
                },
                search: "na".into(),
                active: Field::Value(0),
                caret: 12,
                anchor: Some(0),
                history: Default::default(),
                operator: Some(0),
                order_picker: false,
                error: None,
                scroll: ScrollHandle::new(),
                preview: Err(String::new()),
                preview_display: String::new(),
                value_scroll: ScrollHandle::new(),
                reveal_value: std::cell::Cell::new(false),
            };
            editor
                .history
                .record(("secret".into(), 6), ui_core::history::EditKind::Other);
            editor.select_operator(0, op);
            assert!(editor.active == Field::Search);
            assert_eq!(editor.caret, 2);
            assert_eq!(editor.anchor, None);
            assert_eq!(editor.spec.criteria[0].op, op);
            assert_eq!(
                editor.history.undo((editor.search.clone(), editor.caret)),
                None
            );
            assert_eq!(editor.search, "na");
        }
    }
    #[test]
    fn large_unicode_values_keep_absolute_caret_and_selection_mapping() {
        let value = "é".repeat(1000);
        let (display, caret, anchor, start, len) = value_window(&value, 1000, Some(0));
        assert_eq!(display.chars().count(), 160);
        assert_eq!(caret, 160);
        assert_eq!(anchor, Some(0));
        assert_eq!(start + caret, 1000);
        assert_eq!(len, 160);
        let (_, caret, anchor, start, _) = value_window(&value, 0, Some(1000));
        assert_eq!(caret, 0);
        assert_eq!(start, 0);
        assert_eq!(anchor, Some(160));
    }
}
