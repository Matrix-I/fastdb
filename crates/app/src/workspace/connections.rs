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

/// Navigator metadata of one connection
pub(super) struct SchemaMetadata {
    tables: Vec<TableSummary>,
    columns: Vec<(String, String, Vec<ColumnMeta>)>,
    constraints: Vec<(String, String, String, String)>,
    indexes: Vec<(String, String, String)>,
}

/// An open connection parked in the background while another one is active.
/// Holds everything the navigator needs so switching back needs no reconnect.
pub struct LiveConnection {
    driver: Arc<dyn DatabaseDriver>,
    tables: Vec<TableSummary>,
    table_columns: HashMap<(String, String), Vec<ColumnMeta>>,
    table_constraints: HashMap<(String, String), Vec<(String, String)>>,
    table_indexes: HashMap<(String, String), Vec<String>>,
    nav_databases_expanded: bool,
    nav_database_expanded: bool,
    nav_schemas_expanded: bool,
    nav_schema_expanded: HashSet<String>,
    nav_tables_expanded: HashSet<String>,
    nav_table_expanded: HashSet<String>,
    nav_subfolder_expanded: HashSet<String>,
    selected_table_name: Option<String>,
}

/// Loads tables, then columns, constraints and indexes in batch; only a failed
/// table listing is an error, the rest fall back to empty
async fn fetch_schema_metadata(driver: &dyn DatabaseDriver) -> Result<SchemaMetadata, String> {
    let tables = driver.list_tables().await
        .map_err(|e| format!("Connected but failed to list tables: {}", e))?;
    Ok(SchemaMetadata {
        tables,
        columns: driver.list_all_columns().await.unwrap_or_default(),
        constraints: driver.list_all_constraints().await.unwrap_or_default(),
        indexes: driver.list_all_indexes().await.unwrap_or_default(),
    })
}

/// Where builds before the config dir kept the connections, relative to the working directory
const LEGACY_CONFIG_FILE: &str = ".fastdb_config.json";

/// The saved connections file, in the user's config dir so every build and launch shares it
fn connections_config_path() -> std::path::PathBuf {
    platform_hal::paths::config_dir().join("connections.json")
}

