use super::*;
impl Workspace {
    /// "+ SQL", Cmd+] and the navigator's "SQL Editor": opens the latest script of
    /// `conn_id` (the active connection when `None`), creating one when it has none, or
    /// offers the script chooser when the latest one is already open
    pub fn open_sql_script(&mut self, conn_id: Option<&str>, cx: &mut Context<Self>) {
        let Some(conn_id) = conn_id
            .map(str::to_owned)
            .or_else(|| self.connections.active_connection_id.clone())
        else {
            self.open_new_connection_dialog();
            cx.notify();
            return;
        };
        let store = self.script_store.clone();
        cx.spawn(async move |this, cx| {
            let id = conn_id.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    store
                        .most_recent(&id)
                        .map(Ok)
                        .unwrap_or_else(|| store.create(&id))
                })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(script) => {
                    if this
                        .tabs
                        .iter()
                        .any(|t| t.script_path.as_ref() == Some(&script.path))
                    {
                        this.open_script_chooser(&conn_id, cx);
                    } else {
                        this.open_script_file(&script, cx);
                    }
                }
                Err(error) => tracing::warn!("{error:#}"),
            });
        })
        .detach();
    }

    pub fn open_script_file(&mut self, script: &ScriptFile, cx: &mut Context<Self>) {
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.script_path.as_ref() == Some(&script.path))
        {
            self.active_tab = idx;
            self.activate_script_connection(cx);
            cx.notify();
            return;
        }
        let script = script.clone();
        let store = self.script_store.clone();
        cx.spawn(async move |this, cx| {
            let file = script.clone();
            let result = cx
                .background_executor()
                .spawn(async move { store.read(&file) })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(text) => this.install_script_tab(&script, &text, cx),
                Err(error) => tracing::warn!("{error:#}"),
            });
        })
        .detach();
    }
    pub(super) fn install_script_tab(
        &mut self,
        script: &ScriptFile,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        if !self
            .connections
            .saved_connections
            .iter()
            .any(|c| c.id == script.connection_id)
        {
            return;
        }
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.script_path.as_ref() == Some(&script.path))
        {
            self.active_tab = idx;
        } else {
            let id = self.allocate_tab_id();
            self.tabs.push(TabState::script(
                id,
                script,
                text,
                self.theme.clone(),
                self.connections
                    .session(&script.connection_id)
                    .schema_cache
                    .clone(),
            ));
            self.active_tab = self.tabs.len() - 1;
        }
        self.input.is_filter_focused = false;
        self.blur_fetch_size();
        self.activate_script_connection(cx);
        self.save_session();
        cx.notify();
    }
    pub(super) fn create_script(&mut self, connection_id: String, cx: &mut Context<Self>) {
        let store = self.script_store.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { store.create(&connection_id) })
                .await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(script) => this.install_script_tab(&script, "", cx),
                Err(error) => tracing::warn!("{error:#}"),
            });
        })
        .detach();
    }

    /// Makes the active script tab's connection the active connection, so it runs there
    pub fn activate_script_connection(&mut self, cx: &mut Context<Self>) {
        let Some(conn_id) = self
            .tabs
            .get(self.active_tab)
            .and_then(|t| t.connection_id.clone())
        else {
            return;
        };
        if self.connections.active_connection_id.as_deref() != Some(conn_id.as_str())
            || !self.connections.active().driver.is_some()
        {
            self.connect_saved_by_id(&conn_id, cx);
        }
    }

    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.close_tabs_where(index, |i| i != index, cx);
        }
    }
}
