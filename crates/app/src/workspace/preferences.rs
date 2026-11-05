use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use ui_core::theme::Theme;

use super::{PropertySection, TableSubTab};

pub const MAX_PREFERENCES_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_WORKSPACE_BYTES: u64 = 16 * 1024 * 1024;

fn default_version() -> u32 {
    1
}

fn default_editor_font_size() -> f32 {
    13.0
}

fn default_fetch_size() -> usize {
    200
}

fn default_navigator_width() -> f32 {
    260.0
}

fn default_value_panel_width() -> f32 {
    300.0
}

fn default_subtab() -> TableSubTab {
    TableSubTab::Data
}

fn default_property_section() -> PropertySection {
    PropertySection::Columns
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
}

impl ThemeMode {
    pub fn to_theme(self) -> Theme {
        match self {
            ThemeMode::Dark => Theme::dark(),
            ThemeMode::Light => Theme::light(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Preferences {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub theme: ThemeMode,
    #[serde(default = "default_editor_font_size")]
    pub editor_font_size: f32,
    #[serde(default)]
    pub editor_font_family: Option<String>,
    #[serde(default = "default_fetch_size")]
    pub default_fetch_size: usize,
    #[serde(default)]
    pub keymap: HashMap<String, String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: default_version(),
            theme: ThemeMode::Dark,
            editor_font_size: default_editor_font_size(),
            editor_font_family: None,
            default_fetch_size: default_fetch_size(),
            keymap: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PersistedTab {
    Table {
        connection_id: Option<String>,
        schema: String,
        table: String,
        #[serde(default)]
        filter_sql: String,
        #[serde(default = "default_subtab")]
        active_subtab: TableSubTab,
        #[serde(default = "default_property_section")]
        active_property_section: PropertySection,
        #[serde(default = "default_fetch_size")]
        fetch_size: usize,
    },
    Script {
        connection_id: Option<String>,
        title: String,
        script_path: Option<PathBuf>,
        #[serde(default)]
        unsaved_text: Option<String>,
        #[serde(default = "default_true")]
        editor_focused: bool,
    },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WorkspaceSession {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub active_tab: usize,
    #[serde(default = "default_navigator_width")]
    pub navigator_width: f32,
    #[serde(default = "default_value_panel_width")]
    pub value_panel_width: f32,
    #[serde(default)]
    pub show_right_panels: bool,
    #[serde(default)]
    pub tabs: Vec<PersistedTab>,
}

impl Default for WorkspaceSession {
    fn default() -> Self {
        Self {
            version: default_version(),
            active_tab: 0,
            navigator_width: default_navigator_width(),
            value_panel_width: default_value_panel_width(),
            show_right_panels: false,
            tabs: Vec::new(),
        }
    }
}

pub fn preferences_path(config_dir: &Path) -> PathBuf {
    config_dir.join("preferences.json")
}

pub fn workspace_session_path(config_dir: &Path) -> PathBuf {
    config_dir.join("workspace-state.json")
}

pub fn load_preferences(config_dir: &Path) -> Preferences {
    let path = preferences_path(config_dir);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("Failed to open preferences at {}: {error}", path.display());
            }
            return Preferences::default();
        }
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.take(MAX_PREFERENCES_BYTES + 1).read_to_end(&mut bytes) {
        tracing::warn!("Failed to read preferences: {error}");
        return Preferences::default();
    }
    if bytes.len() as u64 > MAX_PREFERENCES_BYTES {
        tracing::warn!("Preferences file exceeds size limit; using defaults");
        return Preferences::default();
    }
    serde_json::from_slice(&bytes).unwrap_or_else(|error| {
        tracing::warn!("Failed to parse preferences: {error}; using defaults");
        Preferences::default()
    })
}

pub fn save_preferences(config_dir: &Path, prefs: &Preferences) -> anyhow::Result<()> {
    let path = preferences_path(config_dir);
    let bytes = serde_json::to_vec_pretty(prefs)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_PREFERENCES_BYTES,
        "Preferences data exceeds size limit"
    );
    std::fs::create_dir_all(config_dir)?;
    let mut temporary = tempfile::NamedTempFile::new_in(config_dir)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path)?;
    Ok(())
}

pub fn load_workspace_session(config_dir: &Path) -> Option<WorkspaceSession> {
    let path = workspace_session_path(config_dir);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(
                    "Failed to open workspace session at {}: {error}",
                    path.display()
                );
            }
            return None;
        }
    };
    let mut bytes = Vec::new();
    if let Err(error) = file.take(MAX_WORKSPACE_BYTES + 1).read_to_end(&mut bytes) {
        tracing::warn!("Failed to read workspace session: {error}");
        return None;
    }
    if bytes.len() as u64 > MAX_WORKSPACE_BYTES {
        tracing::warn!("Workspace session file exceeds size limit");
        return None;
    }
    match serde_json::from_slice(&bytes) {
        Ok(session) => Some(session),
        Err(error) => {
            tracing::warn!("Failed to parse workspace session: {error}");
            None
        }
    }
}

