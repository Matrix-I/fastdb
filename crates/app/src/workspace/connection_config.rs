use db_wire::DbDriverKind;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedConnection {
    pub id: String,
    pub name: String,
    pub driver: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub user: String,
    pub password: Option<String>,
    /// Preserve a credential reference when its secret could not be loaded.
    pub password_unavailable: bool,
    pub use_unix_socket: bool,
    pub tls_enabled: bool,
    pub tls_ca_cert: Option<String>,
}

/// Where builds before the config dir kept the connections, relative to the working directory
const LEGACY_CONFIG_FILE: &str = ".fastdb_config.json";

pub(super) fn write_connections_config(
    path: &std::path::Path,
    content: &str,
) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written beside the file and renamed over it, so a crash mid-write never loses the connections
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)
}

pub(super) struct ConnectionRepository {
    pub path: std::path::PathBuf,
    pub legacy_path: std::path::PathBuf,
    pub credentials: std::sync::Arc<dyn platform_hal::keychain::CredentialStore>,
}
impl ConnectionRepository {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            legacy_path: LEGACY_CONFIG_FILE.into(),
            credentials: std::sync::Arc::new(platform_hal::keychain::KeychainStore::default()),
        }
    }
    pub fn load_saved_connections_from_str(
        content: &str,
    ) -> (Vec<SavedConnection>, Option<String>) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(content) else {
            return (Vec::new(), None);
        };

        let mut connections = Vec::new();

        if let Some(arr) = v["connections"].as_array() {
            for (idx, item) in arr.iter().enumerate() {
                let id = item["id"]
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("conn_{}", idx + 1));
                let d = item["database"].as_str().unwrap_or("").to_string();
                let drv_str = item["driver"].as_str().unwrap_or("Postgres").to_string();
                let default_name = if drv_str == "Sqlite" {
                    std::path::Path::new(&d)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&d)
                        .to_string()
                } else if !d.is_empty() {
                    format!("{}-local", d)
                } else {
                    format!("{}-conn", drv_str)
                };
                let name = item["name"]
                    .as_str()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&default_name)
                    .to_string();
                let h = item["host"].as_str().unwrap_or("localhost").to_string();
                let p = if let Some(n) = item["port"].as_u64() {
                    n as u16
                } else if let Some(s) = item["port"].as_str() {
                    s.parse::<u16>().unwrap_or_else(|_| {
                        DbDriverKind::from_name(&drv_str)
                            .unwrap_or(DbDriverKind::Postgres)
                            .default_port()
                    })
                } else {
                    DbDriverKind::from_name(&drv_str)
                        .unwrap_or(DbDriverKind::Postgres)
                        .default_port()
                };
                let u = item["user"].as_str().unwrap_or("postgres").to_string();
                let pw = item["password"].as_str().map(|s| s.to_string());
                let sock = item["use_unix_socket"].as_bool().unwrap_or(false);

                if !d.is_empty() || drv_str == "Sqlite" {
                    connections.push(SavedConnection {
                        id,
                        name,
                        driver: drv_str,
                        host: h,
                        port: p,
                        database: d,
                        user: u,
                        password: pw,
                        password_unavailable: item["keychain_password"].as_bool().unwrap_or(false),
                        use_unix_socket: sock,
                        tls_enabled: item["tls_enabled"].as_bool().unwrap_or(false),
                        tls_ca_cert: item["tls_ca_cert"]
                            .as_str()
                            .filter(|s| !s.trim().is_empty())
                            .map(str::to_owned),
                    });
                }
            }
        } else {
            // Legacy single connection format migration
            let d = v["database"].as_str().unwrap_or("").to_string();
            let has = v["has_saved_connection"].as_bool().unwrap_or(!d.is_empty());
            if has && !d.is_empty() {
                let drv_str = v["driver"].as_str().unwrap_or("Postgres").to_string();
                let h = v["host"].as_str().unwrap_or("localhost").to_string();
                let p = if let Some(n) = v["port"].as_u64() {
                    n as u16
                } else if let Some(s) = v["port"].as_str() {
                    s.parse::<u16>().unwrap_or_else(|_| {
                        DbDriverKind::from_name(&drv_str)
                            .unwrap_or(DbDriverKind::Postgres)
                            .default_port()
                    })
                } else {
                    DbDriverKind::from_name(&drv_str)
                        .unwrap_or(DbDriverKind::Postgres)
                        .default_port()
                };
                let u = v["user"].as_str().unwrap_or("postgres").to_string();
                let pw = v["password"].as_str().map(|s| s.to_string());
                let sock = v["use_unix_socket"].as_bool().unwrap_or(false);
                let name = if drv_str == "Sqlite" {
                    std::path::Path::new(&d)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&d)
                        .to_string()
                } else {
                    format!("{}-local", d)
                };

                connections.push(SavedConnection {
                    id: "conn_1".to_string(),
                    name,
                    driver: drv_str,
                    host: h,
                    port: p,
                    database: d,
                    user: u,
                    password: pw,
                    password_unavailable: false,
                    use_unix_socket: sock,
                    tls_enabled: false,
                    tls_ca_cert: None,
                });
            }
        }

        let active_id = v["active_connection_id"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| connections.first().map(|c| c.id.clone()));

        (connections, active_id)
    }

    /// Reads the saved connections from the user's config dir. A config left by older builds
    /// in the working directory is picked up once and moved there.
    pub fn load_saved_connections(&self) -> (Vec<SavedConnection>, Option<String>, Option<String>) {
        let (content, legacy) = match std::fs::read_to_string(&self.path) {
            Ok(content) => (content, false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match std::fs::read_to_string(&self.legacy_path) {
                    Ok(content) => (content, true),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        return (Vec::new(), None, None);
                    }
                    Err(_) => {
                        return (
                            Vec::new(),
                            None,
                            Some("Cannot read legacy connections config".into()),
                        );
                    }
                }
            }
            Err(_) => {
                return (
                    Vec::new(),
                    None,
                    Some("Cannot read connections config".into()),
                );
            }
        };
        if serde_json::from_str::<serde_json::Value>(&content).is_err() {
            return (
                Vec::new(),
                None,
                Some("Invalid connections config; the file was left unchanged".into()),
            );
        }
        let (mut connections, active) = Self::load_saved_connections_from_str(&content);
        let migrate = legacy
            || connections.iter().any(|c| c.password.is_some())
            || serde_json::from_str::<serde_json::Value>(&content)
                .ok()
                .is_some_and(|v| v["credential_cleanup"].is_array() || v["legacy_cleanup"] == true);
        let mut warning = None;
        for connection in &mut connections {
            if connection.password_unavailable && connection.password.is_none() {
                match self.credentials.get(&connection.id) {
                    Ok(Some(password)) => {
                        connection.password = Some(password);
                        connection.password_unavailable = false;
                    }
                    _ => warning = Some("Some saved passwords could not be read from the keychain. Unlock the keychain and restart, or re-enter the password in Edit connection.".into()),
                }
            }
        }
        if migrate && self.save(&connections, active.as_deref()).is_err() {
            warning = Some("Could not finish migrating credentials or cleaning up old secrets. Unlock the keychain and retry saving. Plaintext is removed only after its password is stored in the keychain.".into());
        }
        (connections, active, warning)
    }

    /// Secrets are written before removing plaintext; clearing/deletion happens only
    /// after committing the config, so a failed file write retains existing secrets.
    pub fn save(
        &self,
        connections: &[SavedConnection],
        active: Option<&str>,
    ) -> Result<(), String> {
        let mut legacy_source = !self.path.exists() && self.legacy_path.is_file();
        let mut cleanup = Vec::<String>::new();
        let previous = match std::fs::read_to_string(&self.path) {
            Ok(content) => {
                let json = serde_json::from_str::<serde_json::Value>(&content)
                    .map_err(|_| "Cannot replace an invalid connections config".to_string())?;
                legacy_source |= json["legacy_cleanup"] == true;
                if let Some(pending) = json["credential_cleanup"].as_array() {
                    cleanup.extend(pending.iter().filter_map(|v| v.as_str().map(str::to_owned)));
                }
                Self::load_saved_connections_from_str(&content).0
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err("Cannot read connections config before saving".into()),
        };
        let mut ids = std::collections::HashSet::new();
        if connections
            .iter()
            .any(|c| c.id.is_empty() || !ids.insert(&c.id))
        {
            return Err("Connection IDs must be unique and nonempty".into());
        }
        let keep = |id: &str| {
            connections.iter().any(|c| {
                c.id == id
                    && (c.password_unavailable
                        || c.password.as_ref().is_some_and(|p| !p.is_empty()))
            })
        };
        cleanup.extend(
            previous
                .iter()
                .filter(|c| (c.password_unavailable || c.password.is_some()) && !keep(&c.id))
                .map(|c| c.id.clone()),
        );
        cleanup.retain(|id| !keep(id));
        cleanup.sort();
        cleanup.dedup();
        let mut changed = Vec::new();
        let rollback = |changed: &[(String, Option<String>)]| {
            for (id, old) in changed.iter().rev() {
                let _ = match old {
                    Some(password) => self.credentials.save(id, password),
                    None => self.credentials.delete(id),
                };
            }
        };
        for connection in connections {
            if let Some(password) = connection.password.as_deref().filter(|p| !p.is_empty()) {
                let old = match self.credentials.get(&connection.id) {
                    Ok(old) => old,
                    Err(_) => {
                        rollback(&changed);
                        return Err("Cannot access the keychain. Unlock it and retry saving the connection.".into());
                    }
                };
                if old.as_deref() == Some(password) {
                    continue;
                }
                if self.credentials.save(&connection.id, password).is_err() {
                    rollback(&changed);
                    return Err("Cannot save password in the keychain. Unlock it and retry saving the connection.".into());
                }
                changed.push((connection.id.clone(), old));
            }
        }
        let content = Self::serialize_connections_config(connections, active);
        let mut json: serde_json::Value =
            serde_json::from_str(&content).expect("serialized config");
        if legacy_source {
            json["legacy_cleanup"] = serde_json::json!(true);
        }
        if !cleanup.is_empty() {
            json["credential_cleanup"] = serde_json::json!(cleanup);
        }
        if write_connections_config(&self.path, &json.to_string()).is_err() {
            rollback(&changed);
            return Err("Cannot save connections config; credential updates were rolled back where possible".into());
        }
        for id in &cleanup {
            self.credentials.delete(id)
                .map_err(|_| "Connection config saved, but an unused password could not be deleted from the keychain. Retry saving after unlocking it.".to_string())?;
        }
        if legacy_source {
            match std::fs::remove_file(&self.legacy_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(
                    "Credentials migrated, but the legacy plaintext config could not be removed"
                        .into(),
                ),
            }
        }
        if !cleanup.is_empty() || legacy_source {
            write_connections_config(&self.path, &content).map_err(|_| {
                "Credentials saved, but cleanup bookkeeping could not be updated".to_string()
            })?;
        }
        Ok(())
    }

    pub fn serialize_connections_config(
        connections: &[SavedConnection],
        active_id: Option<&str>,
    ) -> String {
        let connections_json: Vec<serde_json::Value> = connections
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "name": c.name,
                    "driver": c.driver,
                    "host": c.host,
                    "port": c.port.to_string(),
                    "database": c.database,
                    "user": c.user,
                    "keychain_password": c.password_unavailable || c.password.as_ref().is_some_and(|p| !p.is_empty()),
                    "use_unix_socket": c.use_unix_socket,
                    "tls_enabled": c.tls_enabled,
                    "tls_ca_cert": c.tls_ca_cert,
                })
            })
            .collect();

        let save_data = serde_json::json!({
            "has_saved_connection": !connections.is_empty(),
            "active_connection_id": active_id,
            "connections": connections_json,
        });

        serde_json::to_string_pretty(&save_data).unwrap_or_else(|_| save_data.to_string())
    }
}

