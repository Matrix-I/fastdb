use super::*;
use db_wire::{export::write_csv_field, sql::SqlDialect};
use std::io::{self, Write};
use ui_grid::{Changeset, GridSelection, resolve_cell};

const CLIPBOARD_LIMIT: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub(super) enum CopyFormat {
    Tsv,
    Csv,
    Sql,
}

impl CopyFormat {
    fn label(self) -> &'static str {
        match self {
            Self::Tsv => "TSV",
            Self::Csv => "CSV",
            Self::Sql => "SQL INSERT",
        }
    }
}

#[derive(Clone)]
pub(super) struct GridContextMenu {
    tab_id: u64,
    pager: Arc<db_wire::QueryResultPager>,
    position: Point<Pixels>,
    cell: ui_grid::GridCoord,
    revision: u64,
}

struct SelectionSnapshot {
    pager: Arc<db_wire::QueryResultPager>,
    changeset: Changeset,
    rows: std::ops::RangeInclusive<u64>,
    columns: Vec<usize>,
}

impl SelectionSnapshot {
    fn capture(grid: &VirtualDataGrid) -> Option<Self> {
        let (first, last, columns) = grid.selection_display_range()?;
        Some(Self {
            rows: first..=last,
            columns: grid.display_columns()[columns].to_vec(),
            pager: grid.pager.clone()?,
            changeset: grid.changeset.clone(),
        })
    }

    fn serialize(
        &self,
        format: CopyFormat,
        dialect: SqlDialect,
        target: &str,
        limit: usize,
    ) -> anyhow::Result<String> {
        let mut output = ClipboardWriter {
            bytes: Vec::new(),
            limit,
        };
        for row in self.rows.clone() {
            let columns = if matches!(format, CopyFormat::Sql)
                && let ui_grid::RowRef::New(index) = self.changeset.resolve_row(row)
            {
                let inserted = &self.changeset.inserted_rows[index];
                std::borrow::Cow::Owned(
                    self.columns
                        .iter()
                        .copied()
                        .filter(|&column| {
                            inserted.values.get(column).is_some_and(Option::is_some)
                                || inserted.explicit_nulls.contains(&column)
                        })
                        .collect::<Vec<_>>(),
                )
            } else {
                std::borrow::Cow::Borrowed(self.columns.as_slice())
            };
            if columns.is_empty() {
                writeln!(output, "{}", dialect.default_insert(target)?)?;
                continue;
            }
            if matches!(format, CopyFormat::Sql) {
                write!(output, "INSERT INTO {target} (")?;
                for (index, &col) in columns.iter().enumerate() {
                    if index > 0 {
                        output.write_all(b", ")?;
                    }
                    write!(
                        output,
                        "{}",
                        dialect.quote_ident(&self.pager.columns[col].name)
                    )?;
                }
                output.write_all(b") VALUES (")?;
            }
            for (index, &col) in columns.iter().enumerate() {
                if index > 0 {
                    output.write_all(match format {
                        CopyFormat::Tsv => b"\t",
                        CopyFormat::Csv => b",",
                        CopyFormat::Sql => b", ",
                    })?;
                }
                let value = resolve_cell(&self.pager, &self.changeset, row, col);
                match format {
                    CopyFormat::Tsv => {
                        output.write_all(value.as_deref().unwrap_or_default().as_bytes())?
                    }
                    CopyFormat::Csv => write_csv_field(
                        &mut output,
                        value.as_deref().unwrap_or_default().as_bytes(),
                    )?,
                    CopyFormat::Sql => write!(output, "{}", dialect.literal(value.as_deref()))?,
                }
            }
            match format {
                CopyFormat::Sql => output.write_all(b");\n")?,
                CopyFormat::Csv => output.write_all(b"\n")?,
                CopyFormat::Tsv if row < *self.rows.end() => output.write_all(b"\n")?,
                CopyFormat::Tsv => {}
            }
        }
        Ok(String::from_utf8(output.bytes)?)
    }
}

