use super::script_tools::tool;
use super::*;
use db_wire::column::{ColumnApplyOutcome, ColumnDefinition, ColumnDraft, column_change_sql};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    Name,
    Type,
    Using,
    Default,
    Comment,
}
impl Field {
    fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Type => "Native type",
            Self::Using => "USING expression",
            Self::Default => "Default SQL expression",
            Self::Comment => "Comment",
        }
    }
}
pub(super) struct ColumnEditor {
    pub original: Option<ColumnDefinition>,
    pub draft: Option<ColumnDraft>,
    pub types: Vec<String>,
    pub active: Field,
    pub busy: bool,
    pub request: u64,
    pub preview: Option<Vec<String>>,
    pub refresh_required: bool,
    pub requested_name: String,
    pub(super) source_driver: Option<Arc<dyn DatabaseDriver>>,
}
impl ColumnEditor {
    fn new(name: String) -> Self {
        Self {
            original: None,
            draft: None,
            types: Vec::new(),
            active: Field::Name,
            busy: false,
            request: 0,
            preview: None,
            refresh_required: false,
            requested_name: name,
            source_driver: None,
        }
    }
    pub fn dirty(&self) -> bool {
        self.original
            .as_ref()
            .zip(self.draft.as_ref())
            .is_some_and(|(o, d)| &ColumnDraft::from(o) != d)
    }
    fn value(&self, field: Field) -> String {
        self.draft
            .as_ref()
            .map(|d| match field {
                Field::Name => d.name.clone(),
                Field::Type => d.native_type.clone(),
                Field::Using => d.using_expression.clone(),
                Field::Default => d.default.clone().unwrap_or_default(),
                Field::Comment => d.comment.clone().unwrap_or_default(),
            })
            .unwrap_or_default()
    }
    fn sync(&mut self, value: String) {
        if let Some(d) = &mut self.draft {
            match self.active {
                Field::Name => d.name = value,
                Field::Type => d.native_type = value,
                Field::Using => d.using_expression = value,
                Field::Default => d.default = Some(value),
                Field::Comment => d.comment = Some(value),
            }
        }
    }
    fn editable(&self) -> bool {
        !self.busy
            && !self.refresh_required
            && self.preview.is_none()
            && self.original.as_ref().is_some_and(|o| o.editable)
    }
}
pub(super) type ColumnPick = std::rc::Rc<dyn Fn(String, usize, &mut Window, &mut App)>;

pub(super) struct PendingColumn {
    pub ids: Vec<u64>,
    pub action: AfterSave,
    pub tab: u64,
    pub awaiting_save: bool,
}

