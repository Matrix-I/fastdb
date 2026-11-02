use super::*;
use db_wire::{QueryResultPager, export::*, sql::SqlDialect};
use std::io::{BufWriter, Write};
use std::path::Path;

#[derive(Default)]
pub(super) struct ExportState {
    pub busy: bool,
    pub message: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) enum ExportFormat {
    Csv,
    Sql,
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Sql => "sql",
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

    pub(super) fn render_export_toolbar(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let enabled = self.can_export(index);
        let theme = self.theme.clone();
        let mut toolbar = div().flex().flex_none().items_center().gap_1();
        for (label, format) in [
            ("Export CSV", ExportFormat::Csv),
            ("Export SQL", ExportFormat::Sql),
        ] {
            let mut button = div().id(label).px_1().rounded_xs().child(label);
            if enabled {
                button = button
                    .cursor_pointer()
                    .hover(move |s| s.bg(theme.bg_surface_hover))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.export_result(index, format, cx)),
                    );
            } else {
                button = button.opacity(0.4);
            }
            toolbar = toolbar.child(button);
        }
        toolbar.child(div().child(
            if self.tabs[index].grid.changeset.is_dirty()
                || self.tabs[index].grid.editing_cell.is_some()
            {
                "(save/cancel edits first)"
            } else {
                "(loaded rows)"
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{ExportFormat, can_export_tab, sql_target, write_export};
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
