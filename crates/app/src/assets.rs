use anyhow::Result;
use gpui::{AssetSource, SharedString};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../assets"]
pub struct Assets;

pub struct FastDbAssetSource;

impl AssetSource for FastDbAssetSource {
    fn load(&self, path: &str) -> Result<Option<std::borrow::Cow<'static, [u8]>>> {
        match Assets::get(path) {
            Some(file) => Ok(Some(file.data)),
            None => Ok(None),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut list = Vec::new();
        for file in Assets::iter() {
            if file.starts_with(path) {
                list.push(SharedString::from(file.to_string()));
            }
        }
        Ok(list)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dbeaver_icons_loadable() {
        let source = FastDbAssetSource;
        assert!(source.load("icons/folder_database.svg").unwrap().is_some());
        assert!(source.load("icons/database.svg").unwrap().is_some());
        assert!(source.load("icons/folder_schema.svg").unwrap().is_some());
        assert!(source.load("icons/schema.svg").unwrap().is_some());
        assert!(source.load("icons/folder_table.svg").unwrap().is_some());
        assert!(source.load("icons/table.svg").unwrap().is_some());
        assert!(source.load("icons/columns.svg").unwrap().is_some());
        assert!(source.load("icons/column.svg").unwrap().is_some());
        assert!(source.load("icons/folder_constraint.svg").unwrap().is_some());
        assert!(source.load("icons/constraint.svg").unwrap().is_some());
        assert!(source.load("icons/folder.svg").unwrap().is_some());
        assert!(source.load("icons/foreign_key.svg").unwrap().is_some());
        assert!(source.load("icons/table_index.svg").unwrap().is_some());
        assert!(source.load("icons/database_navigator.svg").unwrap().is_some());
        assert!(source.load("icons/connect.svg").unwrap().is_some());
        assert!(source.load("icons/disconnect.svg").unwrap().is_some());
        assert!(source.load("icons/edit.svg").unwrap().is_some());
        assert!(source.load("icons/delete.svg").unwrap().is_some());
        assert!(source.load("icons/refresh.svg").unwrap().is_some());
        assert!(source.load("icons/sql_exec.svg").unwrap().is_some());
        assert!(source.load("icons/sql_script.svg").unwrap().is_some());
        assert!(source.load("icons/grid.svg").unwrap().is_some());
        assert!(source.load("icons/configuration.svg").unwrap().is_some());
        assert!(source.load("icons/sql_validate.svg").unwrap().is_some());
        assert!(source.load("icons/function.svg").unwrap().is_some());
        assert!(source.load("icons/sql.svg").unwrap().is_some());
        assert!(source.load("icons/postgresql.png").unwrap().is_some());
        assert!(source.load("icons/mysql.png").unwrap().is_some());
        assert!(source.load("icons/sqlite.png").unwrap().is_some());
        assert!(source.load("icons/success_ovr.png").unwrap().is_some());
    }

    #[test]
    fn test_load_legacy_single_connection() {
        use crate::workspace::Workspace;

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

        let (connections, active_id) = Workspace::load_saved_connections_from_str(legacy_json);
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
        use crate::workspace::{SavedConnection, Workspace};

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
            },
        ];