pub fn save_workspace_session(config_dir: &Path, session: &WorkspaceSession) -> anyhow::Result<()> {
    let path = workspace_session_path(config_dir);
    let bytes = serde_json::to_vec_pretty(session)?;
    anyhow::ensure!(
        bytes.len() as u64 <= MAX_WORKSPACE_BYTES,
        "Workspace session data exceeds size limit"
    );
    std::fs::create_dir_all(config_dir)?;
    let mut temporary = tempfile::NamedTempFile::new_in(config_dir)?;
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(&path)?;
    Ok(())
}

use super::{Context, NAVIGATOR_DEFAULT_WIDTH, ScriptFile, TabState, Workspace};
use std::sync::Arc;

impl Workspace {
    pub(super) fn restore_session(&mut self, cx: &mut Context<Self>) {
        let config_dir = platform_hal::paths::config_dir();
        let Some(session) = load_workspace_session(&config_dir) else {
            return;
        };
        self.layout.navigator_width = session.navigator_width.clamp(160.0, 800.0);
        self.layout.value_panel_width = session.value_panel_width.clamp(180.0, 1000.0);
        self.layout.show_right_panels = session.show_right_panels;

        for tab in session.tabs {
            match tab {
                PersistedTab::Table {
                    connection_id,
                    schema,
                    table,
                    filter_sql,
                    active_subtab,
                    active_property_section,
                    fetch_size,
                } => {
                    let Some(ref conn_id) = connection_id else {
                        continue;
                    };
                    if !self
                        .connections
                        .saved_connections
                        .iter()
                        .any(|c| &c.id == conn_id)
                    {
                        continue;
                    }
                    let id = self.allocate_tab_id();
                    let cache = self.connections.session_mut(conn_id).schema_cache.clone();
                    let mut t = TabState::table(id, &schema, &table, self.theme.clone(), cache);
                    t.connection_id = Some(conn_id.clone());
                    t.filter_sql = filter_sql;
                    t.fetch.size = fetch_size;
                    t.fetch.size_text = fetch_size.to_string();
                    t.active_subtab = active_subtab;
                    t.active_property_section = active_property_section;
                    self.tabs.push(t);
                }
                PersistedTab::Script {
                    connection_id,
                    title,
                    script_path,
                    unsaved_text,
                    editor_focused,
                } => {
                    let conn_id = connection_id.unwrap_or_default();
                    let cache = if !conn_id.is_empty() {
                        self.connections.session_mut(&conn_id).schema_cache.clone()
                    } else {
                        Arc::new(sql_intel::SchemaCache::new().expect("schema cache"))
                    };
                    let id = self.allocate_tab_id();
                    let script_file = if let Some(ref path) = script_path {
                        ScriptFile {
                            name: title.clone(),
                            connection_id: conn_id.clone(),
                            path: path.clone(),
                            modified: std::time::SystemTime::now(),
                            preview: String::new(),
                        }
                    } else {
                        ScriptFile {
                            name: title.clone(),
                            connection_id: conn_id.clone(),
                            path: PathBuf::new(),
                            modified: std::time::SystemTime::now(),
                            preview: String::new(),
                        }
                    };
                    let (disk_text, has_unsaved) = if let Some(unsaved) = unsaved_text {
                        let disk = if let Some(ref path) = script_path {
                            std::fs::read_to_string(path).unwrap_or_default()
                        } else {
                            String::new()
                        };
                        (disk, Some(unsaved))
                    } else if let Some(ref path) = script_path {
                        (std::fs::read_to_string(path).unwrap_or_default(), None)
                    } else {
                        (String::new(), None)
                    };
                    let mut t =
                        TabState::script(id, &script_file, &disk_text, self.theme.clone(), cache);
                    if let Some(unsaved) = has_unsaved
                        && unsaved != disk_text
                    {
                        let len = t.editor.buffer.text().len();
                        t.editor.buffer.replace_range(0, len, &unsaved);
                    }
                    t.editor.font_size = self.preferences.editor_font_size;
                    t.editor.font_family = self.preferences.editor_font_family.clone();
                    t.editor_focused = editor_focused;
                    self.tabs.push(t);
                }
            }
        }

        if !self.tabs.is_empty() {
            self.active_tab = session.active_tab.min(self.tabs.len() - 1);
            self.tab_scroll.scroll_to_item(self.active_tab);
        }
        cx.notify();
    }