// A clipboard needs a complete in-memory string. Stop before growing it beyond
// the budget; never truncate CSV fields or SQL and never replace it on failure.
struct ClipboardWriter {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for ClipboardWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(
                "selection exceeds the clipboard size limit; select fewer cells or use Export CSV / SQL",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Workspace {
    pub(super) fn open_grid_context_menu(
        &mut self,
        index: usize,
        row: u64,
        col: usize,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.commit_cell_edit();
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let Some(pager) = tab.grid.pager.clone() else {
            return;
        };
        if row >= tab.grid.row_count() || col >= pager.columns.len() {
            return;
        }
        let selected = tab
            .grid
            .selection_display_range()
            .is_some_and(|(first, last, columns)| {
                (first..=last).contains(&row) && tab.grid.display_columns()[columns].contains(&col)
            });
        if !selected {
            tab.grid.selection = GridSelection::single(row, col);
        }
        tab.editor_focused = false;
        self.input.is_filter_focused = false;
        self.input.is_fetch_size_focused = false;
        self.menus.nav_context_menu = None;
        self.menus.tab_context_menu = None;
        self.menus.column_menu = None;
        self.menus.grid_context_menu = Some(GridContextMenu {
            tab_id: tab.id,
            pager,
            position,
            cell: ui_grid::GridCoord { row, col },
            revision: tab.grid.changeset_revision(),
        });
        cx.notify();
    }

    pub(super) fn copy_grid_selection(
        &mut self,
        index: usize,
        format: CopyFormat,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        if tab.grid.editing_cell.is_some() {
            return;
        }
        let Some(snapshot) = SelectionSnapshot::capture(&tab.grid) else {
            return;
        };
        let dialect = SqlDialect(self.tab_provider(index));
        let target = super::export::sql_target(
            dialect,
            tab.schema_name.as_deref(),
            tab.table_name.as_deref(),
        );
        let tab_id = tab.id;
        let pager = snapshot.pager.clone();
        let rows = snapshot.rows.end() - snapshot.rows.start() + 1;
        self.clipboard_request_id += 1;
        let request = self.clipboard_request_id;
        self.tabs[index].clipboard_request_id = request;
        self.tabs[index].export.message = Some(format!("Copying selection as {}…", format.label()));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { snapshot.serialize(format, dialect, &target, CLIPBOARD_LIMIT) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(tab) = this.tabs.iter_mut().find(|t| t.id == tab_id) else {
                    return;
                };
                if tab.clipboard_request_id != request {
                    return;
                }
                let message = if this.clipboard_request_id != request {
                    "Copy superseded by a newer request".into()
                } else if !tab
                    .grid
                    .pager
                    .as_ref()
                    .is_some_and(|p| Arc::ptr_eq(p, &pager))
                {
                    "Copy cancelled: result changed".into()
                } else {
                    match result {
                        Ok(text) => {
                            cx.write_to_clipboard(ClipboardItem::new_string(text));
                            format!("Copied {rows} row(s) as {}", format.label())
                        }
                        Err(error) => format!("Copy failed: {error}"),
                    }
                };
                tab.export.message = Some(message);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn render_grid_context_menu(
        &self,
        menu: GridContextMenu,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let index = self.tabs.iter().position(|t| t.id == menu.tab_id);
        let enabled = index.is_some_and(|i| {
            let grid = &self.tabs[i].grid;
            grid.editing_cell.is_none()
                && grid.selection_display_range().is_some()
                && grid
                    .pager
                    .as_ref()
                    .is_some_and(|p| Arc::ptr_eq(p, &menu.pager))
        });
        let theme = self.theme.clone();
        let mut panel = div()
            .id("grid-context-menu")
            .occlude()
            .flex()
            .flex_col()
            .w(px(200.0))
            .p_1()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.menus.grid_context_menu = None;
                cx.notify();
            }));
        for (label, format) in [
            ("Copy (TSV)", CopyFormat::Tsv),
            ("Copy as CSV", CopyFormat::Csv),
            ("Copy as SQL INSERT", CopyFormat::Sql),
        ] {
            let mut item = div()
                .id(label)
                .h(px(26.0))
                .px_2()
                .rounded_sm()
                .text_size(px(12.0))
                .text_color(if enabled {
                    theme.text_primary
                } else {
                    theme.text_muted
                })
                .child(label);
            if enabled {
                let pager = menu.pager.clone();
                let tab_id = menu.tab_id;
                item = item
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.bg_surface_selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menus.grid_context_menu = None;
                        if let Some(i) = this.tabs.iter().position(|t| t.id == tab_id)
                            && this.tabs[i]
                                .grid
                                .pager
                                .as_ref()
                                .is_some_and(|p| Arc::ptr_eq(p, &pager))
                        {
                            this.copy_grid_selection(i, format, cx);
                        }
                        cx.notify();
                    }));
            }
            panel = panel.child(item);
        }
        if enabled && index == Some(self.active_tab) && self.can_edit_active_rows() {
            let new_row = matches!(
                self.tabs[self.active_tab]
                    .grid
                    .changeset
                    .resolve_row(menu.cell.row),
                ui_grid::RowRef::New(_)
            );
            for (label, default) in [("Set SQL NULL", false), ("Use database default", true)] {
                if default && !new_row {
                    continue;
                }
                let menu = menu.clone();
                panel = panel.child(super::script_tools::tool(label).on_click(cx.listener(
                    move |this, _, _, cx| {
                        this.menus.grid_context_menu = None;
                        if this.tabs.get(this.active_tab).is_some_and(|tab| {
                            tab.id == menu.tab_id
                                && tab.grid.changeset_revision() == menu.revision
                                && tab
                                    .grid
                                    .pager
                                    .as_ref()
                                    .is_some_and(|pager| Arc::ptr_eq(pager, &menu.pager))
                        }) {
                            this.set_grid_empty_value(menu.cell, default, cx);
                        }
                        cx.notify();
                    },
                )));
            }
        }
        if enabled && let Some(index) = index.filter(|&index| index == self.active_tab) {
            for (index, reference) in self
                .grid_references(index, menu.cell)
                .into_iter()
                .take(8)
                .enumerate()
            {
                if reference.can_choose() && self.can_edit_active_rows() {
                    let picker_menu = menu.clone();
                    let picker_reference = reference.clone();
                    panel = panel.child(
                        div()
                            .id(("grid-key-lookup", index))
                            .px_2()
                            .py_1()
                            .text_size(px(12.0))
                            .text_color(theme.text_primary)
                            .cursor_pointer()
                            .child(format!("Choose key ({})", reference.name))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.menus.grid_context_menu = None;
                                if this.tabs.get(this.active_tab).is_some_and(|tab| {
                                    tab.id == picker_menu.tab_id
                                        && tab.grid.changeset_revision() == picker_menu.revision
                                        && tab.grid.pager.as_ref().is_some_and(|pager| {
                                            Arc::ptr_eq(pager, &picker_menu.pager)
                                        })
                                }) {
                                    this.choose_grid_reference(
                                        picker_menu.cell,
                                        picker_reference.clone(),
                                        cx,
                                    );
                                }
                                cx.notify();
                            })),
                    );
                }
                let menu = menu.clone();
                panel = panel.child(
                    div()
                        .id(("grid-reference", index))
                        .px_2()
                        .py_1()
                        .text_size(px(12.0))
                        .text_color(theme.text_primary)
                        .cursor_pointer()
                        .child(reference.label())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.menus.grid_context_menu = None;
                            if this.tabs.get(this.active_tab).is_some_and(|tab| {
                                tab.id == menu.tab_id
                                    && tab.grid.changeset_revision() == menu.revision
                                    && tab
                                        .grid
                                        .pager
                                        .as_ref()
                                        .is_some_and(|pager| Arc::ptr_eq(pager, &menu.pager))
                            }) {
                                this.open_grid_reference(menu.cell, reference.clone(), cx);
                            }
                            cx.notify();
                        })),
                );
            }
        }
        deferred(
            anchored()
                .position(menu.position)
                .snap_to_window()
                .child(panel),
        )
        .with_priority(1)
    }
}

