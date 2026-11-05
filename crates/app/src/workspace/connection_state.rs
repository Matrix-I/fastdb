use super::{
    FormField, SavedConnection,
    connection_config::{
        ConnectionAppearance, DEFAULT_CONNECT_TIMEOUT_SECS, MAX_CONNECT_TIMEOUT_SECS, SavedSsh,
    },
};
use db_wire::{ConnectionConfig, DatabaseDriver, DbDriverKind, TableSummary};
use sql_intel::SchemaCache;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use ui_editor::line_input::LineHistory;

pub(super) struct ConnectionFormState {
    pub request_id: u64,
    pub editing_connection_id: Option<String>,
    pub is_connecting: bool,
    pub show_connection_modal: bool,
    pub selected_driver_kind: DbDriverKind,
    pub form_name: String,
    pub connection_group: String,
    pub connection_color: String,
    pub form_host: String,
    pub form_port: String,
    pub form_database: String,
    pub form_user: String,
    pub form_password: String,
    pub form_use_unix_socket: bool,
    pub form_tls_enabled: bool,
    pub form_tls_ca_cert: String,
    pub ssh: SshDraft,
    pub connect_timeout: String,
    pub read_only: bool,
    pub application_name: String,
    pub client_archive: String,
    pub client_archive_password: String,
    client_archive_password_unavailable: bool,
    pub active_field: Option<FormField>,
    pub form_history: HashMap<FormField, LineHistory>,
    pub show_password: bool,
    pub test_result: Option<Result<String, String>>,
    pub field_anchor: Option<usize>,
    pub field_cursor: usize,
}

pub(super) struct SshDraft {
    pub enabled: bool,
    pub host: String,
    pub port: String,
    pub user: String,
    pub fingerprint: String,
    pub key_path: String,
    pub secret: String,
    secret_unavailable: bool,
}

impl SshDraft {
    fn from_saved(saved: Option<&SavedSsh>) -> Self {
        Self {
            enabled: saved.is_some(),
            host: saved.map(|ssh| ssh.host.clone()).unwrap_or_default(),
            port: saved
                .map(|ssh| ssh.port.to_string())
                .unwrap_or_else(|| "22".into()),
            user: saved.map(|ssh| ssh.user.clone()).unwrap_or_default(),
            fingerprint: saved.map(|ssh| ssh.fingerprint.clone()).unwrap_or_default(),
            key_path: saved
                .and_then(|ssh| ssh.key_path.clone())
                .unwrap_or_default(),
            secret: saved.and_then(|ssh| ssh.secret.clone()).unwrap_or_default(),
            secret_unavailable: saved.is_some_and(|ssh| ssh.secret_unavailable),
        }
    }

    fn build(&self, driver: DbDriverKind) -> Result<Option<db_wire::SshTunnelConfig>, String> {
        if !self.enabled {
            return Ok(None);
        }
        if !driver.supports_ssh() {
            return Err("SSH tunnels are unavailable for this provider".into());
        }
        let port = self
            .port
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or("SSH port must be between 1 and 65535")?;
        let key_path = self.key_path.trim();
        if self.secret_unavailable && self.secret.is_empty() {
            return Err("Unlock the keychain or re-enter the SSH password or passphrase".into());
        }
        let settings = db_wire::SshTunnelConfig {
            host: self.host.trim().into(),
            port,
            user: self.user.trim().into(),
            host_key_fingerprint: self.fingerprint.trim().into(),
            key_path: (!key_path.is_empty()).then(|| key_path.into()),
            password: (!self.secret.is_empty()).then(|| self.secret.clone()),
        };
        settings.validate().map_err(|error| error.to_string())?;
        Ok(Some(settings))
    }
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
    pub selected_relation: Option<(String, String)>,
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
                s.insert(super::navigator::table_folder_key("public"));
                s
            },
            nav_table_expanded: HashSet::new(),
            nav_subfolder_expanded: HashSet::new(),
            selected_relation: None,
        }
    }
}