#[cfg(test)]
mod tls_tests {
    use super::ConnectionRepository;

    #[test]
    fn enterprise_provider_settings_survive_save_and_reload() {
        for (provider, port) in [("SqlServer", 1433), ("Oracle", 1521), ("Db2", 50000)] {
            let input = format!(
                r#"{{"connections":[{{"id":"test","driver":"{provider}","database":"app","tls_enabled":true}}]}}"#
            );
            let (connections, _) = ConnectionRepository::load_saved_connections_from_str(&input);
            assert_eq!(connections[0].port, port);
            let saved =
                ConnectionRepository::serialize_connections_config(&connections, Some("test"));
            let (loaded, _) = ConnectionRepository::load_saved_connections_from_str(&saved);
            assert_eq!(loaded, connections);
            assert!(loaded[0].tls_enabled);
        }
    }

    #[test]
    fn mariadb_provider_identity_survives_save_and_reload() {
        let input = r#"{"connections":[{"id":"maria","driver":"MariaDb","host":"localhost","port":3306,"database":"app"}]}"#;
        let (connections, _) = ConnectionRepository::load_saved_connections_from_str(input);
        let saved = ConnectionRepository::serialize_connections_config(&connections, Some("maria"));
        let (loaded, active) = ConnectionRepository::load_saved_connections_from_str(&saved);
        assert_eq!(loaded[0].driver, "MariaDb");
        assert_eq!(loaded[0].port, 3306);
        assert_eq!(active.as_deref(), Some("maria"));
    }