#[cfg(test)]
mod tests {
    use super::{CLIPBOARD_LIMIT, ClipboardWriter, CopyFormat, SelectionSnapshot};
    use db_wire::{ColumnMeta, DbDriverKind, QueryResultPager, sql::SqlDialect};
    use mem_engine::DataType;
    use std::{io::Write, sync::Arc};
    use ui_core::theme::Theme;
    use ui_grid::{GridCoord, GridSelection, VirtualDataGrid};

    fn grid() -> VirtualDataGrid {
        let pager = Arc::new(
            QueryResultPager::new(vec![
                ColumnMeta::new("excluded", DataType::Text),
                ColumnMeta::new("a\"`]", DataType::Text),
                ColumnMeta::new("nullable", DataType::Text),
            ])
            .unwrap(),
        );
        pager
            .append_row([
                Some("ignore"),
                Some("O'Reilly\\path,\"quoted\"\r\nTiếng Việt"),
                None,
            ])
            .unwrap();
        pager
            .append_row([Some("ignore too"), Some(""), Some("NULL")])
            .unwrap();
        let mut grid = VirtualDataGrid::new(Some(pager), Theme::dark());
        // A reversed range must copy in visual row/column order.
        grid.selection
            .set_range(GridCoord { row: 1, col: 2 }, GridCoord { row: 0, col: 1 });
        grid
    }