pub(super) struct ConnectionSession {
    pub metadata_generation: u64,
    pub diagram: Arc<super::diagram_model::DiagramModel>,
    pub kind: DbDriverKind,
    pub schema_tree: Arc<std::collections::BTreeMap<String, Vec<TableSummary>>>,
    pub properties: HashMap<(String, String), Arc<super::properties::model::TableProperties>>,
    pub driver: Option<Arc<dyn DatabaseDriver>>,
    pub schema_cache: Arc<SchemaCache>,
    pub tables: Vec<TableSummary>,
    pub table_columns: Arc<super::object_search::ColumnMap>,
    pub table_constraints: HashMap<(String, String), Vec<(String, String)>>,
    pub table_indexes: HashMap<(String, String), Vec<String>>,
}
impl Default for ConnectionSession {
    fn default() -> Self {
        Self {
            diagram: Arc::default(),
            metadata_generation: 0,
            kind: DbDriverKind::Postgres,
            schema_tree: Arc::new(std::collections::BTreeMap::new()),
            properties: HashMap::new(),
            driver: None,
            schema_cache: Arc::new(SchemaCache::new().expect("schema cache")),
            tables: Vec::new(),
            table_columns: Arc::default(),
            table_constraints: HashMap::new(),
            table_indexes: HashMap::new(),
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
            self.connection_group.clone(),
            self.connection_color.clone(),
            self.form_host.clone(),
            self.form_port.clone(),
            self.form_database.clone(),
            self.form_user.clone(),
            self.form_password.clone(),
            self.form_use_unix_socket.to_string(),
            self.form_tls_enabled.to_string(),
            self.form_tls_ca_cert.clone(),
            self.show_connection_modal.to_string(),
            self.connect_timeout.clone(),
            self.read_only.to_string(),
            self.application_name.clone(),
            self.client_archive.clone(),
            self.client_archive_password.clone(),
            self.ssh.enabled.to_string(),
            self.ssh.host.clone(),
            self.ssh.port.clone(),
            self.ssh.user.clone(),
            self.ssh.fingerprint.clone(),
            self.ssh.key_path.clone(),
            self.ssh.secret.clone(),
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
    pub fn session_mut(&mut self, id: &str) -> &mut ConnectionSession {
        self.sessions.entry(id.to_string()).or_default()
    }
    pub fn session(&self, id: &str) -> &ConnectionSession {
        self.sessions.get(id).unwrap_or(&self.empty)
    }
}

impl ConnectionFormState {
    pub(super) fn shows_client_identity(&self) -> bool {
        !self.client_archive.is_empty()
            || !self.client_archive_password.is_empty()
            || self.client_archive_password_unavailable
            || (self.form_tls_enabled && self.selected_driver_kind.supports_tls_client_identity())
    }

    fn build_client_identity(&self) -> Result<Option<db_wire::TlsClientIdentity>, String> {
        let archive = self.client_archive.trim();
        if archive.is_empty() && self.client_archive_password.is_empty() {
            return Ok(None);
        }
        if !self.form_tls_enabled || !self.selected_driver_kind.supports_tls_client_identity() {
            return Err("Enable TLS on PostgreSQL, MySQL, or MariaDB, or clear the TLS client identity fields".into());
        }
        if archive.is_empty() {
            return Err("Enter the PKCS#12 client identity path".into());
        }
        if self.client_archive_password_unavailable && self.client_archive_password.is_empty() {
            return Err("Unlock the keychain or re-enter the TLS client identity password".into());
        }
        Ok(Some(db_wire::TlsClientIdentity {
            archive_path: archive.into(),
            password: (!self.client_archive_password.is_empty())
                .then(|| self.client_archive_password.clone()),
        }))
    }

    pub(super) fn build_appearance(&self) -> Result<ConnectionAppearance, String> {
        ConnectionAppearance::parse(&self.connection_group, &self.connection_color)
    }

    pub fn build_config(&self) -> Result<ConnectionConfig, String> {
        let port = if self.form_port.trim().is_empty() {
            self.selected_driver_kind.default_port()
        } else {
            self.form_port
                .parse::<u16>()
                .map_err(|_| "Port must be a number between 0 and 65535".to_string())?
        };

        let connect_timeout_secs = if self.selected_driver_kind == DbDriverKind::Sqlite
            || self.connect_timeout.trim().is_empty()
        {
            DEFAULT_CONNECT_TIMEOUT_SECS
        } else {
            self.connect_timeout.trim().parse::<u16>().ok()
                .filter(|seconds| (1..=MAX_CONNECT_TIMEOUT_SECS).contains(seconds))
                .ok_or_else(|| format!("Connection timeout must be between 1 and {MAX_CONNECT_TIMEOUT_SECS} seconds"))?
        };
        if self.read_only && !self.selected_driver_kind.supports_read_only() {
            return Err("Read-only connections are unavailable for this provider. Clear Read-only to continue.".into());
        }
        let config = ConnectionConfig {
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
            tls_client_identity: self.build_client_identity()?,
            ssh_tunnel: self.ssh.build(self.selected_driver_kind)?,
            connect_timeout: std::time::Duration::from_secs(connect_timeout_secs.into()),
            read_only: self.read_only,
            application_name: (!self.application_name.trim().is_empty())
                .then(|| self.application_name.trim().to_owned()),
        };
        config
            .validate_application_name()
            .map_err(|error| error.to_string())?;
        Ok(config)
    }
}

impl ConnectionFormState {
    pub fn load(&mut self, saved: Option<&SavedConnection>, visible: bool) {
        let request_id = self.request_id + 1;
        *self = Self::from_saved(saved);
        self.request_id = request_id;
        self.show_connection_modal = visible;
        if saved.is_some() {
            self.active_field = Some(FormField::Database);
        }
    }
    pub fn select_preset(&mut self, kind: DbDriverKind) {
        self.invalidate();
        self.selected_driver_kind = kind;
        self.form_host = "localhost".into();
        self.form_port = kind.default_port().to_string();
        self.form_database = match kind {
            DbDriverKind::Postgres => "postgres",
            DbDriverKind::MySql | DbDriverKind::MariaDb => "mysql",
            DbDriverKind::SqlServer => "master",
            DbDriverKind::Oracle => "FREEPDB1",
            DbDriverKind::Sqlite => "fastdb.sqlite",
            DbDriverKind::Db2 => "",
        }
        .into();
        self.form_user = match kind {
            DbDriverKind::Postgres => "postgres",
            DbDriverKind::MySql | DbDriverKind::MariaDb => "root",
            DbDriverKind::SqlServer => "sa",
            DbDriverKind::Db2 => "db2inst1",
            _ => "",
        }
        .into();
        self.form_password.clear();
        self.form_use_unix_socket = matches!(
            kind,
            DbDriverKind::Postgres | DbDriverKind::MySql | DbDriverKind::MariaDb
        );
        self.form_tls_enabled = kind == DbDriverKind::SqlServer;
        self.form_tls_ca_cert.clear();
        self.form_history.clear();
        self.test_result = None;
        self.show_password = false;
        self.active_field = Some(if kind == DbDriverKind::Sqlite {
            FormField::Database
        } else {
            FormField::Host
        });
        self.field_cursor = usize::MAX;
        self.field_anchor = None;
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
            connection_group: saved
                .and_then(|s| s.appearance.group.clone())
                .unwrap_or_default(),
            connection_color: saved.map(|s| s.appearance.color_text()).unwrap_or_default(),
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
            ssh: SshDraft::from_saved(saved.and_then(|s| s.ssh.as_ref())),
            connect_timeout: saved
                .map_or(DEFAULT_CONNECT_TIMEOUT_SECS, |s| s.connect_timeout_secs)
                .to_string(),
            client_archive: saved
                .and_then(|s| s.client_identity.as_ref())
                .map(|identity| identity.archive_path.clone())
                .unwrap_or_default(),
            client_archive_password: saved
                .and_then(|s| s.client_identity.as_ref())
                .and_then(|identity| identity.password.clone())
                .unwrap_or_default(),
            client_archive_password_unavailable: saved
                .and_then(|s| s.client_identity.as_ref())
                .is_some_and(|identity| identity.password_unavailable),
            read_only: saved.is_some_and(|profile| profile.read_only),
            application_name: saved
                .and_then(|profile| profile.application_name.clone())
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
    fn postgres_application_name_roundtrips_and_validates() {
        use crate::workspace::connection_config::ConnectionRepository;
        let (profiles, _) = ConnectionRepository::load_saved_connections_from_str(
            r#"{"connections":[{"database":"db","application_name":"FastDB reporting"}]}"#,
        );
        let json = ConnectionRepository::serialize_connections_config(&profiles, None);
        let (profiles, _) = ConnectionRepository::load_saved_connections_from_str(&json);
        let mut form = ConnectionFormState::from_saved(profiles.first());
        assert_eq!(
            form.build_config().unwrap().application_name.as_deref(),
            Some("FastDB reporting")
        );
        form.application_name = "x".repeat(64);
        assert!(form.build_config().is_err());
        form.application_name = "reporting".into();
        form.select_preset(DbDriverKind::MySql);
        assert!(form.build_config().is_err());
    }

    #[test]
    fn read_only_profile_survives_reload_and_preset_selection() {
        use crate::workspace::connection_config::ConnectionRepository;
        let (profiles, _) = ConnectionRepository::load_saved_connections_from_str(
            r#"{"connections":[{"database":"db","read_only":true},{"database":"legacy"}]}"#,
        );
        let json = ConnectionRepository::serialize_connections_config(&profiles, None);
        let (loaded, _) = ConnectionRepository::load_saved_connections_from_str(&json);
        assert!(!loaded[1].read_only);
        let mut draft = ConnectionFormState::from_saved(loaded.first());
        assert!(draft.build_config().unwrap().read_only);
        let request = draft.begin_request();
        let snapshot = draft.snapshot();
        draft.read_only = false;
        assert!(!draft.accepts(request, &snapshot));
        draft.read_only = true;
        draft.select_preset(DbDriverKind::Oracle);
        assert!(draft.build_config().is_err());
        draft.select_preset(DbDriverKind::Sqlite);
        assert!(draft.build_config().unwrap().read_only);
    }

    #[test]
    fn client_identity_is_part_of_the_draft_and_requires_tls() {
        let mut form = ConnectionFormState::from_saved(None);
        form.client_archive = "/keys/client.p12".into();
        assert!(form.build_config().is_err());
        form.form_tls_enabled = true;
        let request = form.begin_request();
        let snapshot = form.snapshot();
        form.client_archive_password = "archive-fixture-secret".into();
        assert!(!form.accepts(request, &snapshot));
        let identity = form.build_config().unwrap().tls_client_identity.unwrap();
        assert_eq!(identity.archive_path, "/keys/client.p12");
        assert_eq!(identity.password.as_deref(), Some("archive-fixture-secret"));
        form.select_preset(DbDriverKind::Db2);
        assert!(form.build_config().is_err());
    }

    #[test]
    fn saved_timeout_reaches_connection_config_and_invalidates_old_requests() {
        use crate::workspace::connection_config::ConnectionRepository;
        let (profiles, _) = ConnectionRepository::load_saved_connections_from_str(
            r#"{"connections":[{"id":"a","database":"db","connect_timeout_secs":23}]}"#,
        );
        let json = ConnectionRepository::serialize_connections_config(&profiles, Some("a"));
        let (profiles, _) = ConnectionRepository::load_saved_connections_from_str(&json);
        let mut form = ConnectionFormState::from_saved(profiles.first());
        assert_eq!(form.build_config().unwrap().connect_timeout.as_secs(), 23);
        let request = form.begin_request();
        let snapshot = form.snapshot();
        form.connect_timeout = "0".into();
        assert!(form.build_config().is_err());
        assert!(!form.accepts(request, &snapshot));
        form.connect_timeout.clear();
        assert_eq!(form.build_config().unwrap().connect_timeout.as_secs(), 10);
    }

    #[test]
    fn ssh_draft_validates_transport_and_rejects_stale_test_results() {
        let mut form = ConnectionFormState::from_saved(None);
        assert!(form.build_config().unwrap().ssh_tunnel.is_none());
        form.ssh.enabled = true;
        assert!(form.build_config().is_err());
        form.ssh.host = "jump.example".into();
        form.ssh.user = "test".into();
        form.ssh.secret = "fixture".into();
        form.ssh.fingerprint = format!("SHA256:{}", "A".repeat(43));
        let transport = form.build_config().unwrap().ssh_tunnel.unwrap();
        assert_eq!(transport.host, "jump.example");
        assert_eq!(transport.port, 22);
        let request = form.begin_request();
        let snapshot = form.snapshot();
        form.ssh.port = "2222".into();
        assert!(!form.accepts(request, &snapshot));
        assert_eq!(form.build_config().unwrap().ssh_tunnel.unwrap().port, 2222);
        form.selected_driver_kind = DbDriverKind::Oracle;
        assert!(form.build_config().is_err());
    }

    #[test]
    fn reselecting_provider_keeps_saved_tunnel_enforced() {
        use crate::workspace::connection_config::ConnectionRepository;
        let (profiles, _) = ConnectionRepository::load_saved_connections_from_str(&format!(
            r#"{{"connections":[{{"database":"db","ssh":{{"host":"jump.example","port":22,"user":"operator","fingerprint":"SHA256:{}","key_path":"/keys/operator"}}}}]}}"#,
            "A".repeat(43)
        ));
        let mut form = ConnectionFormState::from_saved(profiles.first());
        for driver in [DbDriverKind::Postgres, DbDriverKind::MySql] {
            form.select_preset(driver);
            let tunnel = form.build_config().unwrap().ssh_tunnel.unwrap();
            assert_eq!(tunnel.host, "jump.example");
        }
        form.select_preset(DbDriverKind::Oracle);
        assert!(form.build_config().is_err());
        form.ssh.enabled = false;
        assert!(form.build_config().unwrap().ssh_tunnel.is_none());
    }

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

#[cfg(test)]
mod preset_tests {
    use super::ConnectionFormState;
    use db_wire::DbDriverKind;
    #[test]
    fn provider_presets_preserve_form_identity_and_invalidate_pending_requests() {
        let mut form = ConnectionFormState::from_saved(None);
        form.editing_connection_id = Some("existing".into());
        form.show_connection_modal = true;
        let request = form.begin_request();
        let snapshot = form.snapshot();
        for kind in [
            DbDriverKind::Oracle,
            DbDriverKind::SqlServer,
            DbDriverKind::Db2,
            DbDriverKind::MariaDb,
        ] {
            form.select_preset(kind);
            assert_eq!(form.editing_connection_id.as_deref(), Some("existing"));
            assert!(form.show_connection_modal);
            assert_eq!(form.build_config().unwrap().driver, kind);
        }
        assert!(!form.accepts(request, &snapshot));
    }
}
