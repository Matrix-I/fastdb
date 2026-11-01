use super::text_input::LineHistory;
use super::{FormField, SavedConnection};
use db_wire::{ColumnMeta, ConnectionConfig, DatabaseDriver, DbDriverKind, TableSummary};
use sql_intel::SchemaCache;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(super) struct ConnectionFormState {
    pub request_id: u64,
    pub editing_connection_id: Option<String>,
    pub is_connecting: bool,
    pub show_connection_modal: bool,
    pub selected_driver_kind: DbDriverKind,
    pub form_name: String,
    pub form_host: String,
    pub form_port: String,
    pub form_database: String,
    pub form_user: String,
    pub form_password: String,
    pub form_use_unix_socket: bool,
    pub form_tls_enabled: bool,
    pub form_tls_ca_cert: String,
    pub active_field: Option<FormField>,
    pub form_history: HashMap<FormField, LineHistory>,
    pub show_password: bool,
    pub test_result: Option<Result<String, String>>,
    pub field_anchor: Option<usize>,
    pub field_cursor: usize,
}

pub(super) struct NavigatorState {
    pub nav_connection_expanded: bool,
    pub nav_databases_expanded: bool,
    pub nav_database_expanded: bool,
    pub nav_schemas_expanded: bool,
    pub nav_schema_expanded: HashSet<String>,
    pub nav_tables_expanded: HashSet<String>,
    pub nav_table_expanded: HashSet<String>,
    pub nav_subfolder_expanded: HashSet<String>,
    pub selected_table_name: Option<String>,
}
impl Default for NavigatorState {
    fn default() -> Self {
        Self {
            nav_connection_expanded: false,
            nav_databases_expanded: true,
            nav_database_expanded: true,
            nav_schemas_expanded: true,
            nav_schema_expanded: {
                let mut s = HashSet::new();
                s.insert("public".to_string());
                s
            },
            nav_tables_expanded: {
                let mut s = HashSet::new();
                s.insert("public".to_string());
                s
            },
            nav_table_expanded: HashSet::new(),
            nav_subfolder_expanded: HashSet::new(),
            selected_table_name: None,
        }
    }
}

pub(super) struct ConnectionSession {
    pub kind: DbDriverKind,
    pub driver: Option<Arc<dyn DatabaseDriver>>,
    pub schema_cache: Arc<SchemaCache>,
    pub tables: Vec<TableSummary>,
    pub table_columns: HashMap<(String, String), Vec<ColumnMeta>>,
    pub table_constraints: HashMap<(String, String), Vec<(String, String)>>,
    pub table_indexes: HashMap<(String, String), Vec<String>>,
    pub is_connected: bool,
}
impl Default for ConnectionSession {
    fn default() -> Self {
        Self {
            kind: DbDriverKind::Postgres,
            driver: None,
            schema_cache: Arc::new(SchemaCache::new().expect("schema cache")),
            tables: Vec::new(),
            table_columns: HashMap::new(),
            table_constraints: HashMap::new(),
            table_indexes: HashMap::new(),
            is_connected: false,
        }
    }
}

pub(super) struct ConnectionManager {
    pub saved_connections: Vec<SavedConnection>,
    pub active_connection_id: Option<String>,
    pub sessions: HashMap<String, ConnectionSession>,
    navigators: HashMap<String, NavigatorState>,
    empty: ConnectionSession,
    empty_nav: NavigatorState,
}
impl ConnectionManager {
    pub fn new(
        saved_connections: Vec<SavedConnection>,
        active_connection_id: Option<String>,
    ) -> Self {
        Self {
            saved_connections,
            active_connection_id,
            sessions: HashMap::new(),
            navigators: HashMap::new(),
            empty: ConnectionSession::default(),
            empty_nav: NavigatorState::default(),
        }
    }
    pub fn active(&self) -> &ConnectionSession {
        self.active_connection_id
            .as_ref()
            .and_then(|id| self.sessions.get(id))
            .unwrap_or(&self.empty)
    }
    pub fn active_mut(&mut self) -> &mut ConnectionSession {
        match self.active_connection_id.as_ref() {
            Some(id) => self.sessions.entry(id.clone()).or_default(),
            None => &mut self.empty,
        }
    }
    pub fn navigator(&self) -> &NavigatorState {
        self.active_connection_id
            .as_ref()
            .and_then(|id| self.navigators.get(id))
            .unwrap_or(&self.empty_nav)
    }
    pub fn navigator_mut(&mut self) -> &mut NavigatorState {
        match self.active_connection_id.as_ref() {
            Some(id) => self.navigators.entry(id.clone()).or_default(),
            None => &mut self.empty_nav,
        }
    }
}

