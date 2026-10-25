use super::*;

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
}

impl Workspace {
    pub fn load_saved_connections_from_str(content: &str) -> (Vec<SavedConnection>, Option<String>) {
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
                    s.parse::<u16>().unwrap_or(5432)
                } else {
                    5432
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
                    s.parse::<u16>().unwrap_or(5432)
                } else {
                    5432
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
                });
            }
        }

        let active_id = v["active_connection_id"]
            .as_str()
            .map(|s| s.to_string())
            .or_else(|| connections.first().map(|c| c.id.clone()));

        (connections, active_id)
    }

    pub fn load_saved_connections() -> (Vec<SavedConnection>, Option<String>) {
        if let Ok(content) = std::fs::read_to_string(".fastdb_config.json") {
            Self::load_saved_connections_from_str(&content)
        } else {
            (Vec::new(), None)
        }
    }

    pub fn serialize_connections_config(connections: &[SavedConnection], active_id: Option<&str>) -> String {
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
                    "use_unix_socket": c.use_unix_socket,
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

    pub fn save_config(&self) {
        let content = Self::serialize_connections_config(&self.saved_connections, self.active_connection_id.as_deref());
        let _ = std::fs::write(".fastdb_config.json", content);
    }

    /// Stores a connection's password in the credential store; an empty password removes it
    pub fn store_connection_secret(store: &dyn CredentialStore, conn_id: &str, password: Option<&str>) -> anyhow::Result<()> {
        match password.filter(|p| !p.is_empty()) {
            Some(pw) => store.save(conn_id, pw),
            None => store.delete(conn_id),
        }
    }

    /// Returns the in-memory password, or fetches it from the credential store on first use
    pub fn resolve_connection_password(store: &dyn CredentialStore, saved: &SavedConnection) -> Option<String> {
        if saved.password.is_some() {
            return saved.password.clone();
        }
        match store.get(&saved.id) {
            Ok(pw) => pw,
            Err(e) => {
                tracing::warn!("Failed to read password for connection {}: {}", saved.id, e);
                None
            }
        }
    }

    /// Moves passwords loaded from a legacy plaintext config into the credential store.
    /// Returns true when the config file should be rewritten without them.
    pub fn migrate_plaintext_passwords(store: &dyn CredentialStore, connections: &mut [SavedConnection]) -> bool {
        let mut migrated = false;
        for conn in connections.iter_mut() {
            let Some(pw) = conn.password.clone() else { continue };
            if pw.is_empty() {
                conn.password = None;
                migrated = true;
                continue;
            }
            match store.save(&conn.id, &pw) {
                Ok(()) => migrated = true,
                Err(e) => tracing::warn!("Failed to migrate password for connection {}: {}", conn.id, e),
            }
        }
        migrated
    }

    fn persist_connection_secret(&mut self, conn_id: &str, password: Option<&str>) {
        if let Err(e) = Self::store_connection_secret(&*self.credentials, conn_id, password) {
            tracing::warn!("Failed to store password for connection {}: {}", conn_id, e);
            self.status_message = format!("⚠ Could not save password to the system keychain: {}", e);
        }
    }

    pub fn active_saved_connection(&self) -> Option<&SavedConnection> {
        if let Some(ref id) = self.active_connection_id {
            self.saved_connections.iter().find(|c| &c.id == id)
        } else {
            self.saved_connections.first()
        }
    }

    pub fn build_current_config(&self) -> ConnectionConfig {
        let port: u16 = self.form_port.parse().unwrap_or(match self.selected_driver_kind {
            DbDriverKind::Postgres => 5432,
            DbDriverKind::MySql => 3306,
            DbDriverKind::Sqlite => 0,
        });

        ConnectionConfig {
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
            ssh_tunnel: None,
            connect_timeout: std::time::Duration::from_millis(10000),
        }
    }

    pub fn test_connection(&mut self, cx: &mut Context<Self>) {
        if self.is_connecting {
            return;
        }

        self.is_connecting = true;
        self.status_message = "Testing database connection...".to_string();
        self.test_result = None;
        cx.notify();

        let config = self.build_current_config();

        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                // Drivers must live on the shared runtime: pools and sockets outlive this call
                let rt = db_wire::get_runtime();

                let timeout_duration = std::time::Duration::from_millis(10000);
                let outcome = rt.block_on(async {
                    tokio::time::timeout(timeout_duration, async {
                        let driver = create_driver(config)
                            .map_err(|e| format!("Driver initialization error: {}", e))?;
                        driver.ping().await
                            .map_err(|e| format!("Ping failed: {}", e))?;
                        Ok::<_, String>("Connection test successful! Database responded to ping.".to_string())
                    }).await
                });

                match outcome {
                    Ok(Ok(msg)) => Ok(msg),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err("Connection timed out (server unreachable or offline)".to_string()),
                }
            }).await;

            let _ = this.update(cx, |this, cx| {
                this.is_connecting = false;
                match result {
                    Ok(msg) => {
                        this.test_result = Some(Ok(msg));
                        this.status_message = "✓ Database connection test passed.".to_string();
                    }
                    Err(err) => {
                        this.test_result = Some(Err(err.clone()));
                        this.status_message = format!("✕ Test failed: {}", err);
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    pub fn connect_to_database(&mut self, cx: &mut Context<Self>) {
        if self.is_connecting {
            return;
        }

        self.is_connecting = true;
        self.status_message = "Connecting to database...".to_string();
        self.test_result = None;
        cx.notify();

        let config = self.build_current_config();

        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                // Drivers must live on the shared runtime: pools and sockets outlive this call
                let rt = db_wire::get_runtime();

                let timeout_duration = std::time::Duration::from_millis(15000);
                let outcome = rt.block_on(async {
                    tokio::time::timeout(timeout_duration, async {
                        let driver = create_driver(config.clone())
                            .map_err(|e| format!("Driver initialization error: {}", e))?;

                        driver.ping().await
                            .map_err(|e| format!("Connection refused: {}", e))?;

                        let tables = driver.list_tables().await
                            .map_err(|e| format!("Connected but failed to list tables: {}", e))?;

                        // Fast batch fetch all columns in 1 single query; fall back gracefully if schema discovery fails
                        let columns_info = driver.list_all_columns().await.unwrap_or_default();
                        let constraints_info = driver.list_all_constraints().await.unwrap_or_default();
                        let indexes_info = driver.list_all_indexes().await.unwrap_or_default();

                        Ok::<_, String>((driver, tables, columns_info, constraints_info, indexes_info, config))
                    }).await
                });

                match outcome {
                    Ok(Ok(val)) => Ok(val),
                    Ok(Err(e)) => Err(e),
                    Err(_) => Err("Connection timed out (server unreachable or request took too long)".to_string()),
                }
            }).await;

            let _ = this.update(cx, |this, cx| {
                this.is_connecting = false;
                match result {
                    Ok((driver, tables, columns_info, constraints_info, indexes_info, config)) => {
                        let _ = this.schema_cache.clear();
                        for t in &tables {
                            let _ = this.schema_cache.add_table(&t.schema, &t.name, &t.table_type);
                        }

                        let mut col_map = HashMap::new();
                        for (schema, table, cols) in columns_info {
                            for c in &cols {
                                let _ = this.schema_cache.add_column(&schema, &table, &c.name, c.data_type.display_name(), c.is_primary_key);
                            }
                            col_map.insert((schema, table), cols);
                        }
                        this.table_columns = col_map;

                        let mut constr_map = HashMap::new();
                        for (schema, table, cname, ctype) in constraints_info {
                            constr_map.entry((schema, table)).or_insert_with(Vec::new).push((cname, ctype));
                        }
                        this.table_constraints = constr_map;

                        let mut idx_map = HashMap::new();
                        for (schema, table, iname) in indexes_info {
                            idx_map.entry((schema, table)).or_insert_with(Vec::new).push(iname);
                        }
                        this.table_indexes = idx_map;

                        this.tables = tables;
                        this.driver = Some(driver);
                        this.is_connected = true;
                        this.show_connection_modal = false;
                        this.test_result = None;

                        let conn_id = this.editing_connection_id.clone().unwrap_or_else(|| {
                            format!("conn_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0))
                        });

                        let conn_name = if !this.form_name.trim().is_empty() {
                            this.form_name.trim().to_string()
                        } else {
                            match config.driver {
                                DbDriverKind::Sqlite => {
                                    std::path::Path::new(&config.database)
                                        .file_name()
                                        .and_then(|f| f.to_str())
                                        .unwrap_or(&config.database)
                                        .to_string()
                                }
                                _ => if !config.database.is_empty() {
                                    format!("{}-local", config.database)
                                } else {
                                    format!("{:?}-conn", config.driver)
                                },
                            }
                        };

                        let saved = SavedConnection {
                            id: conn_id.clone(),
                            name: conn_name.clone(),
                            driver: match config.driver {
                                DbDriverKind::Postgres => "Postgres".to_string(),
                                DbDriverKind::MySql => "MySql".to_string(),
                                DbDriverKind::Sqlite => "Sqlite".to_string(),
                            },
                            host: config.host.clone(),
                            port: config.port,
                            database: config.database.clone(),
                            user: config.user.clone(),
                            password: config.password.clone(),
                            use_unix_socket: config.use_unix_socket,
                        };

                        if let Some(pos) = this.saved_connections.iter().position(|c| c.id == conn_id) {
                            this.saved_connections[pos] = saved.clone();
                        } else {
                            this.saved_connections.push(saved.clone());
                        }
                        this.persist_connection_secret(&conn_id, config.password.as_deref());
                        this.active_connection_id = Some(conn_id.clone());
                        this.editing_connection_id = Some(conn_id);
                        this.save_config();

                        this.nav_connection_expanded = true;
                        this.nav_databases_expanded = true;
                        this.nav_database_expanded = true;
                        this.nav_schemas_expanded = true;
                        this.nav_table_expanded.clear();
                        this.nav_subfolder_expanded.clear();
                        this.selected_table_name = None;

                        for t in &this.tables {
                            this.nav_schema_expanded.insert(t.schema.clone());
                            this.nav_tables_expanded.insert(t.schema.clone());
                        }

                        let db_label = format!("{:?} ({}:{})", config.driver, config.host, config.port);
                        this.status_message = format!("🟢 Connected to: {} ({}) — {} tables found", conn_name, db_label, this.tables.len());

                        for tab in &mut this.tabs {
                            tab.editor.schema_cache = Some(this.schema_cache.clone());
                        }

                        if !this.tabs.is_empty() {
                            let tab = &mut this.tabs[this.active_tab];
                            let current_text = tab.editor.buffer.text();
                            if current_text.trim().is_empty() || current_text.starts_with("-- Welcome to FastDB") {
                                let welcome_conn = format!(
                                    "-- Connected to {}.\n-- Select a table from the navigator or write a query to execute.\n",
                                    conn_name
                                );
                                tab.editor = SqlEditorView::new(&welcome_conn, this.theme.clone(), Some(this.schema_cache.clone()));
                                tab.editor.buffer.set_cursor_offset(0);
                            }
                            tab.grid = VirtualDataGrid::new(None, this.theme.clone());
                        }
                    }
                    Err(e) => {
                        this.test_result = Some(Err(e.clone()));
                        this.status_message = format!("❌ Connection failed: {}", e);
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    pub fn disconnect(&mut self) {
        self.driver = None;
        self.tables.clear();
        let _ = self.schema_cache.clear();
        self.is_connected = false;
        self.selected_table_name = None;
        self.table_columns.clear();
        self.table_constraints.clear();
        self.table_indexes.clear();
        self.nav_table_expanded.clear();
        self.nav_subfolder_expanded.clear();
        self.nav_connection_expanded = false;
        self.status_message = "⚪ Disconnected from database — Click connection in navigator to reconnect.".to_string();
        if !self.tabs.is_empty() {
            let tab = &mut self.tabs[self.active_tab];
            tab.grid = VirtualDataGrid::new(None, self.theme.clone());
            tab.execution_time_ms = None;
            tab.fetch.reset_result();
        }
    }

    pub fn open_new_connection_dialog(&mut self) {
        self.editing_connection_id = None;
        self.form_name = "".to_string();
        self.selected_driver_kind = DbDriverKind::Postgres;
        self.form_host = "localhost".to_string();
        self.form_port = "5432".to_string();
        self.form_database = "".to_string();
        self.form_user = "postgres".to_string();
        self.form_password = "".to_string();
        self.form_use_unix_socket = false;
        self.active_field = Some(FormField::Name);
        self.field_cursor = usize::MAX;
        self.field_selected_all = false;
        self.cursor_visible = true;
        self.show_password = false;
        self.test_result = None;
        self.show_connection_modal = true;
    }

    pub fn edit_saved_connection(&mut self, conn_id: Option<&str>) {
        let target_conn = if let Some(id) = conn_id {
            self.saved_connections.iter().find(|c| c.id == id).cloned()
        } else if let Some(ref id) = self.active_connection_id {
            self.saved_connections.iter().find(|c| &c.id == id).cloned()
        } else {
            self.saved_connections.first().cloned()
        };

        if let Some(saved) = target_conn {
            self.selected_driver_kind = match saved.driver.as_str() {
                "MySql" => DbDriverKind::MySql,
                "Sqlite" => DbDriverKind::Sqlite,
                _ => DbDriverKind::Postgres,
            };
            self.form_name = saved.name.clone();
            self.form_host = saved.host.clone();
            self.form_port = saved.port.to_string();
            self.form_database = saved.database.clone();
            self.form_user = saved.user.clone();
            self.form_password = Self::resolve_connection_password(&*self.credentials, &saved).unwrap_or_default();
            self.form_use_unix_socket = saved.use_unix_socket;
            self.editing_connection_id = Some(saved.id.clone());
            self.test_result = None;
            self.active_field = Some(FormField::Database);
            self.field_cursor = usize::MAX;
            self.field_selected_all = false;
            self.cursor_visible = true;
            self.show_password = false;
            self.show_connection_modal = true;
        } else {
            self.open_new_connection_dialog();
        }
    }

    pub fn connect_saved_by_id(&mut self, conn_id: &str, cx: &mut Context<Self>) {
        if self.is_connecting {
            return;
        }
        let Some(saved) = self.saved_connections.iter().find(|c| c.id == conn_id).cloned() else {
            return;
        };

        self.selected_driver_kind = match saved.driver.as_str() {
            "MySql" => DbDriverKind::MySql,
            "Sqlite" => DbDriverKind::Sqlite,
            _ => DbDriverKind::Postgres,
        };
        self.form_name = saved.name.clone();
        self.form_host = saved.host.clone();
        self.form_port = saved.port.to_string();
        self.form_database = saved.database.clone();
        self.form_user = saved.user.clone();
        self.form_password = Self::resolve_connection_password(&*self.credentials, &saved).unwrap_or_default();
        self.form_use_unix_socket = saved.use_unix_socket;
        self.editing_connection_id = Some(saved.id.clone());
        self.active_connection_id = Some(saved.id.clone());
        self.nav_connection_expanded = true;
        self.connect_to_database(cx);
    }

    pub fn connect_saved(&mut self, cx: &mut Context<Self>) {
        if let Some(ref id) = self.active_connection_id.clone() {
            self.connect_saved_by_id(id, cx);
        } else if let Some(first) = self.saved_connections.first().map(|c| c.id.clone()) {
            self.connect_saved_by_id(&first, cx);
        } else {
            self.open_new_connection_dialog();
        }
    }

    pub fn delete_saved_connection(&mut self, conn_id: &str) {
        self.saved_connections.retain(|c| c.id != conn_id);
        if let Err(e) = self.credentials.delete(conn_id) {
            tracing::warn!("Failed to delete password for connection {}: {}", conn_id, e);
        }
        if self.active_connection_id.as_deref() == Some(conn_id) {
            if self.is_connected {
                self.disconnect();
            }
            self.active_connection_id = self.saved_connections.first().map(|c| c.id.clone());
        }
        self.save_config();
        self.status_message = "Removed connection from list.".to_string();
    }

    pub fn save_connection_form(&mut self) {
        let conn_id = self.editing_connection_id.clone().unwrap_or_else(|| {
            format!("conn_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0))
        });

        let config = self.build_current_config();

        let conn_name = if !self.form_name.trim().is_empty() {
            self.form_name.trim().to_string()
        } else {
            match config.driver {
                DbDriverKind::Sqlite => {
                    std::path::Path::new(&config.database)
                        .file_name()
                        .and_then(|f| f.to_str())
                        .unwrap_or(&config.database)
                        .to_string()
                }
                _ => if !config.database.is_empty() {
                    format!("{}-local", config.database)
                } else {
                    format!("{:?}-conn", config.driver)
                },
            }
        };

        let saved = SavedConnection {
            id: conn_id.clone(),
            name: conn_name,
            driver: match config.driver {
                DbDriverKind::Postgres => "Postgres".to_string(),
                DbDriverKind::MySql => "MySql".to_string(),
                DbDriverKind::Sqlite => "Sqlite".to_string(),
            },
            host: config.host.clone(),
            port: config.port,
            database: config.database.clone(),
            user: config.user.clone(),
            password: config.password.clone(),
            use_unix_socket: config.use_unix_socket,
        };

        if let Some(pos) = self.saved_connections.iter().position(|c| c.id == conn_id) {
            self.saved_connections[pos] = saved;
        } else {
            self.saved_connections.push(saved);
        }
        self.active_connection_id = Some(conn_id.clone());
        self.save_config();
        self.show_connection_modal = false;
        self.status_message = "✓ Saved connection to list.".to_string();
        self.persist_connection_secret(&conn_id, config.password.as_deref());
    }

    pub fn select_preset_postgres(&mut self) {
        self.selected_driver_kind = DbDriverKind::Postgres;
        self.form_host = "localhost".to_string();
        self.form_port = "5432".to_string();
        self.form_database = "postgres".to_string();
        self.form_user = "postgres".to_string();
        self.form_password = "".to_string();
        self.form_use_unix_socket = true;
        self.active_field = Some(FormField::Host);
        self.field_cursor = usize::MAX;
        self.test_result = None;
    }

    pub fn select_preset_mysql(&mut self) {
        self.selected_driver_kind = DbDriverKind::MySql;
        self.form_host = "localhost".to_string();
        self.form_port = "3306".to_string();
        self.form_database = "mysql".to_string();
        self.form_user = "root".to_string();
        self.form_password = "".to_string();
        self.form_use_unix_socket = true;
        self.active_field = Some(FormField::Host);
        self.field_cursor = usize::MAX;
        self.test_result = None;
    }

    pub fn select_preset_sqlite(&mut self) {
        self.selected_driver_kind = DbDriverKind::Sqlite;
        self.form_host = "localhost".to_string();
        self.form_port = "0".to_string();
        self.form_database = "fastdb.sqlite".to_string();
        self.form_user = "".to_string();
        self.form_password = "".to_string();
        self.form_use_unix_socket = false;
        self.active_field = Some(FormField::Database);
        self.field_cursor = usize::MAX;
        self.test_result = None;
    }
}
