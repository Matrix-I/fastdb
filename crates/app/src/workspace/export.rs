mod json;
mod markup;

use super::*;
use db_wire::{QueryResultPager, export::*, sql::SqlDialect};
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Default)]
pub(super) struct ExportState {
    pub busy: bool,
    pub message: Option<String>,
    /// Present while a full-source export runs; sending `true` stops it.
    pub cancel: Option<tokio::sync::watch::Sender<bool>>,
}

#[derive(Clone, Copy)]
pub(super) enum ExportFormat {
    Csv,
    Sql,
    Html,
    Markdown,
    Json,
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Sql => "sql",
            Self::Html => "html",
            Self::Markdown => "md",
            Self::Json => "json",
        }
    }
}

pub(super) fn sql_target(dialect: SqlDialect, schema: Option<&str>, table: Option<&str>) -> String {
    let table = table.unwrap_or("query_result");
    match schema.filter(|s| !s.is_empty()) {
        Some(schema) => dialect.qualified_table(schema, table),
        None => dialect.quote_ident(table),
    }
}

/// Builds a full-table read; `columns` pins the column order to the cached catalog order.
pub(super) fn table_source_sql(
    target: &str,
    columns: Option<(SqlDialect, &[String])>,
    filter: &str,
) -> String {
    let list = columns.map_or_else(
        || "*".to_string(),
        |(dialect, names)| {
            names
                .iter()
                .map(|n| dialect.quote_ident(n))
                .collect::<Vec<_>>()
                .join(", ")
        },
    );
    let filter = filter.trim();
    let upper = filter.to_uppercase();
    if filter.is_empty() {
        format!("SELECT {list} FROM {target}")
    } else if upper.starts_with("WHERE ") || upper.starts_with("ORDER BY ") {
        format!("SELECT {list} FROM {target} {filter}")
    } else {
        format!("SELECT {list} FROM {target} WHERE {filter}")
    }
}

/// Keywords that can make a `SELECT`/`WITH` statement write (data-modifying CTEs,
/// `SELECT … INTO`) or that indicate a second statement. Locking reads such as
/// `FOR UPDATE` are rejected too, which is conservative.
const WRITE_KEYWORDS: [&str; 12] = [
    "INSERT", "UPDATE", "DELETE", "MERGE", "INTO", "TRUNCATE", "DROP", "ALTER", "CREATE", "GRANT",
    "REVOKE", "CALL",
];

/// Uppercased bare words outside quotes and comments, or `None` for a second statement.
fn bare_words(sql: &str) -> Option<Vec<String>> {
    let chars: Vec<char> = sql.chars().collect();
    let (mut words, mut word, mut i) = (Vec::new(), String::new(), 0);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let skip_to = |i: usize, end: &str| {
            let rest: String = chars[i..].iter().collect();
            rest.find(end).map_or(chars.len(), |p| {
                i + rest[..p].chars().count() + end.chars().count()
            })
        };
        let jump = match (c, next) {
            ('\'', _) => Some(skip_to(i + 1, "'")),
            ('"', _) => Some(skip_to(i + 1, "\"")),
            ('`', _) => Some(skip_to(i + 1, "`")),
            ('-', Some('-')) => Some(skip_to(i, "\n")),
            ('/', Some('*')) => Some(skip_to(i + 2, "*/")),
            (';', _) => return None,
            _ => None,
        };
        if c.is_alphanumeric() || c == '_' {
            word.extend(c.to_uppercase());
            i += 1;
            continue;
        }
        if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        i = jump.unwrap_or(i + 1);
    }
    if !word.is_empty() {
        words.push(word);
    }
    Some(words)
}

/// Re-running a source is only safe for read queries; mutations must not repeat.
pub(super) fn read_query(sql: Option<&str>) -> Option<String> {
    let sql = sql?.trim().trim_end_matches(';').trim();
    let words = bare_words(sql)?;
    let reads = matches!(words.first().map(String::as_str), Some("SELECT" | "WITH"));
    (reads && !words.iter().any(|w| WRITE_KEYWORDS.contains(&w.as_str()))).then(|| sql.to_string())
}