fn write_connections_config(content: &str) -> std::io::Result<()> {
    let path = connections_config_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Written beside the file and renamed over it, so a crash mid-write never loses the connections
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, &path)
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

    /// Reads the saved connections from the user's config dir. A config left by older builds
    /// in the working directory is picked up once and moved there.
    pub fn load_saved_connections() -> (Vec<SavedConnection>, Option<String>) {
        let path = connections_config_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            return Self::load_saved_connections_from_str(&content);
        }
        let Ok(content) = std::fs::read_to_string(LEGACY_CONFIG_FILE) else {
            return (Vec::new(), None);
        };
        let loaded = Self::load_saved_connections_from_str(&content);
        if let Err(e) = write_connections_config(&content) {
            tracing::warn!("Failed to move {} to {}: {}", LEGACY_CONFIG_FILE, path.display(), e);
        }
        loaded
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
                    "password": c.password.as_deref().filter(|p| !p.is_empty()),
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
        if let Err(e) = write_connections_config(&content) {
            tracing::error!("Failed to save connections to {}: {}", connections_config_path().display(), e);
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
                            .map_err(|e| format!("Driver initialization error: {:#}", e))?;
                        driver.ping().await
                            .map_err(|e| format!("Ping failed: {:#}", e))?;
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
                    }
                    Err(err) => {
                        this.test_result = Some(Err(err.clone()));
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
                            .map_err(|e| format!("Driver initialization error: {:#}", e))?;

                        driver.ping().await
                            .map_err(|e| format!("Connection failed: {:#}", e))?;

                        let metadata = fetch_schema_metadata(driver.as_ref()).await?;

                        Ok::<_, String>((driver, metadata, config))
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
                    Ok((driver, metadata, config)) => {
                        let conn_id = this.editing_connection_id.clone().unwrap_or_else(|| {
                            format!("conn_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0))
                        });

                        // Keep the previously active connection open in the background
                        if this.active_connection_id.as_deref() != Some(conn_id.as_str()) {
                            this.park_active_connection();
                        }
                        // A reconnect replaces any parked session of the same connection
                        this.live_connections.remove(&conn_id);

                        this.apply_schema_metadata(metadata);
                        this.driver = Some(driver);
                        this.is_connected = true;
                        this.show_connection_modal = false;
                        this.test_result = None;

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

                        for tab in &mut this.tabs {
                            tab.editor.schema_cache = Some(this.schema_cache.clone());
                        }

                        if !this.tabs.is_empty() {
                            let tab = &mut this.tabs[this.active_tab];
                            tab.grid = VirtualDataGrid::new(None, this.theme.clone());
                        }
                    }
                    Err(e) => {
                        this.test_result = Some(Err(e.clone()));
                        // Connected from the navigator: open the saved connection so the error is
                        // visible and a missing password can be filled in
                        if !this.show_connection_modal && this.editing_connection_id.is_some() {
                            this.show_connection_error_dialog();
                        }
                    }
                }
                cx.notify();
            });
        }).detach();
    }

    /// Shows the connection dialog with the already loaded form and the failure banner
    fn show_connection_error_dialog(&mut self) {
        self.active_field = Some(if self.form_password.is_empty() { FormField::Password } else { FormField::Database });
        self.field_cursor = usize::MAX;
        self.field_anchor = None;
        self.cursor_visible = true;
        self.show_password = false;
        self.form_history.clear();
        self.show_connection_modal = true;
    }

    /// True when the connection is open, whether active or parked in the background
    pub fn is_connection_live(&self, conn_id: &str) -> bool {
        (self.is_connected && self.active_connection_id.as_deref() == Some(conn_id))
            || self.live_connections.contains_key(conn_id)
    }

    /// Moves the active open connection into `live_connections`, leaving the
    /// workspace disconnected but the connection itself open
    fn park_active_connection(&mut self) {
        if !self.is_connected {
            return;
        }
        let (Some(id), Some(driver)) = (self.active_connection_id.clone(), self.driver.take()) else {
            return;
        };
        let live = LiveConnection {
            driver,
            tables: std::mem::take(&mut self.tables),
            table_columns: std::mem::take(&mut self.table_columns),
            table_constraints: std::mem::take(&mut self.table_constraints),
            table_indexes: std::mem::take(&mut self.table_indexes),
            nav_databases_expanded: self.nav_databases_expanded,
            nav_database_expanded: self.nav_database_expanded,
            nav_schemas_expanded: self.nav_schemas_expanded,
            nav_schema_expanded: std::mem::take(&mut self.nav_schema_expanded),
            nav_tables_expanded: std::mem::take(&mut self.nav_tables_expanded),
            nav_table_expanded: std::mem::take(&mut self.nav_table_expanded),
            nav_subfolder_expanded: std::mem::take(&mut self.nav_subfolder_expanded),
            selected_table_name: self.selected_table_name.take(),
        };
        self.live_connections.insert(id, live);
        let _ = self.schema_cache.clear();
        self.is_connected = false;
    }

    /// Makes a parked connection the active one without reconnecting.
    /// Returns false when the connection is not parked.
    fn activate_live_connection(&mut self, conn_id: &str) -> bool {
        if !self.live_connections.contains_key(conn_id) {
            return false;
        }
        self.park_active_connection();
        let Some(live) = self.live_connections.remove(conn_id) else {
            return false;
        };

        let _ = self.schema_cache.clear();
        for t in &live.tables {
            let _ = self.schema_cache.add_table(&t.schema, &t.name, &t.table_type);
        }
        for ((schema, table), cols) in &live.table_columns {
            for c in cols {
                let _ = self.schema_cache.add_column(schema, table, &c.name, c.data_type.display_name(), c.is_primary_key);
            }
        }

        self.driver = Some(live.driver);
        self.tables = live.tables;
        self.table_columns = live.table_columns;
        self.table_constraints = live.table_constraints;
        self.table_indexes = live.table_indexes;
        self.nav_connection_expanded = true;
        self.nav_databases_expanded = live.nav_databases_expanded;
        self.nav_database_expanded = live.nav_database_expanded;
        self.nav_schemas_expanded = live.nav_schemas_expanded;
        self.nav_schema_expanded = live.nav_schema_expanded;
        self.nav_tables_expanded = live.nav_tables_expanded;
        self.nav_table_expanded = live.nav_table_expanded;
        self.nav_subfolder_expanded = live.nav_subfolder_expanded;
        self.selected_table_name = live.selected_table_name;
        self.is_connected = true;
        self.active_connection_id = Some(conn_id.to_string());
        self.load_connection_form(conn_id);
        self.save_config();

        // Results in the active tab came from the previous connection
        if !self.tabs.is_empty() {
            let tab = &mut self.tabs[self.active_tab];
            tab.grid = VirtualDataGrid::new(None, self.theme.clone());
            tab.execution_time_ms = None;
            tab.fetch.reset_result();
        }
        for tab in &mut self.tabs {
            tab.editor.schema_cache = Some(self.schema_cache.clone());
        }

        true
    }

    /// Closes one connection: the active one, or a parked one in the background
    pub fn disconnect_by_id(&mut self, conn_id: &str) {
        if self.live_connections.remove(conn_id).is_none()
            && self.active_connection_id.as_deref() == Some(conn_id)
            && self.is_connected
        {
            self.disconnect();
        }
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
        if !self.tabs.is_empty() {
            let tab = &mut self.tabs[self.active_tab];
            tab.grid = VirtualDataGrid::new(None, self.theme.clone());
            tab.execution_time_ms = None;
            tab.fetch.reset_result();
        }
    }

    /// Reloads the navigator metadata of the open connection without reconnecting
    pub fn refresh_connection(&mut self, cx: &mut Context<Self>) {
        if self.is_connecting {
            return;
        }
        let Some(driver) = self.driver.clone() else {
            return;
        };
        cx.notify();

        let fetch_driver = driver.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move {
                let rt = db_wire::get_runtime();
                let timeout_duration = std::time::Duration::from_millis(15000);
                rt.block_on(async {
                    tokio::time::timeout(timeout_duration, fetch_schema_metadata(fetch_driver.as_ref()))
                        .await
                        .unwrap_or_else(|_| Err("Refresh timed out".to_string()))
                })
            }).await;

            let _ = this.update(cx, |this, cx| {
                // Ignore a result that arrives after a disconnect or a switch to another connection
                if !this.driver.as_ref().is_some_and(|d| Arc::ptr_eq(d, &driver)) {
                    return;
                }
                if let Ok(metadata) = result {
                    this.apply_schema_metadata(metadata);
                }
                cx.notify();
            });
        }).detach();
    }

    /// Replaces the cached tables, columns, constraints and indexes of the open connection
    fn apply_schema_metadata(&mut self, metadata: SchemaMetadata) {
        let _ = self.schema_cache.clear();
        for t in &metadata.tables {
            let _ = self.schema_cache.add_table(&t.schema, &t.name, &t.table_type);
        }

        let mut col_map = HashMap::new();
        for (schema, table, cols) in metadata.columns {
            for c in &cols {
                let _ = self.schema_cache.add_column(&schema, &table, &c.name, c.data_type.display_name(), c.is_primary_key);
            }
            col_map.insert((schema, table), cols);
        }
        self.table_columns = col_map;

        let mut constr_map = HashMap::new();
        for (schema, table, cname, ctype) in metadata.constraints {
            constr_map.entry((schema, table)).or_insert_with(Vec::new).push((cname, ctype));
        }
        self.table_constraints = constr_map;

        let mut idx_map = HashMap::new();
        for (schema, table, iname) in metadata.indexes {
            idx_map.entry((schema, table)).or_insert_with(Vec::new).push(iname);
        }
        self.table_indexes = idx_map;

        self.tables = metadata.tables;
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
        self.field_anchor = None;
        self.cursor_visible = true;
        self.show_password = false;
        self.test_result = None;
        self.form_history.clear();
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
            self.form_password = saved.password.clone().unwrap_or_default();
            self.form_use_unix_socket = saved.use_unix_socket;
            self.editing_connection_id = Some(saved.id.clone());
            self.test_result = None;
            self.active_field = Some(FormField::Database);
            self.field_cursor = usize::MAX;
            self.field_anchor = None;
            self.cursor_visible = true;
            self.show_password = false;
            self.form_history.clear();
            self.show_connection_modal = true;
        } else {
            self.open_new_connection_dialog();
        }
    }

    pub fn connect_saved_by_id(&mut self, conn_id: &str, cx: &mut Context<Self>) {
        if self.is_connecting {
            return;
        }
        if self.is_connected && self.active_connection_id.as_deref() == Some(conn_id) {
            self.nav_connection_expanded = true;
            return;
        }
        if self.activate_live_connection(conn_id) {
            return;
        }
        if !self.load_connection_form(conn_id) {
            return;
        }
        self.park_active_connection();
        self.active_connection_id = Some(conn_id.to_string());
        self.nav_connection_expanded = true;
        self.connect_to_database(cx);
    }

    /// Fills the connection form from a saved connection; false when it does not exist
    fn load_connection_form(&mut self, conn_id: &str) -> bool {
        let Some(saved) = self.saved_connections.iter().find(|c| c.id == conn_id).cloned() else {
            return false;
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
        self.form_password = saved.password.clone().unwrap_or_default();
        self.form_use_unix_socket = saved.use_unix_socket;
        self.editing_connection_id = Some(saved.id.clone());
        true
    }

    pub fn delete_saved_connection(&mut self, conn_id: &str) {
        self.saved_connections.retain(|c| c.id != conn_id);
        self.live_connections.remove(conn_id);
        if self.active_connection_id.as_deref() == Some(conn_id) {
            if self.is_connected {
                self.disconnect();
            }
            self.active_connection_id = self.saved_connections.first().map(|c| c.id.clone());
        }
        self.save_config();
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