impl Workspace {
    pub(super) fn column_open_pick(
        &self,
        owner: String,
        schema: String,
        table: String,
        cx: &mut Context<Self>,
    ) -> ColumnPick {
        let workspace = cx.entity().downgrade();
        std::rc::Rc::new(move |name, clicks, window, cx| {
            let _ = workspace.update(cx, |this, cx| {
                this.column_pick =
                    Some((owner.clone(), schema.clone(), table.clone(), name.clone()));
                this.input.is_filter_focused = false;
                this.input.is_fetch_size_focused = false;
                window.focus(&this.focus_handle);
                if clicks >= 2 {
                    this.column_pick = None;
                    this.open_column(owner.clone(), schema.clone(), table.clone(), name, cx);
                }
                cx.notify();
            });
        })
    }
    pub(super) fn open_column(
        &mut self,
        owner: String,
        schema: String,
        table: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        if let Some(index) = self.tabs.iter().position(|t| {
            t.connection_id.as_ref() == Some(&owner)
                && t.schema_name.as_ref() == Some(&schema)
                && t.table_name.as_ref() == Some(&table)
                && t.column_editor.as_ref().is_some_and(|c| {
                    c.requested_name == name || c.original.as_ref().is_some_and(|o| o.name == name)
                })
        }) {
            self.active_tab = index;
            self.tab_scroll.scroll_to_item(index);
            cx.notify();
            return;
        }
        let id = self.allocate_tab_id();
        let mut tab = TabState::table(
            id,
            &schema,
            &table,
            self.theme.clone(),
            self.connections.session(&owner).schema_cache.clone(),
        );
        tab.title = format!("{table}.{name}");
        tab.connection_id = Some(owner);
        tab.column_editor = Some(ColumnEditor::new(name));
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        self.tab_scroll.scroll_to_item(self.active_tab);
        self.input.is_filter_focused = false;
        self.input.is_fetch_size_focused = false;
        self.load_column_tab(id, cx);
    }
    pub(super) fn load_column_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let Some(driver) = self.tab_session(index).driver.clone() else {
            self.tabs[index].last_error =
                Some("Connection is disconnected; reconnect and Refresh".into());
            cx.notify();
            return;
        };
        let expected_driver = driver.clone();
        let owner = self.tabs[index].connection_id.clone().unwrap_or_default();
        let tab = &mut self.tabs[index];
        let Some(c) = &mut tab.column_editor else {
            return;
        };
        if c.busy {
            return;
        }
        c.busy = true;
        c.request += 1;
        let request = c.request;
        let original = c.original.clone();
        let schema = tab.schema_name.clone().unwrap_or_default();
        let table = tab.table_name.clone().unwrap_or_default();
        let name = c.requested_name.clone();
        tab.last_error = None;
        cx.spawn(async move |this, cx| {
            let result = db_wire::run_on_shared_runtime(async move {
                let definition = match original {
                    Some(original) => driver.reload_column(&original).await?,
                    None => driver.load_column(&schema, &table, &name).await?,
                };
                let types = driver.list_column_types().await.unwrap_or_default();
                Ok((definition, types))
            })
            .await;
            let _ = this.update(cx, |this, cx| {
                let same_connection = this
                    .connections
                    .session(&owner)
                    .driver
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &expected_driver));
                if same_connection
                    && let Ok((definition, _)) = &result
                    && let Some(other) = this
                        .tabs
                        .iter()
                        .find(|t| {
                            t.id != id
                                && t.connection_id.as_deref() == Some(&owner)
                                && t.column_editor
                                    .as_ref()
                                    .and_then(|c| c.original.as_ref())
                                    .is_some_and(|o| {
                                        o.database == definition.database
                                            && o.relation_oid == definition.relation_oid
                                            && o.attribute == definition.attribute
                                    })
                        })
                        .map(|t| t.id)
                {
                    this.tabs.retain(|t| t.id != id);
                    this.active_tab = this.tabs.iter().position(|t| t.id == other).unwrap_or(0);
                    cx.notify();
                    return;
                }
                let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) else {
                    return;
                };
                let Some(c) = &mut tab.column_editor else {
                    return;
                };
                if c.request != request {
                    return;
                }
                c.busy = false;
                if !same_connection {
                    tab.last_error =
                        Some("Connection changed while loading. Reconnect and Refresh".into());
                    cx.notify();
                    return;
                }
                match result {
                    Ok((definition, types)) => {
                        tab.schema_name = Some(definition.schema.clone());
                        tab.table_name = Some(definition.table.clone());
                        tab.title = format!("{}.{}", definition.table, definition.name);
                        c.source_driver = Some(expected_driver.clone());
                        c.requested_name = definition.name.clone();
                        c.draft = Some(ColumnDraft::from(&definition));
                        c.original = Some(definition);
                        c.types = types;
                        c.preview = None;
                        c.refresh_required = false;
                        c.active = Field::Name;
                        tab.editor =
                            SqlEditorView::new(&c.value(c.active), this.theme.clone(), None);
                    }
                    Err(e) => tab.last_error = Some(format!("{e:#}")),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn choose_column_field(&mut self, id: u64, field: Field, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
            && let Some(c) = &mut tab.column_editor
            && c.editable()
        {
            c.active = field;
            tab.editor = SqlEditorView::new(&c.value(field), self.theme.clone(), None);
            tab.last_error = None;
        }
        cx.notify();
    }
    pub(super) fn preview_column(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
            && let Some(c) = &mut tab.column_editor
            && c.editable()
            && let Some((o, d)) = c.original.as_ref().zip(c.draft.as_ref())
        {
            match column_change_sql(o, d) {
                Ok(sql) if !sql.is_empty() => {
                    c.preview = Some(sql);
                    tab.last_error = None;
                }
                Ok(_) => tab.last_error = Some("No column changes".into()),
                Err(e) => tab.last_error = Some(e.to_string()),
            }
        }
        if self
            .column_pending
            .as_ref()
            .is_some_and(|p| p.tab == id && p.awaiting_save)
            && !self.tabs.iter().any(|t| {
                t.id == id
                    && t.column_editor
                        .as_ref()
                        .is_some_and(|c| c.preview.is_some())
            })
        {
            self.column_pending = None;
        }
        cx.notify();
    }
    fn revert_column(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id)
            && let Some(c) = &mut tab.column_editor
            && !c.busy
            && !c.refresh_required
        {
            c.draft = c.original.as_ref().map(ColumnDraft::from);
            c.preview = None;
            tab.editor = SqlEditorView::new(&c.value(c.active), self.theme.clone(), None);
            tab.last_error = None;
        }
        cx.notify();
    }
    fn persist_column_tab(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(index) = self.tabs.iter().position(|t| t.id == id) else {
            return;
        };
        let Some(driver) = self.tab_session(index).driver.clone() else {
            self.tabs[index].last_error = Some("Reconnect before applying DDL".into());
            cx.notify();
            return;
        };
        if driver.is_read_only() {
            self.tabs[index].last_error = Some("This connection is read-only".into());
            cx.notify();
            return;
        }
        let tab = &mut self.tabs[index];
        let Some(c) = &mut tab.column_editor else {
            return;
        };
        if c.busy || c.refresh_required || c.preview.is_none() {
            return;
        }
        if !c
            .source_driver
            .as_ref()
            .is_some_and(|loaded| Arc::ptr_eq(loaded, &driver))
        {
            c.refresh_required = true;
            c.preview = None;
            tab.last_error =
                Some("Connection changed since this snapshot. Refresh before applying DDL".into());
            self.column_pending = None;
            cx.notify();
            return;
        }
        let Some((original, draft)) = c.original.clone().zip(c.draft.clone()) else {
            return;
        };
        let owner = tab.connection_id.clone().unwrap_or_default();
        let Some(run) =
            self.operations
                .begin(id, owner.clone(), driver.clone(), OperationKind::Save)
        else {
            tab.last_error = Some("Wait for the active operation before applying DDL".into());
            cx.notify();
            return;
        };
        c.busy = true;
        let target = (original.schema.clone(), original.table.clone());
        cx.spawn(async move |this,cx|{
            let result=db_wire::run_on_shared_runtime(async move {Ok(driver.apply_column(&original,&draft).await)}).await;
            let _=this.update(cx,|this,cx|{
                if !this.operations.finish(run){return;}
                let Some(tab)=this.tabs.iter_mut().find(|t|t.id==id) else{return;};
                let c=tab.column_editor.as_mut().unwrap();c.busy=false;
                let mut success=false;
                match result{
                    Ok(ColumnApplyOutcome::Applied(definition))=>{
                        let definition=*definition;
                        tab.title=format!("{}.{}",definition.table,definition.name);
                        c.requested_name=definition.name.clone();
                        c.draft=Some(ColumnDraft::from(&definition));c.original=Some(definition);
                        c.preview=None;
                        tab.editor=SqlEditorView::new(&c.value(c.active),this.theme.clone(),None);
                        tab.last_error=None;success=true;
                    }
                    Ok(ColumnApplyOutcome::Rejected(error))=>{tab.last_error=Some(error);c.preview=None;},
                    Ok(ColumnApplyOutcome::ReadbackRequired(error))=>{
                        tab.last_error=Some(error);c.refresh_required=true;c.preview=None;
                    }
                    Err(e)=>{tab.last_error=Some(format!("Schema operation interrupted; Refresh before retry: {e:#}"));c.refresh_required=true;c.preview=None;},
                }
                if !success { this.column_pending=None; }
                if success{
                    for tab in &mut this.tabs{
                        if tab.connection_id.as_deref()==Some(&owner) && tab.column_editor.is_none()
                            && tab.table_name.as_deref()==Some(&target.1) && tab.schema_name.as_deref()==Some(&target.0){
                            tab.last_error=Some("Table schema changed; reload results before editing or saving rows".into());
                            tab.schema_stale=true;
                        }
                    }
                    this.refresh_connection_for(&owner,cx);
                    if let Some(pending)=this.column_pending.take(){
                        this.save_then(pending.ids,pending.action,cx);
                    }
                }
                cx.notify();
            });
        }).detach();
        cx.notify();
    }
    pub(super) fn handle_column_key(
        &mut self,
        event: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return false;
        };
        let Some(c) = &mut tab.column_editor else {
            return false;
        };
        let id = tab.id;
        if (event.keystroke.modifiers.platform || event.keystroke.modifiers.control)
            && event.keystroke.key == "s"
        {
            self.preview_column(id, cx);
        } else if c.editable() {
            tab.editor.handle_input_key(&event.keystroke, cx);
            c.sync(tab.editor.buffer.text());
            cx.notify();
        }
        true
    }
    /// Returns true when a metadata draft/busy operation takes ownership of this action.
    pub(super) fn guard_column_action(
        &mut self,
        ids: &[u64],
        action: &AfterSave,
        cx: &mut Context<Self>,
    ) -> bool {
        if matches!(
            action,
            AfterSave::None
                | AfterSave::Run(_)
                | AfterSave::RunScript(_)
                | AfterSave::Explain(_, _)
        ) {
            return false;
        }
        if let Some(index) = self.tabs.iter().position(|t| {
            ids.contains(&t.id)
                && t.column_editor
                    .as_ref()
                    .is_some_and(|c| c.busy || c.dirty() || c.refresh_required)
        }) {
            self.active_tab = index;
            let tab = &mut self.tabs[index];
            let c = tab.column_editor.as_ref().unwrap();
            if c.busy {
                tab.last_error = Some("Wait for the column operation to finish".into());
            } else if self.column_pending.is_none() {
                self.column_pending = Some(PendingColumn {
                    ids: ids.to_vec(),
                    action: action.clone(),
                    tab: tab.id,
                    awaiting_save: false,
                });
            }
            cx.notify();
            return true;
        }
        false
    }
    pub(super) fn render_column_prompt(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pending = self.column_pending.as_ref().unwrap();
        let id = pending.tab;
        let can_save = self
            .tabs
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.column_editor.as_ref())
            .is_some_and(|c| {
                !c.busy && !c.refresh_required && c.original.as_ref().is_some_and(|o| o.editable)
            });
        div()
            .absolute()
            .inset_0()
            .occlude()
            .bg(rgba(0x00000088))
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .w(px(440.0))
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .bg(self.theme.bg_surface)
                    .text_color(self.theme.text_primary)
                    .child("This column has an unsaved draft or unconfirmed database state.")
                    .child("Save opens the SQL preview. Discard abandons the local draft.")
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                tool(if can_save {
                                    "Save"
                                } else {
                                    "Refresh (discard draft)"
                                })
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        if !can_save {
                                            this.column_pending = None;
                                            this.load_column_tab(id, cx);
                                            return;
                                        }
                                        if let Some(p) = &mut this.column_pending {
                                            p.awaiting_save = true;
                                        }
                                        this.preview_column(id, cx);
                                    },
                                )),
                            )
                            .child(tool("Discard").on_click(cx.listener(move |this, _, _, cx| {
                                let Some(p) = this.column_pending.take() else {
                                    return;
                                };
                                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == p.tab)
                                    && let Some(c) = &mut tab.column_editor
                                {
                                    c.draft = c.original.as_ref().map(ColumnDraft::from);
                                    c.refresh_required = false;
                                    c.preview = None;
                                    tab.editor = SqlEditorView::new(
                                        &c.value(c.active),
                                        this.theme.clone(),
                                        None,
                                    );
                                }
                                this.save_then(p.ids, p.action, cx);
                            })))
                            .child(tool("Cancel").on_click(cx.listener(|this, _, _, cx| {
                                this.column_pending = None;
                                cx.notify();
                            }))),
                    ),
            )
    }
    pub(super) fn render_column_editor(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let tab = &self.tabs[index];
        let id = tab.id;
        let c = tab.column_editor.as_ref().unwrap();
        let scroll = tab.properties_scroll.clone();
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .text_color(theme.text_primary)
            .text_size(px(12.0))
            .child(format!(
                "Column · {} · {}",
                tab.title,
                if c.dirty() {
                    "Unsaved draft"
                } else {
                    "Catalog snapshot"
                }
            ));
        let mut tools = div()
            .flex()
            .gap_2()
            .child(tool("Refresh").on_click(cx.listener(move |this, _, _, cx| {
                this.save_then(vec![id], AfterSave::RefreshColumn(id), cx);
            })));
        if c.editable() {
            tools = tools
                .child(
                    tool("Save / Preview SQL")
                        .on_click(cx.listener(move |this, _, _, cx| this.preview_column(id, cx))),
                )
                .child(
                    tool("Revert")
                        .on_click(cx.listener(move |this, _, _, cx| this.revert_column(id, cx))),
                );
        }
        body = body.child(tools);
        if c.busy {
            body = body.child("Loading/applying column definition…");
        }
        if let Some(error) = &tab.last_error {
            body = body.child(div().text_color(rgb(0xf85149)).child(error.clone()));
        }
        if let Some(o) = &c.original {
            body=body.child(format!("{}.{}.{} · table OID {} · ordinal {} · identity: {} · generated: {} · collation: {} · storage: {} · local: {}",
                o.database,o.schema,o.table,o.relation_oid,o.attribute,
                if o.identity.is_empty(){"None"}else{&o.identity},
                o.generation_expression.as_deref().unwrap_or("None"),
                o.collation.as_deref().unwrap_or("Unavailable"),o.storage,o.local));
            body=body.child("Identity, generated expressions, collation, storage and permissions are read-only in this editor.");
            if !o.editable {
                body = body.child(o.restriction.clone());
            }
        }
        if let Some(d) = &c.draft {
            for field in [
                Field::Name,
                Field::Type,
                Field::Using,
                Field::Default,
                Field::Comment,
            ] {
                body = body.child(
                    div()
                        .id(field.label())
                        .p_2()
                        .border_1()
                        .border_color(if c.active == field {
                            theme.border_focused
                        } else {
                            theme.border
                        })
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.choose_column_field(id, field, cx)
                        }))
                        .child(format!("{}: {}", field.label(), c.value(field))),
                );
            }
            let nullable = d.nullable;
            let default = d.default.is_some();
            let comment = d.comment.is_some();
            if c.editable() {
                for (kind, label, on) in [
                    (0, "Nullable", nullable),
                    (1, "Default expression enabled", default),
                    (2, "Comment enabled", comment),
                ] {
                    body = body.child(
                        div()
                            .id(("column-toggle", kind as usize))
                            .cursor_pointer()
                            .child(format!("[{}] {label}", if on { "x" } else { " " }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id)
                                    && let Some(c) = &mut tab.column_editor
                                    && c.editable()
                                    && let Some(d) = &mut c.draft
                                {
                                    match kind {
                                        0 => d.nullable = !on,
                                        1 => d.default = (!on).then(String::new),
                                        _ => d.comment = (!on).then(String::new),
                                    }
                                    if (kind == 1 && c.active == Field::Default)
                                        || (kind == 2 && c.active == Field::Comment)
                                    {
                                        tab.editor = SqlEditorView::new(
                                            &c.value(c.active),
                                            this.theme.clone(),
                                            None,
                                        );
                                    }
                                }
                                cx.notify();
                            })),
                    );
                }
            }
            if c.active == Field::Type && c.editable() {
                for choice in c
                    .types
                    .iter()
                    .filter(|t| t.to_lowercase().contains(&d.native_type.to_lowercase()))
                    .take(12)
                {
                    let choice = choice.clone();
                    body = body.child(
                        div()
                            .id(SharedString::from(choice.clone()))
                            .cursor_pointer()
                            .child(choice.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id)
                                    && let Some(c) = &mut tab.column_editor
                                    && c.editable()
                                {
                                    c.sync(choice.clone());
                                    tab.editor =
                                        SqlEditorView::new(&choice, this.theme.clone(), None);
                                }
                                cx.notify();
                            })),
                    );
                }
            }
        }
        if let Some(sql) = &c.preview {
            body=body.child("Review DDL. Type changes can rewrite data; NOT NULL can scan/lock the table. Persist uses a dedicated transaction.");
            let copy = sql.join("\n");
            body = body.child(
                div()
                    .flex()
                    .gap_2()
                    .child(tool("Copy SQL").on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()))
                    })))
                    .child(tool("Persist").on_click(
                        cx.listener(move |this, _, _, cx| this.persist_column_tab(id, cx)),
                    ))
                    .child(
                        tool("Cancel preview").on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id)
                                && let Some(c) = &mut tab.column_editor
                                && !c.busy
                            {
                                c.preview = None;
                            }
                            this.column_pending = None;
                            cx.notify();
                        })),
                    ),
            );
            for line in sql.join("\n").lines() {
                body = body.child(div().font_family("monospace").child(line.to_string()));
            }
        }
        let editable = c.editable();
        let active = c.active.label();
        let pick = self.script_editor_pick(index, cx);
        let tab = &mut self.tabs[index];
        let editor_scroll = tab.editor.scroll_handle.clone();
        let mut panel = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_hidden()
            .bg(theme.bg_editor)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        div()
                            .id(("column-scroll", id))
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .overflow_scroll()
                            .track_scroll(&scroll)
                            .child(body),
                    )
                    .child(properties::grid::detail_scrollbar(
                        scroll.clone(),
                        true,
                        &theme,
                    )),
            )
            .child(properties::grid::detail_scrollbar(scroll, false, &theme));
        if editable {
            panel=panel.child(div().p_2().text_color(theme.text_secondary).text_size(px(11.0)).child(format!("Editing {active} · SQL expressions use SQL syntax; comments are plain multiline text")))
                .child(div().flex().flex_none().h(px(180.0)).min_w_0()
                    .child(div().flex_1().min_w_0().overflow_hidden()
                        .on_mouse_down(MouseButton::Left,cx.listener(|this,_,window,_|window.focus(&this.focus_handle)))
                        .child(tab.editor.render_editor(true,true,pick,window)))
                    .child(properties::grid::detail_scrollbar(editor_scroll.clone(),true,&theme)))
                .child(properties::grid::detail_scrollbar(editor_scroll,false,&theme));
        }
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::{ColumnDefinition, ColumnDraft, ColumnEditor, Field, column_change_sql};
    fn state() -> ColumnEditor {
        let original = ColumnDefinition {
            name: "host".into(),
            native_type: "character varying(50)".into(),
            nullable: true,
            editable: true,
            ..Default::default()
        };
        let mut c = ColumnEditor::new("host".into());
        c.draft = Some(ColumnDraft::from(&original));
        c.original = Some(original);
        c
    }
    #[test]
    fn drafts_preserve_multiline_values_and_preview_freezes_edits() {
        let mut c = state();
        assert!(!c.dirty());
        c.active = Field::Comment;
        c.sync("one\ntwo".into());
        assert_eq!(c.value(Field::Comment), "one\ntwo");
        assert!(c.dirty());
        c.preview = Some(
            column_change_sql(c.original.as_ref().unwrap(), c.draft.as_ref().unwrap()).unwrap(),
        );
        assert!(!c.editable());
        c.preview = None;
        assert!(c.editable());
        c.refresh_required = true;
        assert!(!c.editable());
        assert!(c.dirty());
        c.refresh_required = false;
        c.draft = c.original.as_ref().map(ColumnDraft::from);
        assert!(!c.dirty());
    }
}