/// Keep any previous destination intact until the complete export is ready.
fn write_export(
    path: &Path,
    format: ExportFormat,
    pager: &QueryResultPager,
    rows: u64,
    dialect: SqlDialect,
    target: &str,
) -> anyhow::Result<u64> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
    let count = {
        let mut writer = BufWriter::new(temporary.as_file_mut());
        let count = match format {
            ExportFormat::Csv => export_to_csv_rows(pager, &mut writer, rows)?,
            ExportFormat::Json => json::write_json(pager, &mut writer, rows, dialect.0)?,

            ExportFormat::Html => {
                markup::Markup::Html.write(pager, &mut writer, rows, dialect.0)?
            }
            ExportFormat::Markdown => {
                markup::Markup::Markdown.write(pager, &mut writer, rows, dialect.0)?
            }
            // Single-row INSERTs also work with Oracle and older Db2 servers.
            ExportFormat::Sql => {
                export_to_sql_inserts_for_target(target, dialect, pager, &mut writer, 1, rows)?
            }
        };
        writer.flush()?;
        count
    };
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(count)
}

fn can_export_tab(tab: &TabState, query_running: bool) -> bool {
    !query_running
        && !tab.export.busy
        && !tab.grid.changeset.is_dirty()
        && tab.grid.editing_cell.is_none()
        && tab.grid.pager.as_ref().is_some_and(|p| p.num_cols() > 0)
}

impl Workspace {
    fn can_export(&self, index: usize) -> bool {
        self.tabs
            .get(index)
            .is_some_and(|tab| can_export_tab(tab, self.operations.is_running()))
    }

