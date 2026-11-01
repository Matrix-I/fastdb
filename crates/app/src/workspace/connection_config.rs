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
}
impl ConnectionRepository {
    pub fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            legacy_path: ".fastdb_config.json".into(),
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
    pub fn load_saved_connections(&self) -> (Vec<SavedConnection>, Option<String>) {
        let path = &self.path;
        if let Ok(content) = std::fs::read_to_string(path) {
            return Self::load_saved_connections_from_str(&content);
        }
        let Ok(content) = std::fs::read_to_string(&self.legacy_path) else {
            return (Vec::new(), None);
        };
        let loaded = Self::load_saved_connections_from_str(&content);
        if let Err(e) = write_connections_config(path, &content) {
            tracing::warn!(
                "Failed to move {} to {}: {}",
                LEGACY_CONFIG_FILE,
                path.display(),
                e
            );
        }
        loaded
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
                    "password": c.password.as_deref().filter(|p| !p.is_empty()),
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
            use_unix_socket: false,
            tls_enabled: false,
            tls_ca_cert: None,
        }
    }

    #[test]
    fn test_password_roundtrips_through_config() {
        use crate::workspace::connection_config::ConnectionRepository;

        let conns = vec![saved_pg_connection("conn_pg", Some("s3cret-pw"))];
        let serialized =
            ConnectionRepository::serialize_connections_config(&conns, Some("conn_pg"));

        let (loaded, _) = ConnectionRepository::load_saved_connections_from_str(&serialized);
        assert_eq!(loaded[0].password.as_deref(), Some("s3cret-pw"));
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
        };
        let (saved, active) = repository.load_saved_connections();
        assert_eq!(active.as_deref(), Some("a"));
        assert!(path.exists());
        write_connections_config(
            &path,
            &ConnectionRepository::serialize_connections_config(&saved, active.as_deref()),
        )
        .unwrap();
        assert_eq!(repository.load_saved_connections(), (saved, active));
    }
}