    #[test]
    fn csv_quotes_selected_values_without_headers_and_preserves_tsv_behavior() {
        let grid = grid();
        let snapshot = SelectionSnapshot::capture(&grid).unwrap();
        let dialect = SqlDialect(DbDriverKind::Postgres);
        assert_eq!(
            snapshot
                .serialize(CopyFormat::Csv, dialect, "unused", CLIPBOARD_LIMIT)
                .unwrap(),
            "\"O'Reilly\\path,\"\"quoted\"\"\r\nTiếng Việt\",\n,NULL\n"
        );
        assert_eq!(
            snapshot
                .serialize(CopyFormat::Tsv, dialect, "unused", CLIPBOARD_LIMIT)
                .unwrap(),
            grid.selection_text().unwrap()
        );
    }

    #[test]
    fn sql_uses_selected_columns_and_provider_literals_and_identifiers() {
        let snapshot = SelectionSnapshot::capture(&grid()).unwrap();
        for provider in [
            DbDriverKind::Postgres,
            DbDriverKind::MySql,
            DbDriverKind::MariaDb,
            DbDriverKind::Sqlite,
            DbDriverKind::SqlServer,
            DbDriverKind::Oracle,
            DbDriverKind::Db2,
        ] {
            let dialect = SqlDialect(provider);
            let target = dialect.qualified_table("sch\"`", "order\"`]");
            let sql = snapshot
                .serialize(CopyFormat::Sql, dialect, &target, CLIPBOARD_LIMIT)
                .unwrap();
            assert_eq!(sql.matches("INSERT INTO").count(), 2);
            assert!(!sql.contains("excluded"));
            assert!(sql.contains("O''Reilly"));
            assert!(sql.contains(", NULL);\n"));
            assert!(sql.ends_with(if provider == DbDriverKind::SqlServer {
                "VALUES (N'', N'NULL');\n"
            } else {
                "VALUES ('', 'NULL');\n"
            }));
            match provider {
                DbDriverKind::MySql | DbDriverKind::MariaDb => {
                    assert!(sql.starts_with("INSERT INTO `sch\"```"));
                    assert!(sql.contains("O''Reilly\\\\path"));
                }
                DbDriverKind::SqlServer => {
                    assert!(sql.contains("([a\"`]]], [nullable]) VALUES (N'"))
                }
                _ => assert!(sql.contains("(\"a\"\"`]\", \"nullable\") VALUES ('")),
            }
        }
    }

    #[test]
    fn snapshot_includes_staged_edits_insertions_and_nulls_and_ignores_later_changes() {
        let mut grid = grid();
        grid.changeset.set_cell(0, 1, None, Some("edited".into()));
        grid.changeset.set_cell(1, 2, None, None);
        grid.changeset.insert_row_at(
            1,
            vec![
                Some("skip".into()),
                Some("inserted".into()),
                Some("new".into()),
            ],
        );
        grid.selection
            .set_range(GridCoord { row: 0, col: 1 }, GridCoord { row: 2, col: 2 });
        let snapshot = SelectionSnapshot::capture(&grid).unwrap();
        grid.changeset.clear();
        grid.pager
            .as_ref()
            .unwrap()
            .append_row([Some("later"), Some("later"), Some("later")])
            .unwrap();
        assert_eq!(
            snapshot
                .serialize(
                    CopyFormat::Csv,
                    SqlDialect(DbDriverKind::Postgres),
                    "unused",
                    CLIPBOARD_LIMIT
                )
                .unwrap(),
            "edited,\ninserted,new\n,\n"
        );
    }