    #[test]
    fn saved_tls_settings_round_trip_and_legacy_defaults_to_disabled() {
        let legacy = r#"{"connections":[{"id":"pg","database":"postgres"}]}"#;
        let (mut connections, _) = ConnectionRepository::load_saved_connections_from_str(legacy);
        assert!(!connections[0].tls_enabled);
        assert_eq!(connections[0].tls_ca_cert, None);
        connections[0].tls_enabled = true;
        connections[0].tls_ca_cert = Some("/path with spaces/root.pem".into());
        let saved = ConnectionRepository::serialize_connections_config(&connections, Some("pg"));
        let (loaded, active) = ConnectionRepository::load_saved_connections_from_str(&saved);
        assert_eq!(loaded, connections);
        assert_eq!(active.as_deref(), Some("pg"));
    }
}

#[cfg(test)]
mod relocated_tests {
    use super::ConnectionRepository;
    #[test]
    fn test_load_legacy_single_connection() {
        use crate::workspace::connection_config::ConnectionRepository;

        let legacy_json = r#"{
            "has_saved_connection": true,
            "driver": "Postgres",
            "host": "ubiid.ubitec.local",
            "port": "8091",
            "database": "ubiid",
            "user": "ubiid",
            "password": "db@ubiid",
            "use_unix_socket": false
        }"#;