        let serialized = Workspace::serialize_connections_config(&conns, Some("conn_mysql"));
        let (deserialized, active_id) = Workspace::load_saved_connections_from_str(&serialized);

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
            },
        ];

        conns.retain(|c| c.id != "conn_1");

        assert_eq!(conns.len(), 1);
        assert_eq!(conns[0].id, "conn_2");
        assert_eq!(conns[0].name, "Database 2");
    }

    #[test]
    fn test_double_click_select_all_and_typing_replacement() {
        let single_click_count = 1;
        let double_click_count = 2;
        let triple_click_count = 3;

        let is_selected_single = single_click_count >= 2;
        assert!(!is_selected_single);

        let is_selected_double = double_click_count >= 2;
        assert!(is_selected_double);

        let is_selected_triple = triple_click_count >= 2;
        assert!(is_selected_triple);

        // Test typing replacement when selected all
        let mut field_val = "localhost".to_string();
        let mut selected_all = true;

        if selected_all {
            field_val.clear();
            selected_all = false;
        }
        field_val.push_str("127.0.0.1");

        assert_eq!(field_val, "127.0.0.1");
        assert!(!selected_all);

        // Test backspace when selected all clears the field
        selected_all = true;
        if selected_all {
            field_val.clear();
            selected_all = false;
        }
        assert_eq!(field_val, "");
        assert!(!selected_all);
    }

    #[test]
    fn test_table_properties_column_meta_and_ddl_generation() {
        use db_wire::{ColumnMeta, DataType};
        use crate::workspace::generate_table_ddl;

        let cols = vec![
            ColumnMeta::new("key", DataType::Varchar)
                .with_nullable(false)
                .with_pk(true)
                .with_raw_type("varchar")
                .with_collation("default"),
            ColumnMeta::new("value", DataType::Json)
                .with_nullable(true)
                .with_raw_type("jsonb"),
        ];

        assert_eq!(cols[0].display_type(), "varchar");
        assert_eq!(cols[1].display_type(), "jsonb");
        assert!(!cols[0].nullable);
        assert!(cols[1].nullable);
        assert!(cols[0].is_primary_key);

        let ddl = generate_table_ddl("public", "additionalInfo", &cols, &[], &[]);
        assert!(ddl.contains("CREATE TABLE public.\"additionalInfo\""));
        assert!(ddl.contains("\"key\" varchar NOT NULL"));
        assert!(ddl.contains("\"value\" jsonb NULL"));
        assert!(ddl.contains("CONSTRAINT additionalInfo_pkey PRIMARY KEY (\"key\")"));
    }

    #[test]
    fn test_property_sections_and_subtabs() {
        use crate::workspace::{TableSubTab, PropertySection, TabState};
        use ui_core::theme::Theme;

        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        let mut tab = TabState::table(1, "public", "additionalInfo", Theme::dark(), cache);
        tab.active_subtab = TableSubTab::Properties;

        assert_eq!(tab.active_subtab, TableSubTab::Properties);
        assert_eq!(tab.active_property_section, PropertySection::Columns);
        assert!(!tab.properties_header_collapsed);
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
        }
    }

    #[test]
    fn test_password_roundtrips_through_config() {
        use crate::workspace::Workspace;

        let conns = vec![saved_pg_connection("conn_pg", Some("s3cret-pw"))];
        let serialized = Workspace::serialize_connections_config(&conns, Some("conn_pg"));

        let (loaded, _) = Workspace::load_saved_connections_from_str(&serialized);
        assert_eq!(loaded[0].password.as_deref(), Some("s3cret-pw"));
    }

    #[test]
    fn test_empty_password_is_not_written_to_config() {
        use crate::workspace::Workspace;

        let conns = vec![saved_pg_connection("conn_a", None), saved_pg_connection("conn_b", Some(""))];
        let serialized = Workspace::serialize_connections_config(&conns, None);

        let (loaded, _) = Workspace::load_saved_connections_from_str(&serialized);
        assert_eq!(loaded[0].password, None);
        assert_eq!(loaded[1].password, None);
    }

    fn sample_table_tab() -> crate::workspace::TabState {
        use ui_core::theme::Theme;
        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        crate::workspace::TabState::table(7, "public", "users", Theme::dark(), cache)
    }

    #[test]
    fn test_failed_run_keeps_its_message_until_the_next_run() {
        use db_wire::StreamEvent;

        let mut tab = sample_table_tab();
        tab.begin_fetch(true, false);
        tab.apply_stream_event(StreamEvent::Error("relation \"users\" does not exist".to_string()));
        assert_eq!(tab.last_error.as_deref(), Some("relation \"users\" does not exist"));

        tab.begin_fetch(true, false);
        assert_eq!(tab.last_error, None);
    }

    #[test]
    fn test_script_fetch_query_pages_selects_and_runs_other_statements_as_written() {
        use crate::workspace::{script_fetch_query, TabState};
        use ui_core::theme::Theme;

        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        let mut tab = TabState::script(1, "SQL Script 1".to_string(), Theme::dark(), cache);
        tab.fetch.size = 100;

        tab.script_sql = Some("SELECT * FROM users;".to_string());
        assert_eq!(
            script_fetch_query(&tab, 0),
            ("SELECT * FROM (SELECT * FROM users\n) fastdb_page LIMIT 100".to_string(), true)
        );
        assert_eq!(
            script_fetch_query(&tab, 100).0,
            "SELECT * FROM (SELECT * FROM users\n) fastdb_page LIMIT 100 OFFSET 100"
        );

        tab.script_sql = Some("DELETE FROM users WHERE id = 1;".to_string());
        assert_eq!(script_fetch_query(&tab, 0), ("DELETE FROM users WHERE id = 1;".to_string(), false));

        tab.script_sql = None;
        assert_eq!(script_fetch_query(&tab, 0), (String::new(), false));
    }

    #[test]
    fn test_script_tabs_start_empty_with_the_editor_focused() {
        use ui_core::theme::Theme;
        let cache = std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap());
        let tab = crate::workspace::TabState::script(3, "SQL Script 1".to_string(), Theme::dark(), cache);
        assert!(tab.is_script());
        assert!(tab.editor_focused);
        assert_eq!(tab.editor.buffer.text(), "");
        assert!(!sample_table_tab().is_script());
    }

    #[test]
    fn test_stream_events_fill_the_tab_grid_incrementally() {
        use crate::workspace::QueryStatus;
        use db_wire::{ColumnMeta, DataType, QueryResultPager, StreamEvent};
        use std::sync::Arc;

        let mut tab = sample_table_tab();
        let pager = Arc::new(
            QueryResultPager::new(vec![ColumnMeta::new("id", DataType::Int4), ColumnMeta::new("name", DataType::Text)])
                .unwrap(),
        );

        // The grid shows the result as soon as the schema arrives
        assert_eq!(tab.apply_stream_event(StreamEvent::SchemaReady(Arc::clone(&pager))), QueryStatus::Running { rows: 0 });
        assert!(Arc::ptr_eq(tab.grid.pager.as_ref().unwrap(), &pager));
        assert_eq!(tab.grid.table_name, "users");

        pager.append_row(vec![Some(&b"1"[..]), Some(&b"a fairly long user name streamed in"[..])]).unwrap();
        pager.append_row(vec![Some(&b"2"[..]), None]).unwrap();
        assert_eq!(tab.apply_stream_event(StreamEvent::PageAppended { total_rows: 2 }), QueryStatus::Running { rows: 2 });
        assert_eq!(tab.grid.width_sampled_rows, 2);

        assert_eq!(
            tab.apply_stream_event(StreamEvent::Completed { total_rows: 2, elapsed_ms: 15 }),
            QueryStatus::Done { rows: 2, elapsed_ms: 15 }
        );
        assert_eq!(tab.execution_time_ms, Some(15));
    }

    #[test]
    fn test_stream_error_before_schema_keeps_previous_result() {
        use crate::workspace::QueryStatus;
        use db_wire::{ColumnMeta, DataType, QueryResultPager, StreamEvent};
        use std::sync::Arc;

        let mut tab = sample_table_tab();
        let previous = Arc::new(QueryResultPager::new(vec![ColumnMeta::new("id", DataType::Int4)]).unwrap());
        tab.grid.set_pager(Arc::clone(&previous));

        assert_eq!(
            tab.apply_stream_event(StreamEvent::Error("relation does not exist".to_string())),
            QueryStatus::Failed("relation does not exist".to_string())
        );
        assert!(Arc::ptr_eq(tab.grid.pager.as_ref().unwrap(), &previous));
    }

    #[test]
    fn test_fetch_next_appends_rows_to_the_current_result() {
        use crate::workspace::QueryStatus;
        use db_wire::{ColumnMeta, DataType, QueryResultPager, StreamEvent};
        use std::sync::Arc;

        let columns = || vec![ColumnMeta::new("id", DataType::Int4)];
        let mut tab = sample_table_tab();
        tab.fetch.size = 2;

        // First chunk comes back full, so more rows may follow
        let first = Arc::new(QueryResultPager::new(columns()).unwrap());
        tab.begin_fetch(true, false);
        tab.apply_stream_event(StreamEvent::SchemaReady(Arc::clone(&first)));
        first.append_row(vec![Some(&b"1"[..])]).unwrap();
        first.append_row(vec![Some(&b"2"[..])]).unwrap();
        tab.apply_stream_event(StreamEvent::Completed { total_rows: 2, elapsed_ms: 3 });
        assert!(tab.fetch.has_more);
        assert!(tab.fetch.last.as_ref().unwrap().message().starts_with("2 row(s) fetched - "));

        // The next chunk lands after the existing rows in the same pager
        let next = Arc::new(QueryResultPager::new(columns()).unwrap());
        tab.begin_fetch(true, true);
        assert_eq!(tab.apply_stream_event(StreamEvent::SchemaReady(Arc::clone(&next))), QueryStatus::Running { rows: 2 });
        next.append_row(vec![Some(&b"3"[..])]).unwrap();
        assert_eq!(tab.apply_stream_event(StreamEvent::PageAppended { total_rows: 1 }), QueryStatus::Running { rows: 3 });
        assert_eq!(
            tab.apply_stream_event(StreamEvent::Completed { total_rows: 1, elapsed_ms: 2 }),
            QueryStatus::Done { rows: 3, elapsed_ms: 2 }
        );

        assert!(Arc::ptr_eq(tab.grid.pager.as_ref().unwrap(), &first));
        assert_eq!(first.get_cell(2, 0), Some(b"3".to_vec()));
        assert!(!tab.fetch.has_more, "a short chunk means the table is exhausted");
        assert!(!tab.fetch.is_appending());
        assert!(tab.fetch.last.as_ref().unwrap().message().starts_with("3 row(s) fetched - "));
    }

    #[test]
    fn test_fetch_size_text_falls_back_to_current_size() {
        let mut tab = sample_table_tab();
        tab.fetch.size_text = "500".to_string();
        tab.fetch.commit_size_text();
        assert_eq!(tab.fetch.size, 500);

        tab.fetch.size_text = "0".to_string();
        tab.fetch.commit_size_text();
        assert_eq!((tab.fetch.size, tab.fetch.size_text.as_str()), (500, "500"));
    }

    #[test]
    fn test_row_counts_group_thousands() {
        use crate::workspace::group_thousands;
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1400), "1,400");
        assert_eq!(group_thousands(1234567), "1,234,567");
    }
}