    #[test]
    fn empty_and_stale_ranges_are_rejected_and_endpoints_are_clamped() {
        let mut grid = grid();
        grid.selection.clear();
        assert!(SelectionSnapshot::capture(&grid).is_none());
        grid.selection = GridSelection::single(u64::MAX, usize::MAX);
        assert!(SelectionSnapshot::capture(&grid).is_none());
        grid.selection.set_range(
            GridCoord { row: 0, col: 1 },
            GridCoord {
                row: u64::MAX,
                col: usize::MAX,
            },
        );
        assert!(SelectionSnapshot::capture(&grid).is_none());
        grid.selection.focus.as_mut().unwrap().col = 2;
        let snapshot = SelectionSnapshot::capture(&grid).unwrap();
        assert_eq!(snapshot.rows, 0..=1);
        assert_eq!(snapshot.columns, [1, 2]);
        grid.set_pager(Arc::new(QueryResultPager::new(vec![]).unwrap()));
        grid.selection = GridSelection::single(0, 0);
        assert!(SelectionSnapshot::capture(&grid).is_none());
    }

    #[test]
    fn clipboard_budget_fails_atomically_instead_of_truncating_output() {
        let snapshot = SelectionSnapshot::capture(&grid()).unwrap();
        for format in [CopyFormat::Csv, CopyFormat::Sql, CopyFormat::Tsv] {
            assert!(
                snapshot
                    .serialize(format, SqlDialect(DbDriverKind::Postgres), "\"t\"", 5)
                    .unwrap_err()
                    .to_string()
                    .contains("size limit")
            );
        }
        let mut writer = ClipboardWriter {
            bytes: Vec::new(),
            limit: 3,
        };
        writer.write_all(b"abc").unwrap();
        assert!(writer.write_all(b"d").is_err());
        assert_eq!(writer.bytes, b"abc");
    }

    #[test]
    fn sqlite_selected_insert_replays_null_empty_and_unicode_text() {
        use db_wire::{ConnectionConfig, DatabaseDriver, SqliteDriver};
        let dir = tempfile::tempdir().unwrap();
        let driver = SqliteDriver::new(ConnectionConfig {
            driver: DbDriverKind::Sqlite,
            database: dir.path().join("clipboard.sqlite").to_string_lossy().into(),
            ..Default::default()
        })
        .unwrap();
        let sql = SelectionSnapshot::capture(&grid())
            .unwrap()
            .serialize(
                CopyFormat::Sql,
                SqlDialect(DbDriverKind::Sqlite),
                "\"copy_target\"",
                CLIPBOARD_LIMIT,
            )
            .unwrap();
        db_wire::get_runtime().block_on(async {
            driver
                .execute_query("CREATE TABLE copy_target (\"a\"\"`]\" TEXT, nullable TEXT)")
                .await
                .unwrap();
            // The fixture contains a newline, but no semicolon inside a value.
            for statement in sql.split(';').filter(|s| !s.trim().is_empty()) {
                driver.execute_query(statement).await.unwrap();
            }
            let (pager, count, _) = driver
                .execute_query("SELECT * FROM copy_target ORDER BY rowid")
                .await
                .unwrap();
            assert_eq!(count, 2);
            assert_eq!(
                pager.get_cell(0, 0).unwrap().as_slice(),
                b"O'Reilly\\path,\"quoted\"\r\nTi\xe1\xba\xbfng Vi\xe1\xbb\x87t"
            );
            assert!(pager.get_cell(0, 1).is_none());
            assert_eq!(pager.get_cell(1, 0).unwrap().as_slice(), b"");
            assert_eq!(pager.get_cell(1, 1).unwrap().as_slice(), b"NULL");
        });
    }
}