        let (connections, active_id) =
            ConnectionRepository::load_saved_connections_from_str(legacy_json);
        assert_eq!(connections.len(), 1);
        let first = &connections[0];
        assert_eq!(first.id, "conn_1");
        assert_eq!(first.name, "ubiid-local");
        assert_eq!(first.driver, "Postgres");
        assert_eq!(first.host, "ubiid.ubitec.local");
        assert_eq!(first.port, 8091);
        assert_eq!(first.database, "ubiid");
        assert_eq!(first.user, "ubiid");
        assert_eq!(first.password, Some("db@ubiid".to_string()));
        assert_eq!(active_id, Some("conn_1".to_string()));
    }

    #[test]
    fn test_load_and_serialize_multiple_connections() {
        use crate::workspace::SavedConnection;

        let conns = vec![
            SavedConnection {
                id: "conn_pg".to_string(),
                name: "Production PG".to_string(),
                driver: "Postgres".to_string(),
                host: "postgres.internal".to_string(),
                port: 5432,
                database: "prod_db".to_string(),
                user: "postgres".to_string(),
                password: Some("secret".to_string()),
                password_unavailable: false,
                use_unix_socket: false,
                tls_enabled: false,
                tls_ca_cert: None,
            },
            SavedConnection {
                id: "conn_mysql".to_string(),
                name: "Analytics MySQL".to_string(),
                driver: "MySql".to_string(),
                host: "mysql.internal".to_string(),
                port: 3306,
                database: "analytics".to_string(),
                user: "root".to_string(),
                password: None,
                password_unavailable: false,
                use_unix_socket: false,
                tls_enabled: false,
                tls_ca_cert: None,
            },
            SavedConnection {
                id: "conn_sqlite".to_string(),
                name: "Local Cache".to_string(),
                driver: "Sqlite".to_string(),
                host: "localhost".to_string(),
                port: 0,
                database: "/path/to/cache.db".to_string(),
                user: "".to_string(),
                password: None,
                password_unavailable: false,
                use_unix_socket: false,
                tls_enabled: false,
                tls_ca_cert: None,
            },
        ];

        let serialized =
            ConnectionRepository::serialize_connections_config(&conns, Some("conn_mysql"));
        let (deserialized, active_id) =
            ConnectionRepository::load_saved_connections_from_str(&serialized);

        assert_eq!(deserialized.len(), 3);
        assert_eq!(active_id, Some("conn_mysql".to_string()));
        assert_eq!(deserialized[0].name, "Production PG");
        assert_eq!(deserialized[1].name, "Analytics MySQL");
        assert_eq!(deserialized[2].name, "Local Cache");
        assert_eq!(deserialized[2].database, "/path/to/cache.db");
    }

    #[test]
    fn test_adding_new_connection_does_not_override_existing() {
        use crate::workspace::SavedConnection;

        let mut conns = vec![SavedConnection {
            id: "conn_existing".to_string(),
            name: "Existing DB".to_string(),
            driver: "Postgres".to_string(),
            host: "localhost".to_string(),
            port: 5432,
            database: "existing".to_string(),
            user: "postgres".to_string(),
            password: None,
            password_unavailable: false,
            use_unix_socket: false,
            tls_enabled: false,
            tls_ca_cert: None,
        }];

        let new_conn = SavedConnection {
            id: "conn_new".to_string(),
            name: "New DB".to_string(),
            driver: "MySql".to_string(),
            host: "localhost".to_string(),
            port: 3306,
            database: "new_db".to_string(),
            user: "root".to_string(),
            password: None,
            password_unavailable: false,
            use_unix_socket: false,
            tls_enabled: false,
            tls_ca_cert: None,
        };

        if let Some(pos) = conns.iter().position(|c| c.id == new_conn.id) {
            conns[pos] = new_conn;
        } else {
            conns.push(new_conn);
        }

        assert_eq!(conns.len(), 2);
        assert_eq!(conns[0].name, "Existing DB");
        assert_eq!(conns[1].name, "New DB");
    }

    #[test]
    fn test_editing_connection_updates_in_place() {
        use crate::workspace::SavedConnection;

        let mut conns = [
            SavedConnection {
                id: "conn_1".to_string(),
                name: "Database 1".to_string(),
                driver: "Postgres".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                database: "db1".to_string(),
                user: "postgres".to_string(),
                password: None,
                password_unavailable: false,
                use_unix_socket: false,
                tls_enabled: false,
                tls_ca_cert: None,
            },
            SavedConnection {
                id: "conn_2".to_string(),
                name: "Database 2".to_string(),
                driver: "Postgres".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                database: "db2".to_string(),
                user: "postgres".to_string(),
                password: None,
                password_unavailable: false,
                use_unix_socket: false,
                tls_enabled: false,
                tls_ca_cert: None,
            },
        ];

        let updated = SavedConnection {
            id: "conn_1".to_string(),
            name: "Database 1 (Renamed)".to_string(),
            driver: "Postgres".to_string(),
            host: "db.remote.com".to_string(),
            port: 5433,
            database: "db1_prod".to_string(),
            user: "admin".to_string(),
            password: Some("new_pass".to_string()),
            password_unavailable: false,
            use_unix_socket: false,
            tls_enabled: false,
            tls_ca_cert: None,
        };

        if let Some(pos) = conns.iter().position(|c| c.id == updated.id) {
            conns[pos] = updated;
        }

        assert_eq!(conns.len(), 2);
        assert_eq!(conns[0].name, "Database 1 (Renamed)");
        assert_eq!(conns[0].host, "db.remote.com");
        assert_eq!(conns[0].port, 5433);
        assert_eq!(conns[1].name, "Database 2");
        assert_eq!(conns[1].host, "localhost");
    }

    #[test]
    fn test_deleting_connection_removes_from_list() {
        use crate::workspace::SavedConnection;

        let mut conns = vec![
            SavedConnection {
                id: "conn_1".to_string(),
                name: "Database 1".to_string(),
                driver: "Postgres".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                database: "db1".to_string(),
                user: "postgres".to_string(),
                password: None,
                password_unavailable: false,
                use_unix_socket: false,
                tls_enabled: false,
                tls_ca_cert: None,
            },
            SavedConnection {
                id: "conn_2".to_string(),
                name: "Database 2".to_string(),
                driver: "Postgres".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                database: "db2".to_string(),
                user: "postgres".to_string(),
                password: None,
                password_unavailable: false,
                use_unix_socket: false,
                tls_enabled: false,
                tls_ca_cert: None,
            },
        ];

        conns.retain(|c| c.id != "conn_1");

        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].id, "conn_2");
        assert_eq!(conns[0].name, "Database 2");
    }

    fn saved_pg_connection(id: &str, password: Option<&str>) -> crate::workspace::SavedConnection {
        crate::workspace::SavedConnection {
            id: id.to_string(),
            name: "Production PG".to_string(),
            driver: "Postgres".to_string(),
            host: "postgres.internal".to_string(),
            port: 5432,
            database: "prod_db".to_string(),
            user: "postgres".to_string(),
            password: password.map(|p| p.to_string()),
            password_unavailable: false,
            use_unix_socket: false,
            tls_enabled: false,
            tls_ca_cert: None,
        }
    }

    #[test]
    fn test_password_is_referenced_but_never_serialized() {
        use crate::workspace::connection_config::ConnectionRepository;

        let conns = vec![saved_pg_connection("conn_pg", Some("s3cret-pw"))];
        let serialized =
            ConnectionRepository::serialize_connections_config(&conns, Some("conn_pg"));

        let (loaded, _) = ConnectionRepository::load_saved_connections_from_str(&serialized);
        assert_eq!(loaded[0].password, None);
        assert!(loaded[0].password_unavailable);
        assert!(!serialized.contains("s3cret-pw"));
        assert!(!serialized.contains("\"password\""));
    }

    #[test]
    fn test_empty_password_is_not_written_to_config() {
        use crate::workspace::connection_config::ConnectionRepository;

        let conns = vec![
            saved_pg_connection("conn_a", None),
            saved_pg_connection("conn_b", Some("")),
        ];
        let serialized = ConnectionRepository::serialize_connections_config(&conns, None);

        let (loaded, _) = ConnectionRepository::load_saved_connections_from_str(&serialized);
        assert_eq!(loaded[0].password, None);
        assert_eq!(loaded[1].password, None);
    }
}

