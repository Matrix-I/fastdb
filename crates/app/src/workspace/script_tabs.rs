use super::*;
impl Workspace {
    /// "+ SQL", Cmd+] and the navigator's "SQL Editor": opens the latest script of
    /// `conn_id` (the active connection when `None`), creating one when it has none, or
    /// offers the script chooser when the latest one is already open
    pub fn open_sql_script(&mut self, conn_id: Option<&str>, cx: &mut Context<Self>) {
        let Some(conn_id) = conn_id
            .map(str::to_string)
            .or_else(|| self.connections.active_connection_id.clone())
        else {
            self.form.show_connection_modal = true;
            return;
        };
        let script = match self.script_store.most_recent(&conn_id) {
            Some(recent)
                if self
                    .tabs
                    .iter()
                    .any(|t| t.script_path.as_deref() == Some(recent.path.as_path())) =>
            {
                self.open_script_chooser(&conn_id);
                return;
            }
            Some(recent) => recent,
            None => match self.script_store.create(&conn_id) {
                Ok(script) => script,
                Err(e) => {
                    tracing::warn!("{e:#}");
                    return;
                }
            },
        };
        self.open_script_file(&script, cx);
    }

    /// Activates the tab showing `script`, or reads it into a new tab, on its connection
    pub fn open_script_file(&mut self, script: &ScriptFile, cx: &mut Context<Self>) {
        if let Some(idx) = self
            .tabs
            .iter()
            .position(|t| t.script_path.as_deref() == Some(script.path.as_path()))
        {
            self.active_tab = idx;
        } else {
            let text = match self.script_store.read(script) {
                Ok(text) => text,
                Err(e) => {
                    tracing::warn!("{e:#}");
                    return;
                }
            };
            let id = self.allocate_tab_id();
            self.tabs.push(TabState::script(
                id,
                script,
                &text,
                self.theme.clone(),
                self.connections
                    .session(&script.connection_id)
                    .schema_cache
                    .clone(),
            ));
            self.active_tab = self.tabs.len() - 1;
        }
        self.is_filter_focused = false;
        self.blur_fetch_size();
        self.activate_script_connection(cx);
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
            || !self.connections.active().is_connected
        {
            self.connect_saved_by_id(&conn_id, cx);
        }
    }

    /// Writes a script tab's text to its file when it changed; a failure shows in its banner
    pub fn save_script_tab(&mut self, idx: usize) {
        let Some(tab) = self.tabs.get_mut(idx).filter(|t| t.is_dirty()) else {
            return;
        };
        let Some(path) = tab.script_path.clone() else {
            return;
        };
        let text = tab.editor.buffer.text();
        match self.script_store.write(&path, &text) {
            Ok(()) => tab.saved_text = text,
            Err(e) => tab.last_error = Some(format!("{e:#}")),
        }
    }

    /// Saves every script tab with unsaved changes
    pub fn save_dirty_scripts(&mut self) {
        for idx in 0..self.tabs.len() {
            self.save_script_tab(idx);
        }
    }

    pub fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.close_tabs_where(index, |i| i != index, cx);
        }
    }
}