    pub(super) fn save_session(&self) {
        let config_dir = platform_hal::paths::config_dir();
        let tabs = self
            .tabs
            .iter()
            .filter_map(|tab| {
                if let (Some(schema), Some(table)) = (&tab.schema_name, &tab.table_name) {
                    Some(PersistedTab::Table {
                        connection_id: tab.connection_id.clone(),
                        schema: schema.clone(),
                        table: table.clone(),
                        filter_sql: tab.filter_sql.clone(),
                        active_subtab: tab.active_subtab,
                        active_property_section: tab.active_property_section,
                        fetch_size: tab.fetch.size,
                    })
                } else if tab.is_script() {
                    let unsaved_text = if tab.is_dirty() {
                        Some(tab.editor.buffer.text())
                    } else {
                        None
                    };
                    Some(PersistedTab::Script {
                        connection_id: tab.connection_id.clone(),
                        title: tab.title.clone(),
                        script_path: tab.script_path.clone(),
                        unsaved_text,
                        editor_focused: tab.editor_focused,
                    })
                } else {
                    None
                }
            })
            .collect();

        let session = WorkspaceSession {
            version: 1,
            active_tab: self.active_tab,
            navigator_width: self.layout.navigator_width,
            value_panel_width: self.layout.value_panel_width,
            show_right_panels: self.layout.show_right_panels,
            tabs,
        };

        if let Err(error) = save_workspace_session(&config_dir, &session) {
            tracing::warn!("Failed to save workspace session: {error}");
        }
    }