    pub(super) fn export_result(
        &mut self,
        index: usize,
        format: ExportFormat,
        cx: &mut Context<Self>,
    ) {
        if !self.can_export(index) {
            return;
        }
        let dialect = SqlDialect(self.tab_provider(index));
        let tab = &mut self.tabs[index];
        let id = tab.id;
        let pager = tab.grid.pager.clone().unwrap();
        let rows = pager.total_rows();
        let target = sql_target(
            dialect,
            tab.schema_name.as_deref(),
            tab.table_name.as_deref(),
        );
        let name = tab.table_name.as_deref().unwrap_or("query_result");
        let safe_name: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let name = format!("{safe_name}.{}", format.extension());
        tab.export.busy = true;
        tab.export.message = Some("Choose an export destination…".into());
        let dialog = cx.prompt_for_new_path(&platform_hal::paths::downloads_dir(), Some(&name));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = match dialog.await {
                Ok(Ok(Some(path))) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                            tab.export.message = Some(format!("Exporting {rows} loaded row(s)…"));
                            cx.notify();
                        }
                    });
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            write_export(&path, format, &pager, rows, dialect, &target)
                                .map(|count| {
                                    format!("Exported {count} row(s) to {}", path.display())
                                })
                                .map_err(|e| format!("Export failed: {e}"))
                        })
                        .await;
                    Some(result.unwrap_or_else(|message| message))
                }
                Ok(Ok(None)) => None,
                Ok(Err(error)) => Some(format!("Cannot open export dialog: {error}")),
                Err(_) => Some("Export dialog closed unexpectedly".into()),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                    tab.export.busy = false;
                    tab.export.message = outcome;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn export_all_result(&mut self, index: usize, format: ExportFormat, cx: &mut Context<Self>) {
        if !self.can_export(index) {
            return;
        }
        let Some(driver) = self.tab_session(index).driver.clone() else {
            return;
        };
        let dialect = SqlDialect(self.tab_provider(index));
        let tab = &mut self.tabs[index];
        let id = tab.id;
        let target = sql_target(
            dialect,
            tab.schema_name.as_deref(),
            tab.table_name.as_deref(),
        );
        let name = tab.table_name.as_deref().unwrap_or("query_result");
        let safe_name: String = name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let name = format!("{safe_name}_all.{}", format.extension());
        let full_format = match format {
            ExportFormat::Csv => db_wire::transfer::export::FullExportFormat::Csv,
            ExportFormat::Sql => db_wire::transfer::export::FullExportFormat::Sql,
            ExportFormat::Html | ExportFormat::Markdown | ExportFormat::Json => return,
        };
        let source_sql = if tab.schema_name.is_some() && tab.table_name.is_some() {
            table_source_sql(&target, None, &tab.filter_sql)
        } else if let Some(sql) = read_query(tab.script_sql.as_deref()) {
            // The active result's statement, not the whole editor buffer.
            sql
        } else {
            tab.export.message =
                Some("Full export requires a SELECT query to avoid repeating mutations".into());
            cx.notify();
            return;
        };
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let (progress_tx, mut progress_rx) = tokio::sync::watch::channel(0u64);
        tab.export.cancel = Some(cancel_tx);
        tab.export.busy = true;
        tab.export.message = Some("Choose a destination for full export…".into());
        let dialog = cx.prompt_for_new_path(&platform_hal::paths::downloads_dir(), Some(&name));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = match dialog.await {
                Ok(Ok(Some(path))) => {
                    let _ = this.update(cx, |this, cx| {
                        if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                            tab.export.message =
                                Some("Exporting all rows streaming to file…".into());
                            cx.notify();
                        }
                    });
                    let progress_task = cx.spawn({
                        let this = this.clone();
                        async move |cx| {
                            while progress_rx.changed().await.is_ok() {
                                let rows = *progress_rx.borrow();
                                let _ = this.update(cx, |this, cx| {
                                    if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id)
                                        && tab.export.cancel.as_ref().is_some_and(|c| !*c.borrow())
                                    {
                                        tab.export.message = Some(format!(
                                            "Exporting all rows… {} written",
                                            group_thousands(rows)
                                        ));
                                        cx.notify();
                                    }
                                });
                            }
                        }
                    });
                    let cancelled = cancel_rx.clone();
                    let settings = db_wire::transfer::export::FullExportSettings {
                        format: full_format,
                        csv_options: db_wire::transfer::csv::CsvOptions::default(),
                        target_table: target,
                        dialect,
                    };
                    let result = cx
                        .background_executor()
                        .spawn(async move {
                            db_wire::transfer::export::run_full_export(
                                driver,
                                source_sql,
                                &path,
                                settings,
                                cancel_rx,
                                progress_tx,
                            )
                            .await
                            .map(|count| {
                                format!("Exported all {count} row(s) to {}", path.display())
                            })
                            .map_err(|e| {
                                if *cancelled.borrow() {
                                    "Export cancelled; destination unchanged".to_string()
                                } else {
                                    format!("Full export failed: {e}")
                                }
                            })
                        })
                        .await;
                    drop(progress_task);
                    Some(result.unwrap_or_else(|message| message))
                }
                Ok(Ok(None)) => None,
                Ok(Err(error)) => Some(format!("Cannot open export dialog: {error}")),
                Err(_) => Some("Export dialog closed unexpectedly".into()),
            };
            let _ = this.update(cx, |this, cx| {
                if let Some(tab) = this.tabs.iter_mut().find(|t| t.id == id) {
                    tab.export.busy = false;
                    tab.export.cancel = None;
                    tab.export.message = outcome;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn cancel_full_export(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(cancel) = self.tabs.get(index).and_then(|t| t.export.cancel.as_ref()) {
            cancel.send_replace(true);
            self.tabs[index].export.message = Some("Cancelling export…".into());
            cx.notify();
        }
    }

    /// Opens transfer for the tab's table, or for the active result's read query.
    pub(super) fn open_result_transfer(&mut self, index: usize, cx: &mut Context<Self>) {
        if !self.can_export(index) {
            return;
        }
        let tab = &self.tabs[index];
        let conn_id = tab
            .connection_id
            .clone()
            .or_else(|| self.connections.active_connection_id.clone());
        if let (Some(conn_id), Some(schema), Some(table)) = (
            conn_id.clone(),
            tab.schema_name.clone(),
            tab.table_name.clone(),
        ) {
            let filter = tab.filter_sql.clone();
            if let Err(message) = self.open_table_transfer(&conn_id, &schema, &table, &filter, cx) {
                self.tabs[index].export.message = Some(message);
                cx.notify();
            }
            return;
        }
        let Some(sql) = read_query(tab.script_sql.as_deref()) else {
            self.tabs[index].export.message =
                Some("Transfer requires a SELECT query to avoid repeating mutations".into());
            cx.notify();
            return;
        };
        let Some(driver) = self.tab_session(index).driver.clone() else {
            return;
        };
        let columns = tab
            .grid
            .pager
            .as_ref()
            .map(|p| p.columns.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        let source = super::import_modal::QuerySource {
            driver,
            sql,
            label: format!("{} query result", conn_id.unwrap_or_default()),
            relation: None,
        };
        self.open_transfer_modal(source, columns, cx);
    }

    pub(super) fn render_export_toolbar(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let enabled = self.can_export(index);
        let theme = self.theme.clone();
        let mut toolbar = div().flex().flex_none().items_center().gap_1();
        if self.tabs[index].export.cancel.is_some() {
            let cancel_theme = self.theme.clone();
            return toolbar
                .child(
                    div()
                        .id("export-cancel")
                        .px_1()
                        .rounded_xs()
                        .cursor_pointer()
                        .hover(move |s| s.bg(cancel_theme.bg_surface_hover))
                        .child("Cancel export")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.cancel_full_export(index, cx);
                        })),
                )
                .child(div().child(self.tabs[index].export.message.clone().unwrap_or_default()));
        }
        for (label, format, is_all) in [
            ("Export CSV", ExportFormat::Csv, false),
            ("Export SQL", ExportFormat::Sql, false),
            ("Export JSON", ExportFormat::Json, false),
            ("Export HTML", ExportFormat::Html, false),
            ("Export Markdown", ExportFormat::Markdown, false),
            ("Export All (CSV)", ExportFormat::Csv, true),
            ("Export All (SQL)", ExportFormat::Sql, true),
        ] {
            let mut button = div().id(label).px_1().rounded_xs().child(label);
            if enabled {
                button = button
                    .cursor_pointer()
                    .hover(move |s| s.bg(theme.bg_surface_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if is_all {
                            this.export_all_result(index, format, cx);
                        } else {
                            this.export_result(index, format, cx);
                        }
                    }));
            } else {
                button = button.opacity(0.4);
            }
            toolbar = toolbar.child(button);
        }
        let transfer_theme = self.theme.clone();
        let mut transfer_btn = div()
            .id("toolbar-transfer")
            .px_1()
            .rounded_xs()
            .child("Transfer…");
        if enabled {
            transfer_btn = transfer_btn
                .cursor_pointer()
                .hover(move |s| s.bg(transfer_theme.bg_surface_hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.open_result_transfer(index, cx);
                }));
        } else {
            transfer_btn = transfer_btn.opacity(0.4);
        }
        toolbar = toolbar.child(transfer_btn);
        if let Some(table) = self.tabs[index].table_name.clone() {
            let schema = self.tabs[index]
                .schema_name
                .clone()
                .unwrap_or_else(|| "public".to_string());
            let conn_id = self.tabs[index]
                .connection_id
                .clone()
                .or_else(|| self.connections.active_connection_id.clone());
            let import_theme = self.theme.clone();
            let import_btn = div()
                .id("toolbar-import-csv")
                .px_1()
                .rounded_xs()
                .cursor_pointer()
                .hover(move |s| s.bg(import_theme.bg_surface_hover))
                .child("Import CSV…")
                .on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(ref cid) = conn_id {
                        this.open_import_modal(cid, &schema, &table, cx);
                    }
                }));
            toolbar = toolbar.child(import_btn);
        }
        toolbar.child(div().child(
            if self.tabs[index].grid.changeset.is_dirty()
                || self.tabs[index].grid.editing_cell.is_some()
            {
                "(save/cancel edits first)"
            } else {
                "(loaded or full source)"
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ExportFormat, can_export_tab, read_query, sql_target, table_source_sql, write_export,
    };
    use db_wire::{ColumnMeta, DbDriverKind, QueryResultPager, sql::SqlDialect};
    use mem_engine::DataType;

    #[test]
    fn export_requires_columns_idle_query_and_saved_grid() {
        let mut tab = crate::workspace::TabState::table(
            1,
            "main",
            "items",
            ui_core::theme::Theme::dark(),
            std::sync::Arc::new(sql_intel::SchemaCache::new().unwrap()),
        );
        assert!(!can_export_tab(&tab, false));
        tab.grid.pager = Some(std::sync::Arc::new(QueryResultPager::new(vec![]).unwrap()));
        assert!(!can_export_tab(&tab, false));
        tab.grid.pager = Some(std::sync::Arc::new(
            QueryResultPager::new(vec![ColumnMeta::new("id", DataType::Int4)]).unwrap(),
        ));
        assert!(can_export_tab(&tab, false)); // Empty results with columns can be exported.
        assert!(!can_export_tab(&tab, true));
        tab.export.busy = true;
        assert!(!can_export_tab(&tab, false));
        tab.export.busy = false;
        tab.grid.changeset.insert_row_at(0, vec![Some("1".into())]);
        assert!(!can_export_tab(&tab, false));
        tab.grid.changeset.clear();
        assert!(can_export_tab(&tab, false));
    }

    #[test]
    fn table_sources_pin_columns_and_apply_filters() {
        let pg = SqlDialect(DbDriverKind::Postgres);
        let cols = ["id".to_string(), "a\"b".to_string()];
        assert_eq!(table_source_sql("\"t\"", None, " "), "SELECT * FROM \"t\"");
        assert_eq!(
            table_source_sql("\"t\"", Some((pg, &cols)), "id > 1"),
            "SELECT \"id\", \"a\"\"b\" FROM \"t\" WHERE id > 1"
        );
        assert_eq!(
            table_source_sql("\"t\"", None, "order by id"),
            "SELECT * FROM \"t\" order by id"
        );
    }

    #[test]
    fn only_read_queries_can_be_rerun() {
        assert_eq!(read_query(Some(" select 1; ")).as_deref(), Some("select 1"));
        assert_eq!(
            read_query(Some("WITH x AS (SELECT 1) SELECT * FROM x")).as_deref(),
            Some("WITH x AS (SELECT 1) SELECT * FROM x")
        );
        assert_eq!(read_query(Some("DELETE FROM t")), None);
        assert_eq!(
            read_query(Some(
                "WITH d AS (DELETE FROM t RETURNING *) SELECT * FROM d"
            )),
            None
        );
        assert_eq!(read_query(Some("SELECT * INTO copy FROM t")), None);
        assert_eq!(read_query(Some("SELECT 1; DROP TABLE t")), None);
        assert_eq!(
            read_query(Some(
                "SELECT 'delete; into' AS \"update\" -- insert\nFROM t /* merge */"
            ))
            .as_deref(),
            Some("SELECT 'delete; into' AS \"update\" -- insert\nFROM t /* merge */")
        );
        assert_eq!(read_query(None), None);
    }

    #[test]
    fn targets_quote_schema_table_and_script_fallback() {
        let pg = SqlDialect(DbDriverKind::Postgres);
        assert_eq!(
            sql_target(pg, Some("a\"b"), Some("order")),
            "\"a\"\"b\".\"order\""
        );
        assert_eq!(sql_target(pg, None, None), "\"query_result\"");
        assert_eq!(
            sql_target(SqlDialect(DbDriverKind::MySql), Some("db"), Some("a`b")),
            "`db`.`a``b`"
        );
        assert_eq!(
            sql_target(
                SqlDialect(DbDriverKind::Sqlite),
                Some("main"),
                Some("items")
            ),
            "\"items\""
        );
    }

    #[test]
    fn json_export_preserves_native_values_and_rejects_duplicate_keys() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("result.json");
        let dialect = SqlDialect(DbDriverKind::Postgres);
        let pager = QueryResultPager::new(vec![
            ColumnMeta::new("exact", DataType::Numeric),
            ColumnMeta::new("float", DataType::Float8),
            ColumnMeta::new("active", DataType::Bool),
            ColumnMeta::new("text\"", DataType::Text),
            ColumnMeta::new("bytes", DataType::Bytea),
            ColumnMeta::new("document", DataType::Json),
            ColumnMeta::new("missing", DataType::Text),
        ])
        .unwrap();
        pager
            .append_row([
                Some("9007199254740993.000000000001"),
                Some("Infinity"),
                Some("true"),
                Some(""),
                Some(r"\x6162"),
                Some("null"),
                None,
            ])
            .unwrap();
        assert_eq!(
            write_export(&output, ExportFormat::Json, &pager, 1, dialect, "ignored").unwrap(),
            1
        );
        let json = std::fs::read_to_string(&output).unwrap();
        assert!(json.contains(r#""exact":9007199254740993.000000000001"#));
        let values: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(values[0]["active"], true);
        assert_eq!(values[0]["float"], "Infinity");
        assert_eq!(values[0]["text\""], "");
        assert_eq!(values[0]["bytes"], r"\x6162");
        assert_eq!(values[0]["document"], "null");
        assert!(values[0]["missing"].is_null());

        let empty_output = dir.path().join("empty.json");
        assert_eq!(
            write_export(
                &empty_output,
                ExportFormat::Json,
                &pager,
                0,
                dialect,
                "ignored"
            )
            .unwrap(),
            0
        );
        assert_eq!(std::fs::read_to_string(empty_output).unwrap(), "[]\n");

        let duplicate = QueryResultPager::new(vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("id", DataType::Text),
        ])
        .unwrap();
        let error = write_export(
            &output,
            ExportFormat::Json,
            &duplicate,
            0,
            dialect,
            "ignored",
        )
        .unwrap_err();
        assert!(error.to_string().contains("duplicate column name"));
        assert_eq!(std::fs::read_to_string(&output).unwrap(), json);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn markup_exports_escape_data_and_distinguish_null_empty_and_binary() {
        let dir = tempfile::tempdir().unwrap();
        let pager = QueryResultPager::new(vec![
            ColumnMeta::new("<header>|", DataType::Text),
            ColumnMeta::new("empty", DataType::Text),
            ColumnMeta::new("bytes", DataType::Bytea),
        ])
        .unwrap();
        pager
            .append_row([Some(b"<script>|*&\nnext".as_slice()), None, Some(b"ab")])
            .unwrap();
        pager
            .append_row([Some(b"NULL".as_slice()), Some(b""), Some(b"")])
            .unwrap();
        let dialect = SqlDialect(DbDriverKind::Sqlite);
        let html = dir.path().join("rows.html");
        let markdown = dir.path().join("rows.md");
        assert_eq!(
            write_export(&html, ExportFormat::Html, &pager, 1, dialect, "ignored").unwrap(),
            1
        );
        assert_eq!(
            write_export(
                &markdown,
                ExportFormat::Markdown,
                &pager,
                2,
                dialect,
                "ignored"
            )
            .unwrap(),
            2
        );
        let html = std::fs::read_to_string(html).unwrap();
        assert!(html.contains("<th>&lt;header&gt;|</th>"));
        assert!(html.contains("&lt;script&gt;|*&amp;\nnext"));
        assert!(html.contains("<td><em>NULL</em></td><td>\\x6162</td>"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<td>NULL</td>"));
        let markdown = std::fs::read_to_string(markdown).unwrap();
        assert_eq!(
            markdown,
            concat!(
                "| &#60;header&#62;&#124; | empty | bytes |\n",
                "| --- | --- | --- |\n",
                "| &#60;script&#62;&#124;&#42;&#38;<br>next | *NULL* | &#92;x6162 |\n",
                "| NULL |  | &#92;x |\n",
            )
        );
    }

    #[test]
    fn markup_failure_leaves_previous_destination_and_removes_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("result.html");
        std::fs::write(&path, "previous").unwrap();
        let pager = QueryResultPager::new(vec![ColumnMeta::new("bytes", DataType::Bytea)]).unwrap();
        pager.append_row([Some(r"\x6162")]).unwrap();
        pager.append_row([Some(r"\xZZ")]).unwrap();
        let error = write_export(
            &path,
            ExportFormat::Html,
            &pager,
            2,
            SqlDialect(DbDriverKind::Postgres),
            "ignored",
        )
        .unwrap_err();
        assert!(error.to_string().contains("hex digit"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn export_replaces_destination_only_on_success() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("result.csv");
        std::fs::write(&path, "previous").unwrap();
        let empty = QueryResultPager::new(vec![]).unwrap();
        let dialect = SqlDialect(DbDriverKind::Postgres);
        assert!(write_export(&path, ExportFormat::Sql, &empty, 0, dialect, "\"t\"").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "previous");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);

        let pager = QueryResultPager::new(vec![ColumnMeta::new("name", DataType::Text)]).unwrap();
        pager.append_row([Some("first")]).unwrap();
        pager.append_row([Some("second")]).unwrap();
        assert_eq!(
            write_export(&path, ExportFormat::Csv, &pager, 1, dialect, "\"t\"").unwrap(),
            1
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "name\nfirst\n");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(
            write_export(
                &dir.path().join("missing/out.csv"),
                ExportFormat::Csv,
                &pager,
                1,
                dialect,
                "\"t\""
            )
            .is_err()
        );
    }
}