impl ConnectionFormState {
    pub fn snapshot(&self) -> Vec<String> {
        vec![
            self.editing_connection_id.clone().unwrap_or_default(),
            self.selected_driver_kind.name().into(),
            self.form_name.clone(),
            self.form_host.clone(),
            self.form_port.clone(),
            self.form_database.clone(),
            self.form_user.clone(),
            self.form_password.clone(),
            self.form_use_unix_socket.to_string(),
            self.form_tls_enabled.to_string(),
            self.form_tls_ca_cert.clone(),
            self.show_connection_modal.to_string(),
        ]
    }
    pub fn invalidate(&mut self) {
        self.request_id += 1;
        self.is_connecting = false;
    }
    pub fn begin_request(&mut self) -> u64 {
        self.request_id += 1;
        self.request_id
    }
    pub fn accepts(&self, id: u64, snapshot: &[String]) -> bool {
        self.request_id == id && self.snapshot() == snapshot
    }
}
impl ConnectionManager {
    pub fn session(&self, id: &str) -> &ConnectionSession {
        self.sessions.get(id).unwrap_or(&self.empty)
    }
}

impl ConnectionFormState {
    pub fn build_config(&self) -> Result<ConnectionConfig, String> {
        let port = if self.form_port.trim().is_empty() {
            self.selected_driver_kind.default_port()
        } else {
            self.form_port
                .parse::<u16>()
                .map_err(|_| "Port must be a number between 0 and 65535".to_string())?
        };

        Ok(ConnectionConfig {
            driver: self.selected_driver_kind,
            host: self.form_host.clone(),
            port,
            database: self.form_database.clone(),
            user: self.form_user.clone(),
            password: if self.form_password.is_empty() {
                None
            } else {
                Some(self.form_password.clone())
            },
            use_unix_socket: self.form_use_unix_socket,
            tls_enabled: self.selected_driver_kind != DbDriverKind::Sqlite && self.form_tls_enabled,
            tls_ca_cert: if self.form_tls_ca_cert.trim().is_empty() {
                None
            } else {
                Some(self.form_tls_ca_cert.trim().to_string())
            },
            ssh_tunnel: None,
            connect_timeout: std::time::Duration::from_millis(10000),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ConnectionManager, ConnectionSession};
    use db_wire::DbDriverKind;
    #[test]
    fn session_switch_retains_cache_and_metadata_identity() {
        let mut manager = ConnectionManager::new(vec![], Some("a".into()));
        manager
            .sessions
            .insert("a".into(), ConnectionSession::default());
        manager.sessions.insert(
            "b".into(),
            ConnectionSession {
                kind: DbDriverKind::MySql,
                ..ConnectionSession::default()
            },
        );
        let cache = manager.active().schema_cache.clone();
        cache.add_table("public", "same", "TABLE").unwrap();
        manager.active_connection_id = Some("b".into());
        assert_eq!(manager.active().kind, DbDriverKind::MySql);
        assert!(!std::sync::Arc::ptr_eq(
            &cache,
            &manager.active().schema_cache
        ));
        manager.active_connection_id = Some("a".into());
        assert!(std::sync::Arc::ptr_eq(
            &cache,
            &manager.active().schema_cache
        ));
        assert!(manager.session("disconnected").driver.is_none());
    }
}

impl ConnectionFormState {
    pub fn from_saved(saved: Option<&SavedConnection>) -> Self {
        Self {
            request_id: 0,
            editing_connection_id: saved.map(|s| s.id.clone()),
            is_connecting: false,
            show_connection_modal: false,
            selected_driver_kind: saved
                .and_then(|s| DbDriverKind::from_name(&s.driver))
                .unwrap_or(DbDriverKind::Postgres),
            form_name: saved.map(|s| s.name.clone()).unwrap_or_default(),
            form_host: saved
                .map(|s| s.host.clone())
                .unwrap_or_else(|| "localhost".into()),
            form_port: saved
                .map(|s| s.port.to_string())
                .unwrap_or_else(|| "5432".into()),
            form_database: saved.map(|s| s.database.clone()).unwrap_or_default(),
            form_user: saved
                .map(|s| s.user.clone())
                .unwrap_or_else(|| "postgres".into()),
            form_password: saved.and_then(|s| s.password.clone()).unwrap_or_default(),
            form_use_unix_socket: saved.is_some_and(|s| s.use_unix_socket),
            form_tls_enabled: saved.is_some_and(|s| s.tls_enabled),
            form_tls_ca_cert: saved
                .and_then(|s| s.tls_ca_cert.clone())
                .unwrap_or_default(),
            active_field: Some(FormField::Name),
            form_history: HashMap::new(),
            show_password: false,
            test_result: None,
            field_anchor: None,
            field_cursor: usize::MAX,
        }
    }
}
#[cfg(test)]
mod form_tests {
    use super::{ConnectionFormState, ConnectionSession};
    use db_wire::DbDriverKind;
    #[test]
    fn form_changes_do_not_change_session_and_stale_responses_are_rejected() {
        let session = ConnectionSession::default();
        let mut form = ConnectionFormState::from_saved(None);
        let request = form.begin_request();
        let snapshot = form.snapshot();
        assert!(form.accepts(request, &snapshot));
        form.selected_driver_kind = DbDriverKind::MySql;
        assert!(!form.accepts(request, &snapshot));
        assert_eq!(session.kind, DbDriverKind::Postgres);
        form.selected_driver_kind = DbDriverKind::Postgres;
        form.invalidate();
        assert!(!form.accepts(request, &snapshot));
        form.form_port = "99999".into();
        assert!(form.build_config().is_err());
    }
}
