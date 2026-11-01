use super::connection_config::ConnectionRepository;
fn connections_config_path() -> std::path::PathBuf {
    platform_hal::paths::config_dir().join("connections.json")
}
use super::*;

/// Navigator metadata of one connection
pub(super) struct SchemaMetadata {
    tables: Vec<TableSummary>,
    columns: Vec<(String, String, Vec<ColumnMeta>)>,
    constraints: Vec<(String, String, String, String)>,
    indexes: Vec<(String, String, String)>,
    details: Vec<(String, String, db_wire::TableDetails)>,
    details_error: Option<String>,
}

/// Loads tables, then columns, constraints and indexes in batch; only a failed
/// table listing is an error, the rest fall back to empty
async fn fetch_schema_metadata(driver: &dyn DatabaseDriver) -> Result<SchemaMetadata, String> {
    let tables = driver
        .list_tables()
        .await
        .map_err(|e| format!("Connected but failed to list tables: {}", e))?;
    let details = driver.list_table_details().await;
    let details_error = details
        .as_ref()
        .err()
        .map(|e| format!("Table details unavailable: {e}"));
    Ok(SchemaMetadata {
        tables,
        columns: driver.list_all_columns().await.unwrap_or_default(),
        constraints: driver.list_all_constraints().await.unwrap_or_default(),
        indexes: driver.list_all_indexes().await.unwrap_or_default(),
        details: details.unwrap_or_default(),
        details_error,
    })
}

impl Workspace {
    pub fn save_config(&self) {
        let connections = self.connections.saved_connections.clone();
        let active = self.connections.active_connection_id.clone();
        self.save_queue.connection_job(move || {
            ConnectionRepository::new(connections_config_path())
                .save(&connections, active.as_deref())
        });
    }