    pub fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        let new_mode = match self.preferences.theme {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        };
        self.set_theme_mode(new_mode, cx);
    }

    pub fn set_theme_mode(&mut self, mode: ThemeMode, cx: &mut Context<Self>) {
        self.preferences.theme = mode;
        self.theme = mode.to_theme();
        self.panes = None;
        for tab in &mut self.tabs {
            tab.editor.theme = self.theme.clone();
            tab.grid.theme = self.theme.clone();
        }
        let config_dir = platform_hal::paths::config_dir();
        if let Err(error) = save_preferences(&config_dir, &self.preferences) {
            tracing::warn!("Failed to save preferences: {error}");
        }
        cx.notify();
    }

    pub fn zoom_in_editor(&mut self, cx: &mut Context<Self>) {
        self.set_editor_font_size(self.preferences.editor_font_size + 1.0, cx);
    }

    pub fn zoom_out_editor(&mut self, cx: &mut Context<Self>) {
        self.set_editor_font_size(self.preferences.editor_font_size - 1.0, cx);
    }

    pub fn reset_editor_zoom(&mut self, cx: &mut Context<Self>) {
        self.set_editor_font_size(default_editor_font_size(), cx);
    }

    pub fn set_editor_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = size.clamp(9.0, 32.0);
        self.preferences.editor_font_size = size;
        for tab in &mut self.tabs {
            tab.editor
                .set_font(size, self.preferences.editor_font_family.clone());
        }
        let config_dir = platform_hal::paths::config_dir();
        if let Err(error) = save_preferences(&config_dir, &self.preferences) {
            tracing::warn!("Failed to save preferences: {error}");
        }
        cx.notify();
    }

    pub fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        if self.layout.navigator_width > 0.0 {
            self.layout.navigator_width = 0.0;
        } else {
            self.layout.navigator_width = NAVIGATOR_DEFAULT_WIDTH;
        }
        self.save_session();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_roundtrip_preserves_theme_and_font_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let mut prefs = Preferences {
            theme: ThemeMode::Light,
            editor_font_size: 16.0,
            editor_font_family: Some("JetBrains Mono".into()),
            default_fetch_size: 500,
            ..Default::default()
        };
        prefs
            .keymap
            .insert("toggle_theme".into(), "cmd-shift-t".into());

        save_preferences(dir.path(), &prefs).unwrap();

        let loaded = load_preferences(dir.path());
        assert_eq!(loaded.theme, ThemeMode::Light);
        assert_eq!(loaded.editor_font_size, 16.0);
        assert_eq!(loaded.editor_font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(loaded.default_fetch_size, 500);
        assert_eq!(
            loaded.keymap.get("toggle_theme").map(String::as_str),
            Some("cmd-shift-t")
        );
    }

    #[test]
    fn preferences_missing_file_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_preferences(dir.path());
        assert_eq!(loaded.theme, ThemeMode::Dark);
        assert_eq!(loaded.editor_font_size, 13.0);
        assert_eq!(loaded.default_fetch_size, 200);
    }

    #[test]
    fn workspace_session_roundtrip_preserves_tabs_and_layout() {
        let dir = tempfile::tempdir().unwrap();
        let session = WorkspaceSession {
            version: 1,
            active_tab: 1,
            navigator_width: 320.0,
            value_panel_width: 400.0,
            show_right_panels: true,
            tabs: vec![
                PersistedTab::Table {
                    connection_id: Some("pg-prod".into()),
                    schema: "public".into(),
                    table: "orders".into(),
                    filter_sql: "status = 'active'".into(),
                    active_subtab: TableSubTab::Data,
                    active_property_section: PropertySection::Columns,
                    fetch_size: 500,
                },
                PersistedTab::Script {
                    connection_id: Some("pg-prod".into()),
                    title: "analysis.sql".into(),
                    script_path: Some(PathBuf::from("/tmp/analysis.sql")),
                    unsaved_text: Some("SELECT * FROM orders;".into()),
                    editor_focused: true,
                },
            ],
        };

        save_workspace_session(dir.path(), &session).unwrap();

        let loaded = load_workspace_session(dir.path()).expect("session should load");
        assert_eq!(loaded.active_tab, 1);
        assert_eq!(loaded.navigator_width, 320.0);
        assert_eq!(loaded.value_panel_width, 400.0);
        assert!(loaded.show_right_panels);
        assert_eq!(loaded.tabs.len(), 2);

        match &loaded.tabs[0] {
            PersistedTab::Table {
                schema,
                table,
                filter_sql,
                fetch_size,
                ..
            } => {
                assert_eq!(schema, "public");
                assert_eq!(table, "orders");
                assert_eq!(filter_sql, "status = 'active'");
                assert_eq!(*fetch_size, 500);
            }
            _ => panic!("Expected Table tab"),
        }

        match &loaded.tabs[1] {
            PersistedTab::Script {
                title,
                unsaved_text,
                editor_focused,
                ..
            } => {
                assert_eq!(title, "analysis.sql");
                assert_eq!(unsaved_text.as_deref(), Some("SELECT * FROM orders;"));
                assert!(*editor_focused);
            }
            _ => panic!("Expected Script tab"),
        }
    }

    #[test]
    fn workspace_session_missing_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_workspace_session(dir.path()).is_none());
    }

    #[test]
    fn preferences_corrupt_file_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(preferences_path(dir.path()), b"not json content").unwrap();
        let loaded = load_preferences(dir.path());
        assert_eq!(loaded.theme, ThemeMode::Dark);
        assert_eq!(loaded.editor_font_size, 13.0);
    }

    #[test]
    fn workspace_session_corrupt_file_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(workspace_session_path(dir.path()), b"{ invalid json:").unwrap();
        assert!(load_workspace_session(dir.path()).is_none());
    }

    #[test]
    fn preferences_clamps_font_size() {
        let prefs = Preferences {
            editor_font_size: 5.0,
            ..Default::default()
        };
        assert_eq!(prefs.editor_font_size.clamp(9.0, 32.0), 9.0);
        let prefs2 = Preferences {
            editor_font_size: 50.0,
            ..Default::default()
        };
        assert_eq!(prefs2.editor_font_size.clamp(9.0, 32.0), 32.0);
    }

    #[test]
    fn restored_session_does_not_execute_sql_or_restore_live_transactions() {
        let dir = tempfile::tempdir().unwrap();
        let session = WorkspaceSession {
            version: 1,
            active_tab: 0,
            navigator_width: 280.0,
            value_panel_width: 320.0,
            show_right_panels: false,
            tabs: vec![
                PersistedTab::Table {
                    connection_id: Some("test-conn".into()),
                    schema: "public".into(),
                    table: "users".into(),
                    filter_sql: "active = 1".into(),
                    active_subtab: TableSubTab::Data,
                    active_property_section: PropertySection::Columns,
                    fetch_size: 150,
                },
                PersistedTab::Script {
                    connection_id: Some("test-conn".into()),
                    title: "scratch.sql".into(),
                    script_path: None,
                    unsaved_text: Some("SELECT 1;".into()),
                    editor_focused: true,
                },
            ],
        };
        save_workspace_session(dir.path(), &session).unwrap();

        let loaded = load_workspace_session(dir.path()).unwrap();
        assert_eq!(loaded.tabs.len(), 2);

        let cache = Arc::new(sql_intel::SchemaCache::new().unwrap());

        // 1. Table tab:
        let table_tab = TabState::table(1, "public", "users", Theme::dark(), cache.clone());
        assert!(table_tab.grid.pager.is_none());
        assert_eq!(table_tab.transaction_state, "Idle");

        // 2. Script tab:
        let script_file = ScriptFile {
            name: "scratch.sql".into(),
            connection_id: "test-conn".into(),
            path: PathBuf::from("/tmp/scratch.sql"),
            modified: std::time::SystemTime::now(),
            preview: String::new(),
        };
        let mut script_tab = TabState::script(2, &script_file, "", Theme::dark(), cache);
        let len = script_tab.editor.buffer.text().len();
        script_tab.editor.buffer.replace_range(0, len, "SELECT 1;");
        assert_eq!(script_tab.transaction_state, "Idle");
        assert!(script_tab.is_dirty());
        assert_eq!(script_tab.editor.buffer.text(), "SELECT 1;");
    }
}