#[cfg(test)]
mod repository_tests {
    use super::{ConnectionRepository, write_connections_config};
    #[test]
    fn repository_uses_injected_paths_and_migrates_legacy_data() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config/connections.json");
        let legacy = temp.path().join("old.json");
        std::fs::write(&legacy, r#"{"connections":[{"id":"a","database":"db"}]}"#).unwrap();
        let repository = ConnectionRepository {
            path: path.clone(),
            legacy_path: legacy,
            credentials: std::sync::Arc::new(
                platform_hal::keychain::InMemoryCredentialStore::default(),
            ),
        };
        let (saved, active, error) = repository.load_saved_connections();
        assert!(error.is_none());
        assert_eq!(active.as_deref(), Some("a"));
        assert!(path.exists());
        write_connections_config(
            &path,
            &ConnectionRepository::serialize_connections_config(&saved, active.as_deref()),
        )
        .unwrap();
        assert_eq!(repository.load_saved_connections(), (saved, active, None));
    }
}

#[cfg(test)]
mod credential_tests {
    use super::*;
    use platform_hal::keychain::{CredentialStore, InMemoryCredentialStore};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    #[derive(Default)]
    struct Store {
        inner: InMemoryCredentialStore,
        fail_save: AtomicBool,
        fail_get: AtomicBool,
        fail_delete: AtomicBool,
    }
    impl CredentialStore for Store {
        fn get(&self, id: &str) -> anyhow::Result<Option<String>> {
            anyhow::ensure!(!self.fail_get.load(Ordering::SeqCst), "locked");
            self.inner.get(id)
        }
        fn save(&self, id: &str, password: &str) -> anyhow::Result<()> {
            anyhow::ensure!(!self.fail_save.load(Ordering::SeqCst), "locked");
            self.inner.save(id, password)
        }
        fn delete(&self, id: &str) -> anyhow::Result<()> {
            anyhow::ensure!(!self.fail_delete.load(Ordering::SeqCst), "locked");
            self.inner.delete(id)
        }
    }
    fn setup() -> (tempfile::TempDir, ConnectionRepository, Arc<Store>) {
        let temp = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::default());
        let repo = ConnectionRepository {
            path: temp.path().join("connections.json"),
            legacy_path: temp.path().join("legacy.json"),
            credentials: store.clone(),
        };
        (temp, repo, store)
    }
    const PLAINTEXT: &str = r#"{"active_connection_id":"a","connections":[{"id":"a","database":"db","password":"test-secret"}]}"#;

    #[test]
    fn migration_restart_update_clear_and_delete() {
        let (_temp, repo, store) = setup();
        std::fs::write(&repo.path, PLAINTEXT).unwrap();
        let (mut connections, active, warning) = repo.load_saved_connections();
        assert!(warning.is_none());
        assert_eq!(active.as_deref(), Some("a"));
        assert_eq!(store.get("a").unwrap().as_deref(), Some("test-secret"));
        assert!(
            !std::fs::read_to_string(&repo.path)
                .unwrap()
                .contains("test-secret")
        );
        assert_eq!(repo.load_saved_connections().0, connections);
        connections[0].password = Some("updated".into());
        repo.save(&connections, active.as_deref()).unwrap();
        assert_eq!(
            repo.load_saved_connections().0[0].password.as_deref(),
            Some("updated")
        );
        connections[0].password = None;
        repo.save(&connections, active.as_deref()).unwrap();
        assert_eq!(store.get("a").unwrap(), None);
        connections[0].password = Some("again".into());
        repo.save(&connections, active.as_deref()).unwrap();
        repo.save(&[], None).unwrap();
        assert_eq!(store.get("a").unwrap(), None);
    }

    #[test]
    fn failed_migration_preserves_plaintext_until_successful_retry() {
        for legacy in [false, true] {
            let (_temp, repo, store) = setup();
            let source = if legacy {
                &repo.legacy_path
            } else {
                &repo.path
            };
            std::fs::write(source, PLAINTEXT).unwrap();
            store.fail_save.store(true, Ordering::SeqCst);
            let (connections, active, warning) = repo.load_saved_connections();
            assert!(warning.is_some());
            assert_eq!(std::fs::read_to_string(source).unwrap(), PLAINTEXT);
            store.fail_save.store(false, Ordering::SeqCst);
            repo.save(&connections, active.as_deref()).unwrap();
            assert!(!repo.legacy_path.exists());
            assert!(
                !std::fs::read_to_string(&repo.path)
                    .unwrap()
                    .contains("test-secret")
            );
        }
    }

    #[test]
    fn legacy_single_connection_migrates_and_removes_source() {
        let (_temp, repo, store) = setup();
        std::fs::write(
            &repo.legacy_path,
            r#"{"database":"db","password":"old-secret"}"#,
        )
        .unwrap();
        let (connections, _, warning) = repo.load_saved_connections();
        assert!(warning.is_none());
        assert!(!repo.legacy_path.exists());
        assert_eq!(store.get("conn_1").unwrap().as_deref(), Some("old-secret"));
        assert_eq!(repo.load_saved_connections().0, connections);
    }

    #[test]
    fn unavailable_password_reference_survives_unrelated_save() {
        let (_temp, repo, store) = setup();
        std::fs::write(&repo.path, PLAINTEXT).unwrap();
        repo.load_saved_connections();
        store.fail_get.store(true, Ordering::SeqCst);
        let (mut connections, active, warning) = repo.load_saved_connections();
        assert!(warning.is_some());
        assert!(connections[0].password_unavailable);
        connections[0].name = "renamed".into();
        repo.save(&connections, active.as_deref()).unwrap();
        store.fail_get.store(false, Ordering::SeqCst);
        assert_eq!(
            repo.load_saved_connections().0[0].password.as_deref(),
            Some("test-secret")
        );
        store.delete("a").unwrap();
        assert!(repo.load_saved_connections().2.is_some());
    }

    #[test]
    fn failed_file_write_rolls_back_credential_update() {
        let (_temp, repo, store) = setup();
        std::fs::write(&repo.path, PLAINTEXT).unwrap();
        let (mut connections, active, _) = repo.load_saved_connections();
        let before = std::fs::read_to_string(&repo.path).unwrap();
        std::fs::create_dir(repo.path.with_extension("json.tmp")).unwrap();
        connections[0].password = Some("replacement".into());
        assert!(repo.save(&connections, active.as_deref()).is_err());
        assert_eq!(store.get("a").unwrap().as_deref(), Some("test-secret"));
        assert_eq!(std::fs::read_to_string(&repo.path).unwrap(), before);
    }

    #[test]
    fn failed_deletion_is_retried_after_restart() {
        let (_temp, repo, store) = setup();
        std::fs::write(&repo.path, PLAINTEXT).unwrap();
        repo.load_saved_connections();
        store.fail_delete.store(true, Ordering::SeqCst);
        assert!(repo.save(&[], None).is_err());
        assert!(
            std::fs::read_to_string(&repo.path)
                .unwrap()
                .contains("credential_cleanup")
        );
        store.fail_delete.store(false, Ordering::SeqCst);
        assert!(repo.load_saved_connections().2.is_none());
        assert_eq!(store.get("a").unwrap(), None);
    }
}