    pub fn test_connection(&mut self, cx: &mut Context<Self>) {
        if self.form.is_connecting {
            return;
        }

        self.form.is_connecting = true;
        self.form.test_result = None;
        cx.notify();

        let config = match self.form.build_config() {
            Ok(config) => config,
            Err(error) => {
                self.form.is_connecting = false;
                self.form.test_result = Some(Err(error));
                return;
            }
        };
        let request = self.form.begin_request();
        let snapshot = self.form.snapshot();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Drivers must live on the shared runtime: pools and sockets outlive this call
                    let rt = db_wire::get_runtime();

                    let timeout_duration = std::time::Duration::from_millis(10000);
                    let outcome = rt.block_on(async {
                        tokio::time::timeout(timeout_duration, async {
                            let driver = create_driver(config)
                                .map_err(|e| format!("Driver initialization error: {:#}", e))?;
                            driver
                                .ping()
                                .await
                                .map_err(|e| format!("Ping failed: {:#}", e))?;
                            Ok::<_, String>(
                                "Connection test successful! Database responded to ping."
                                    .to_string(),
                            )
                        })
                        .await
                    });

                    match outcome {
                        Ok(Ok(msg)) => Ok(msg),
                        Ok(Err(e)) => Err(e),
                        Err(_) => {
                            Err("Connection timed out (server unreachable or offline)".to_string())
                        }
                    }
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                if !this.form.accepts(request, &snapshot) {
                    if this.form.request_id == request {
                        this.form.is_connecting = false;
                        cx.notify();
                    }
                    return;
                }
                this.form.is_connecting = false;
                match result {
                    Ok(msg) => {
                        this.form.test_result = Some(Ok(msg));
                    }
                    Err(err) => {
                        this.form.test_result = Some(Err(err.clone()));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn connect_to_database(&mut self, cx: &mut Context<Self>) {
        if self.form.is_connecting {
            return;
        }

        self.form.is_connecting = true;
        self.form.test_result = None;
        cx.notify();

        let config = match self.form.build_config() {
            Ok(config) => config,
            Err(error) => {
                self.form.is_connecting = false;
                self.form.test_result = Some(Err(error));
                return;
            }
        };
        let request = self.form.begin_request();
        let snapshot = self.form.snapshot();
        let requested_id = self.form.editing_connection_id.clone();

        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    // Drivers must live on the shared runtime: pools and sockets outlive this call
                    let rt = db_wire::get_runtime();

                    let timeout_duration = std::time::Duration::from_millis(15000);
                    let outcome = rt.block_on(async {
                        tokio::time::timeout(timeout_duration, async {
                            let driver = create_driver(config.clone())
                                .map_err(|e| format!("Driver initialization error: {:#}", e))?;

                            driver
                                .ping()
                                .await
                                .map_err(|e| format!("Connection failed: {:#}", e))?;

                            let metadata = fetch_schema_metadata(driver.as_ref()).await?;

                            Ok::<_, String>((driver, metadata, config))
                        })
                        .await
                    });

                    match outcome {
                        Ok(Ok(val)) => Ok(val),
                        Ok(Err(e)) => Err(e),
                        Err(_) => Err(
                            "Connection timed out (server unreachable or request took too long)"
                                .to_string(),
                        ),
                    }
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                if !this.form.accepts(request, &snapshot) {
                    if this.form.request_id == request {
                        this.form.is_connecting = false;
                        cx.notify();
                    }
                    return;
                }
                this.form.is_connecting = false;
                match result {
                    Ok((driver, metadata, config)) => {
                        let conn_id = requested_id.unwrap_or_else(|| {
                            format!(
                                "conn_{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0)
                            )
                        });

                        this.connections.active_connection_id = Some(conn_id.clone());
                        this.connections.sessions.insert(
                            conn_id.clone(),
                            ConnectionSession {
                                kind: config.driver,
                                driver: Some(driver),
                                ..ConnectionSession::default()
                            },
                        );
                        this.apply_schema_metadata(metadata);
                        this.form.show_connection_modal = false;
                        this.form.test_result = None;

                        let conn_name = if !this.form.form_name.trim().is_empty() {
                            this.form.form_name.trim().to_string()
                        } else {
                            match config.driver {
                                DbDriverKind::Sqlite => std::path::Path::new(&config.database)
                                    .file_name()
                                    .and_then(|f| f.to_str())
                                    .unwrap_or(&config.database)
                                    .to_string(),
                                _ => {
                                    if !config.database.is_empty() {
                                        format!("{}-local", config.database)
                                    } else {
                                        format!("{:?}-conn", config.driver)
                                    }
                                }
                            }
                        };

                        let saved = SavedConnection {
                            id: conn_id.clone(),
                            name: conn_name.clone(),
                            driver: config.driver.name().to_string(),
                            host: config.host.clone(),
                            port: config.port,
                            database: config.database.clone(),
                            user: config.user.clone(),
                            password: config.password.clone(),
                            password_unavailable: config.password.is_none()
                                && this
                                    .connections
                                    .saved_connections
                                    .iter()
                                    .any(|c| c.id == conn_id && c.password_unavailable),
                            use_unix_socket: config.use_unix_socket,
                            tls_enabled: config.tls_enabled,
                            tls_ca_cert: config.tls_ca_cert.clone(),
                        };

                        if let Some(pos) = this
                            .connections
                            .saved_connections
                            .iter()
                            .position(|c| c.id == conn_id)
                        {
                            this.connections.saved_connections[pos] = saved.clone();
                        } else {
                            this.connections.saved_connections.push(saved.clone());
                        }
                        this.connections.active_connection_id = Some(conn_id.clone());
                        this.form.editing_connection_id = Some(conn_id);
                        this.save_config();

                        this.connections.navigator_mut().nav_connection_expanded = true;
                        this.connections.navigator_mut().nav_databases_expanded = true;
                        this.connections.navigator_mut().nav_database_expanded = true;
                        this.connections.navigator_mut().nav_schemas_expanded = true;
                        this.connections.navigator_mut().nav_table_expanded.clear();
                        this.connections
                            .navigator_mut()
                            .nav_subfolder_expanded
                            .clear();
                        this.connections.navigator_mut().selected_table_name = None;

                        for t in this.connections.active().tables.clone() {
                            this.connections
                                .navigator_mut()
                                .nav_schema_expanded
                                .insert(t.schema.clone());
                            this.connections
                                .navigator_mut()
                                .nav_tables_expanded
                                .insert(t.schema.clone());
                        }

                        let cache = this.connections.active().schema_cache.clone();
                        for tab in &mut this.tabs {
                            if tab.connection_id == this.connections.active_connection_id {
                                tab.editor.schema_cache = Some(cache.clone());
                            }
                        }
                    }
                    Err(e) => {
                        this.form.test_result = Some(Err(e.clone()));
                        // Connected from the navigator: open the saved connection so the error is
                        // visible and a missing password can be filled in
                        if !this.form.show_connection_modal
                            && this.form.editing_connection_id.is_some()
                        {
                            this.show_connection_error_dialog();
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Shows the connection dialog with the already loaded form and the failure banner
    fn show_connection_error_dialog(&mut self) {
        self.form.active_field = Some(if self.form.form_password.is_empty() {
            FormField::Password
        } else {
            FormField::Database
        });
        self.form.field_cursor = usize::MAX;
        self.form.field_anchor = None;
        self.input.cursor_visible = true;
        self.form.show_password = false;
        self.form.form_history.clear();
        self.form.show_connection_modal = true;
    }

    /// True when the connection is open, whether active or parked in the background
    pub fn is_connection_live(&self, conn_id: &str) -> bool {
        self.connections
            .sessions
            .get(conn_id)
            .is_some_and(|s| s.driver.is_some())
    }

    fn activate_live_connection(&mut self, conn_id: &str) -> bool {
        if !self.is_connection_live(conn_id) {
            return false;
        }
        self.connections.active_connection_id = Some(conn_id.to_owned());
        self.save_config();
        true
    }

    pub fn disconnect_by_id(&mut self, conn_id: &str) {
        if let Some(session) = self.connections.sessions.get_mut(conn_id) {
            session.driver = None;
        }
    }

    pub fn disconnect(&mut self) {
        if let Some(id) = self.connections.active_connection_id.clone() {
            self.disconnect_by_id(&id);
        }
    }

    pub fn refresh_connection(&mut self, cx: &mut Context<Self>) {
        if self.form.is_connecting {
            return;
        }
        let Some(driver) = self.connections.active().driver.clone() else {
            return;
        };
        cx.notify();

        let fetch_driver = driver.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let rt = db_wire::get_runtime();
                    let timeout_duration = std::time::Duration::from_millis(15000);
                    rt.block_on(async {
                        tokio::time::timeout(
                            timeout_duration,
                            fetch_schema_metadata(fetch_driver.as_ref()),
                        )
                        .await
                        .unwrap_or_else(|_| Err("Refresh timed out".to_string()))
                    })
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                // Ignore a result that arrives after a disconnect or a switch to another connection
                if !this
                    .connections
                    .active()
                    .driver
                    .as_ref()
                    .is_some_and(|d| Arc::ptr_eq(d, &driver))
                {
                    return;
                }
                if let Ok(metadata) = result {
                    this.apply_schema_metadata(metadata);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Replaces the cached tables, columns, constraints and indexes of the open connection
    fn apply_schema_metadata(&mut self, metadata: SchemaMetadata) {
        let _ = self.connections.active().schema_cache.clear();
        for t in &metadata.tables {
            let _ =
                self.connections
                    .active()
                    .schema_cache
                    .add_table(&t.schema, &t.name, &t.table_type);
        }

        let mut col_map = HashMap::new();
        for (schema, table, cols) in metadata.columns {
            for c in &cols {
                let _ = self.connections.active().schema_cache.add_column(
                    &schema,
                    &table,
                    &c.name,
                    c.data_type.display_name(),
                    c.is_primary_key,
                );
            }
            col_map.insert((schema, table), cols);
        }
        self.connections.active_mut().table_columns = col_map;

        let mut constr_map = HashMap::new();
        for (schema, table, cname, ctype) in metadata.constraints {
            constr_map
                .entry((schema, table))
                .or_insert_with(Vec::new)
                .push((cname, ctype));
        }
        self.connections.active_mut().table_constraints = constr_map;

        let mut idx_map = HashMap::new();
        for (schema, table, iname) in metadata.indexes {
            idx_map
                .entry((schema, table))
                .or_insert_with(Vec::new)
                .push(iname);
        }
        self.connections.active_mut().table_indexes = idx_map;

        self.connections.active_mut().tables = metadata.tables;
        let session = self.connections.active_mut();
        let mut tree = BTreeMap::<String, Vec<TableSummary>>::new();
        let mut properties = HashMap::new();
        let mut details: HashMap<_, _> = metadata
            .details
            .into_iter()
            .map(|(schema, table, details)| ((schema, table), details))
            .collect();
        for table in &session.tables {
            tree.entry(table.schema.clone())
                .or_default()
                .push(table.clone());
            let key = (table.schema.clone(), table.name.clone());
            let model = super::properties::model::TableProperties::new(
                session.kind,
                table.schema.clone(),
                table.name.clone(),
                session.table_columns.get(&key).cloned().unwrap_or_default(),
                session
                    .table_constraints
                    .get(&key)
                    .cloned()
                    .unwrap_or_default(),
                session.table_indexes.get(&key).cloned().unwrap_or_default(),
            )
            .with_metadata(details.remove(&key), metadata.details_error.clone());
            properties.insert(key, Arc::new(model));
        }
        session.schema_tree = Arc::new(tree);
        session.properties = properties;
    }

    pub fn open_new_connection_dialog(&mut self) {
        self.form.load(None, true);
        self.input.cursor_visible = true;
    }

    pub fn edit_saved_connection(&mut self, conn_id: Option<&str>) {
        self.form.invalidate();
        let target_conn = if let Some(id) = conn_id {
            self.connections
                .saved_connections
                .iter()
                .find(|c| c.id == id)
                .cloned()
        } else if let Some(ref id) = self.connections.active_connection_id {
            self.connections
                .saved_connections
                .iter()
                .find(|c| &c.id == id)
                .cloned()
        } else {
            self.connections.saved_connections.first().cloned()
        };

        if let Some(saved) = target_conn {
            self.form.load(Some(&saved), true);
            self.input.cursor_visible = true;
        } else {
            self.open_new_connection_dialog();
        }
    }

    pub fn connect_saved_by_id(&mut self, conn_id: &str, cx: &mut Context<Self>) {
        if self.form.is_connecting {
            return;
        }
        if self.connections.active().driver.is_some()
            && self.connections.active_connection_id.as_deref() == Some(conn_id)
        {
            self.connections.navigator_mut().nav_connection_expanded = true;
            return;
        }
        if self.activate_live_connection(conn_id) {
            return;
        }
        if !self.load_connection_form(conn_id) {
            return;
        }
        self.connections.active_connection_id = Some(conn_id.to_string());
        self.connections.navigator_mut().nav_connection_expanded = true;
        self.connect_to_database(cx);
    }

    /// Fills the connection form from a saved connection; false when it does not exist
    fn load_connection_form(&mut self, conn_id: &str) -> bool {
        let Some(saved) = self
            .connections
            .saved_connections
            .iter()
            .find(|c| c.id == conn_id)
            .cloned()
        else {
            return false;
        };

        self.form.load(Some(&saved), false);
        if saved.password_unavailable {
            self.form.show_connection_modal = true;
            self.form.test_result = Some(Err("Saved password unavailable. Unlock the keychain and restart, or re-enter the password.".into()));
            return false;
        }
        true
    }

    pub fn delete_saved_connection(&mut self, conn_id: &str) {
        self.connections
            .saved_connections
            .retain(|c| c.id != conn_id);
        self.connections.sessions.remove(conn_id);
        if self.connections.active_connection_id.as_deref() == Some(conn_id) {
            if self.connections.active().driver.is_some() {
                self.disconnect();
            }
            self.connections.active_connection_id = self
                .connections
                .saved_connections
                .first()
                .map(|c| c.id.clone());
        }
        self.save_config();
    }

    pub fn save_connection_form(&mut self) {
        let conn_id = self.form.editing_connection_id.clone().unwrap_or_else(|| {
            format!(
                "conn_{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis())
                    .unwrap_or(0)
            )
        });

        let config = match self.form.build_config() {
            Ok(config) => config,
            Err(error) => {
                self.form.is_connecting = false;
                self.form.test_result = Some(Err(error));
                return;
            }
        };

        let conn_name = if !self.form.form_name.trim().is_empty() {
            self.form.form_name.trim().to_string()
        } else {
            match config.driver {
                DbDriverKind::Sqlite => std::path::Path::new(&config.database)
                    .file_name()
                    .and_then(|f| f.to_str())
                    .unwrap_or(&config.database)
                    .to_string(),
                _ => {
                    if !config.database.is_empty() {
                        format!("{}-local", config.database)
                    } else {
                        format!("{:?}-conn", config.driver)
                    }
                }
            }
        };

        let saved = SavedConnection {
            id: conn_id.clone(),
            name: conn_name,
            driver: config.driver.name().to_string(),
            host: config.host.clone(),
            port: config.port,
            database: config.database.clone(),
            user: config.user.clone(),
            password: config.password.clone(),
            password_unavailable: config.password.is_none()
                && self
                    .connections
                    .saved_connections
                    .iter()
                    .any(|c| c.id == conn_id && c.password_unavailable),
            use_unix_socket: config.use_unix_socket,
            tls_enabled: config.tls_enabled,
            tls_ca_cert: config.tls_ca_cert.clone(),
        };

        if let Some(pos) = self
            .connections
            .saved_connections
            .iter()
            .position(|c| c.id == conn_id)
        {
            self.connections.saved_connections[pos] = saved;
        } else {
            self.connections.saved_connections.push(saved);
        }

        self.save_config();
        self.form.show_connection_modal = false;
    }
}
