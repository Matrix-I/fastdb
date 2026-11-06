//! Read-only, bounded snapshots of one loaded grid row.
use super::script_tools::tool;
use super::*;
use gpui::prelude::FluentBuilder;
use ui_grid::{Changeset, RowRef};

const PREVIEW_BYTES: usize = 512;
const FIELDS_PER_PAGE: usize = 64;
const RECORD_NAME_WIDTH: f32 = 240.0;
const RECORD_ROW_HEIGHT: f32 = 28.0;

#[derive(Debug, PartialEq, Eq)]
enum Field {
    Text(String),
    Empty,
    Null,
    Default,
}

impl Field {
    fn label(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Empty => "(empty string)",
            Self::Null => "(SQL NULL)",
            Self::Default => "(unset · database default)",
        }
    }
}

fn preview(bytes: &[u8]) -> Field {
    if bytes.is_empty() {
        return Field::Empty;
    }
    let mut end = bytes.len().min(PREVIEW_BYTES);
    if end < bytes.len() {
        while end > 0 && bytes[end] & 0b1100_0000 == 0b1000_0000 {
            end -= 1;
        }
    }
    let text = String::from_utf8_lossy(&bytes[..end]);
    let mut clipped = String::new();
    for character in text.chars() {
        if character.is_control() {
            clipped.extend(character.escape_debug());
        } else {
            clipped.push(character);
        }
    }
    if clipped.len() > PREVIEW_BYTES {
        clipped.truncate(clipped.floor_char_boundary(PREVIEW_BYTES));
        clipped.push('…');
    } else if end < bytes.len() {
        clipped.push('…');
    }
    Field::Text(clipped)
}

struct RowProjection {
    fetched: Option<u64>,
    state: &'static str,
    values: Vec<Option<Field>>,
}

fn project_row(changes: &Changeset, row: u64, columns: std::ops::Range<usize>) -> RowProjection {
    match changes.resolve_row(row) {
        RowRef::Fetched(index) => RowProjection {
            fetched: Some(index),
            state: if changes.is_deleted(index) {
                "Marked for deletion"
            } else {
                "Loaded row"
            },
            values: columns
                .map(|col| {
                    changes
                        .edited_value(index, col)
                        .map(|value| value.map_or(Field::Null, |text| preview(text.as_bytes())))
                })
                .collect(),
        },
        RowRef::New(index) => RowProjection {
            fetched: None,
            state: "New row",
            values: columns
                .map(|col| {
                    if changes
                        .inserted_rows
                        .get(index)
                        .is_some_and(|row| row.explicit_nulls.contains(&col))
                    {
                        return Some(Field::Null);
                    }
                    Some(
                        changes
                            .inserted_rows
                            .get(index)
                            .and_then(|row| row.values.get(col))
                            .and_then(Option::as_deref)
                            .map_or(Field::Default, |text| preview(text.as_bytes())),
                    )
                })
                .collect(),
        },
    }
}

struct Record {
    state: &'static str,
    values: Vec<Field>,
}

fn read_record(
    pager: &db_wire::QueryResultPager,
    projection: RowProjection,
    first_col: usize,
) -> Result<Record, String> {
    let page = projection
        .fetched
        .map(|row| {
            let (id, local) = pager.locate_row(row).ok_or("Loaded row is unavailable")?;
            let page = pager.get_page(id).map_err(|error| error.to_string())?;
            if local >= page.row_count as usize {
                return Err("Loaded row is unavailable".into());
            }
            Ok::<_, String>((page, local))
        })
        .transpose()?;
    let values = projection
        .values
        .into_iter()
        .enumerate()
        .map(|(offset, staged)| {
            staged.unwrap_or_else(|| {
                page.as_ref()
                    .and_then(|(page, local)| page.get_cell(*local, first_col + offset))
                    .map_or(Field::Null, preview)
            })
        })
        .collect();
    Ok(Record {
        state: projection.state,
        values,
    })
}

pub(super) struct RecordView {
    pager: Arc<db_wire::QueryResultPager>,
    row: u64,
    first_col: usize,
    request: Arc<()>,
    changeset_revision: u64,
    result: Option<Result<Record, String>>,
}

impl RecordView {
    fn is_current(&self, grid: &VirtualDataGrid) -> bool {
        grid.pager
            .as_ref()
            .is_some_and(|pager| Arc::ptr_eq(pager, &self.pager))
            && grid.changeset_revision() == self.changeset_revision
            && self.row < grid.row_count()
    }
}

impl TabState {
    /// Leave single-row mode without changing the selected presentation.
    pub(super) fn leave_record_view(&mut self) {
        if let Some(view) = self.record_view.take()
            && view.is_current(&self.grid)
            && self
                .grid
                .selection
                .focus
                .is_none_or(|focus| focus.row != view.row)
            && let Ok(index) = usize::try_from(view.row)
        {
            self.grid.auto_fetch_paused.set(true);
            self.grid.selection = ui_grid::GridSelection::single(view.row, view.first_col);
            self.grid
                .vertical_scroll_handle
                .scroll_to_item(index, ScrollStrategy::Center);
        }
    }

    pub(super) fn record_text(&self) -> Option<String> {
        let view = self
            .record_view
            .as_ref()
            .filter(|view| view.is_current(&self.grid))?;
        let record = view.result.as_ref()?.as_ref().ok()?;
        let title = format!("Row #{}", view.row + 1);
        Some(
            super::data_panel::format_text_table(&["Name", &title], &record_rows(view, record))
                .join("\n"),
        )
    }
}

fn record_rows(view: &RecordView, record: &Record) -> Vec<Vec<String>> {
    record
        .values
        .iter()
        .enumerate()
        .map(|(offset, field)| {
            vec![
                preview(view.pager.columns[view.first_col + offset].name.as_bytes())
                    .label()
                    .to_owned(),
                field.label().to_owned(),
            ]
        })
        .collect()
}

fn record_grid(title: &str, rows: &[Vec<String>], theme: &Theme) -> gpui::Div {
    let table_row = |name: String, value: String, header: bool| {
        div()
            .flex()
            .flex_row()
            .flex_none()
            .h(px(RECORD_ROW_HEIGHT))
            .min_w(px(RECORD_NAME_WIDTH * 2.0))
            .border_b_1()
            .border_color(theme.border)
            .bg(if header {
                theme.bg_surface_selected
            } else {
                theme.bg_editor
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_shrink_0()
                    .w(px(RECORD_NAME_WIDTH))
                    .h_full()
                    .px_2()
                    .bg(theme.bg_surface)
                    .border_r_1()
                    .border_color(theme.border)
                    .child(div().truncate().child(name)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .px_2()
                    .child(div().truncate().child(value)),
            )
    };
    div()
        .flex()
        .flex_col()
        .min_w_full()
        .child(table_row("Name".into(), title.to_owned(), true))
        .children(
            rows.iter()
                .map(|row| table_row(row[0].clone(), row[1].clone(), false)),
        )
}

impl Workspace {
    pub(super) fn open_record_view(&mut self, tab_id: u64, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == tab_id) else {
            return;
        };
        let row = tab.grid.selection.focus.map_or(0, |cell| cell.row);
        self.load_record_view(tab_id, row, 0, cx);
    }

    fn load_record_view(
        &mut self,
        tab_id: u64,
        row: u64,
        first_col: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self
            .tabs
            .get_mut(self.active_tab)
            .filter(|tab| tab.id == tab_id)
        else {
            return;
        };
        if tab.grid.editing_cell.is_some() {
            tab.last_error =
                Some("Finish or cancel the cell editor before opening Record view".into());
            cx.notify();
            return;
        }
        if tab
            .record_view
            .as_ref()
            .is_some_and(|view| view.result.is_none())
        {
            return;
        }
        let Some(pager) = tab.grid.pager.clone() else {
            return;
        };
        if tab.grid.row_count() == 0 || first_col >= pager.columns.len() {
            return;
        }
        let row = row.min(tab.grid.row_count() - 1);
        let projection = project_row(
            &tab.grid.changeset,
            row,
            first_col..(first_col + FIELDS_PER_PAGE).min(pager.columns.len()),
        );
        let request = Arc::new(());
        tab.show_record(RecordView {
            pager: pager.clone(),
            row,
            first_col,
            request: request.clone(),
            changeset_revision: tab.grid.changeset_revision(),
            result: None,
        });
        self.input.is_filter_focused = false;
        self.blur_fetch_size();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { read_record(&pager, projection, first_col) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Some(view) = this
                    .tabs
                    .iter_mut()
                    .find(|tab| tab.id == tab_id)
                    .and_then(|tab| tab.record_view.as_mut())
                    && Arc::ptr_eq(&view.request, &request)
                {
                    view.result = Some(result);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn render_record_view(
        &self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = &self.theme;
        let tab = &self.tabs[index];
        let mut panel = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .bg(theme.bg_editor)
            .text_color(theme.text_primary)
            .text_size(px(12.0));
        let Some(view) = tab.record_view.as_ref() else {
            return panel.child("No loaded row to inspect.");
        };
        let id = tab.id;
        let row = view.row;
        let first = view.first_col;
        let columns = view.pager.columns.len();
        let rows = tab.grid.row_count();
        let current = view.is_current(&tab.grid);
        let ready = current && view.result.is_some();
        let toolbar = div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_2()
            .py_1()
            .bg(theme.bg_surface)
            .child(
                tool("All rows").on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(tab) = this
                        .tabs
                        .get_mut(this.active_tab)
                        .filter(|tab| tab.id == id)
                    {
                        tab.leave_record_view();
                        cx.notify();
                    }
                })),
            )
            .child(
                tool("Previous row")
                    .when(ready && row > 0, |button| {
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            this.load_record_view(id, row.saturating_sub(1), first, cx)
                        }))
                    })
                    .when(!ready || row == 0, |button| button.opacity(0.4)),
            )
            .child(
                tool("Next row")
                    .when(ready && row + 1 < rows, |button| {
                        button.on_click(cx.listener(move |this, _, _, cx| {
                            this.load_record_view(id, row + 1, first, cx)
                        }))
                    })
                    .when(!ready || row + 1 >= rows, |button| button.opacity(0.4)),
            )
            .child(
                tool("Refresh record").when(view.result.is_some(), |button| {
                    button.on_click(cx.listener(move |this, _, _, cx| {
                        this.load_record_view(id, row, if current { first } else { 0 }, cx)
                    }))
                }),
            )
            .child(format!("Loaded row {} of {rows}", row + 1));
        panel = panel.child(toolbar);
        if !current {
            return panel.child(
                div().p_3().child(
                    "The result or staged edits changed. Refresh record or choose All rows.",
                ),
            );
        }
        let Some(result) = &view.result else {
            return panel.child(div().p_3().child("Reading record…"));
        };
        let record = match result {
            Ok(record) => record,
            Err(error) => {
                return panel.child(
                    div()
                        .p_3()
                        .text_color(theme.text_error)
                        .child(error.clone()),
                );
            }
        };
        panel = panel.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap_2()
                .px_2()
                .py_1()
                .child(format!(
                    "{} · Fields {}–{} of {columns}",
                    record.state,
                    first + 1,
                    first + record.values.len()
                ))
                .children((first > 0).then(|| {
                    tool("Previous fields").on_click(cx.listener(move |this, _, _, cx| {
                        this.load_record_view(id, row, first.saturating_sub(FIELDS_PER_PAGE), cx)
                    }))
                }))
                .children((first + FIELDS_PER_PAGE < columns).then(|| {
                    tool("Next fields").on_click(cx.listener(move |this, _, _, cx| {
                        this.load_record_view(id, row, first + FIELDS_PER_PAGE, cx)
                    }))
                })),
        );
        let rows = record_rows(view, record);
        let title = format!("Row #{}", row + 1);
        let text_lines = super::data_panel::format_text_table(&["Name", &title], &rows);
        let text_copy = text_lines.join("\n");
        panel = panel.child(div().flex().flex_none().px_2().py_1().child(
            tool("Copy record").on_click(cx.listener(move |_, _, _, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text_copy.clone()));
            })),
        ));
        let body = match tab.view_mode {
            DataViewMode::Grid => record_grid(&title, &rows, theme).into_any_element(),
            DataViewMode::Text => {
                let width = text_lines
                    .iter()
                    .map(|line| line.chars().count())
                    .max()
                    .unwrap_or(0);
                super::data_panel::text_table_body(text_lines.join("\n"), width as f32 * 7.5 + 40.0)
                    .text_color(theme.text_primary)
                    .into_any_element()
            }
        };
        panel.child(
            div()
                .id("record-fields")
                .flex_1()
                .min_h_0()
                .min_w_0()
                .overflow_scroll()
                .child(body),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Field, PREVIEW_BYTES, preview, project_row, read_record};
    use db_wire::{ColumnMeta, DataType};

    #[test]
    fn opening_record_preserves_text_presentation() {
        use crate::workspace::{DataViewMode, TabState};
        use std::sync::Arc;
        let cache = Arc::new(sql_intel::SchemaCache::new().unwrap());
        let mut tab = TabState::table(1, "public", "example", ui_core::theme::Theme::dark(), cache);
        let pager = Arc::new(
            db_wire::QueryResultPager::new(vec![ColumnMeta::new("id", DataType::Int8)]).unwrap(),
        );
        pager.append_row([Some(b"7".as_slice())]).unwrap();
        tab.grid =
            ui_grid::VirtualDataGrid::new(Some(pager.clone()), ui_core::theme::Theme::dark());
        tab.view_mode = DataViewMode::Text;
        tab.show_record(super::RecordView {
            pager,
            row: 0,
            first_col: 0,
            request: Arc::new(()),
            changeset_revision: tab.grid.changeset_revision(),
            result: None,
        });
        assert_eq!(tab.view_mode, DataViewMode::Text);
    }

    fn sample_tab(script: bool) -> crate::workspace::TabState {
        use crate::workspace::{ScriptFile, TabState};
        use std::sync::Arc;
        let theme = ui_core::theme::Theme::dark();
        let cache = Arc::new(sql_intel::SchemaCache::new().unwrap());
        if script {
            let file = ScriptFile {
                path: "/tmp/record-presentation.sql".into(),
                connection_id: "test".into(),
                name: "record-presentation".into(),
                modified: std::time::SystemTime::UNIX_EPOCH,
                preview: String::new(),
            };
            TabState::script(1, &file, "select id, flag", theme, cache)
        } else {
            TabState::table(1, "public", "example", theme, cache)
        }
    }

    fn show_row(tab: &mut crate::workspace::TabState, row: u64, first_col: usize) {
        use std::sync::Arc;
        let pager = tab.grid.pager.as_ref().unwrap().clone();
        let projection = project_row(&tab.grid.changeset, row, first_col..pager.columns.len());
        let result = read_record(&pager, projection, first_col);
        tab.show_record(super::RecordView {
            pager,
            row,
            first_col,
            request: Arc::new(()),
            changeset_revision: tab.grid.changeset_revision(),
            result: Some(result),
        });
    }

    #[test]
    fn record_and_presentation_are_independent_for_table_and_sql_tabs() {
        use crate::workspace::DataViewMode;
        use std::sync::Arc;
        for script in [false, true] {
            for mode in [DataViewMode::Grid, DataViewMode::Text] {
                let mut tab = sample_tab(script);
                let pager = Arc::new(
                    db_wire::QueryResultPager::new(vec![ColumnMeta::new("id", DataType::Int8)])
                        .unwrap(),
                );
                pager.append_row([Some(b"7".as_slice())]).unwrap();
                pager.append_row([Some(b"8".as_slice())]).unwrap();
                tab.grid =
                    ui_grid::VirtualDataGrid::new(Some(pager), ui_core::theme::Theme::dark());
                tab.grid.selection = ui_grid::GridSelection::single(0, 0);
                tab.view_mode = mode;
                show_row(&mut tab, 0, 0);
                assert_eq!(tab.view_mode, mode);
                assert!(!tab.editor_focused);
                let request = tab.record_view.as_ref().unwrap().request.clone();
                tab.view_mode = DataViewMode::Text;
                assert!(Arc::ptr_eq(
                    &request,
                    &tab.record_view.as_ref().unwrap().request
                ));
                assert!(tab.record_text().unwrap().contains("7     |"));
                tab.view_mode = DataViewMode::Grid;
                assert!(Arc::ptr_eq(
                    &request,
                    &tab.record_view.as_ref().unwrap().request
                ));
                show_row(&mut tab, 1, 0);
                assert!(tab.record_text().unwrap().contains("8     |"));
                tab.view_mode = mode;
                tab.leave_record_view();
                assert!(tab.record_view.is_none());
                assert_eq!(tab.view_mode, mode);
                assert_eq!(tab.grid.selection.focus.unwrap().row, 1);
                assert!(tab.grid.auto_fetch_paused.get());
                show_row(&mut tab, 1, 0);
                tab.close_record_view();
                assert!(tab.record_view.is_none());
                assert_eq!(tab.view_mode, mode);
            }
        }
    }

    #[test]
    fn record_text_transposes_values_and_rejects_stale_pending_or_failed_snapshots() {
        use std::sync::Arc;
        let mut tab = sample_tab(false);
        let pager = Arc::new(
            db_wire::QueryResultPager::new(vec![
                ColumnMeta::new("id", DataType::Int8),
                ColumnMeta::new("flag", DataType::Bool),
            ])
            .unwrap(),
        );
        pager
            .append_row([Some(b"7".as_slice()), Some(b"false".as_slice())])
            .unwrap();
        tab.grid = ui_grid::VirtualDataGrid::new(Some(pager), ui_core::theme::Theme::dark());
        show_row(&mut tab, 0, 0);
        assert_eq!(
            tab.record_text().unwrap(),
            "Name|Row #1|\n----+------+\nid  |7     |\nflag|false |"
        );
        let view = tab.record_view.as_ref().unwrap();
        let record = view.result.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(
            super::record_rows(view, record),
            vec![vec!["id", "7"], vec!["flag", "false"]]
        );
        show_row(&mut tab, 0, 1);
        assert_eq!(
            tab.record_text().unwrap(),
            "Name|Row #1|\n----+------+\nflag|false |"
        );
        tab.record_view.as_mut().unwrap().result = None;
        assert!(tab.record_text().is_none());
        tab.record_view.as_mut().unwrap().result = Some(Err("read failed".into()));
        assert!(tab.record_text().is_none());
        show_row(&mut tab, 0, 0);
        tab.grid
            .stage(|changes| changes.set_cell(0, 0, Some(b"7".to_vec()), Some("9".into())));
        assert!(tab.record_text().is_none());
        tab.grid.selection = ui_grid::GridSelection::single(0, 1);
        tab.leave_record_view();
        assert_eq!(tab.grid.selection.focus.unwrap().col, 1);
    }

    #[test]
    fn record_preview_distinguishes_null_empty_default_and_staged_values() {
        let pager = db_wire::QueryResultPager::new(
            ["name", "notes", "nullable"]
                .map(|name| ColumnMeta::new(name, DataType::Text))
                .into(),
        )
        .unwrap();
        pager
            .append_row([Some(b"old".as_slice()), Some(b"".as_slice()), None])
            .unwrap();
        let mut changes = ui_grid::Changeset::new();
        changes.set_cell(0, 0, Some(b"old".to_vec()), Some("staged".into()));
        changes.insert_row_at(1, vec![Some("new".into()), None, None]);
        changes.deleted_rows.insert(0);
        let fetched = read_record(&pager, project_row(&changes, 0, 0..3), 0).unwrap();
        assert_eq!(fetched.state, "Marked for deletion");
        assert_eq!(
            fetched.values,
            [Field::Text("staged".into()), Field::Empty, Field::Null]
        );
        let added = read_record(&pager, project_row(&changes, 1, 0..3), 0).unwrap();
        assert_eq!(added.state, "New row");
        assert_eq!(
            added.values,
            [Field::Text("new".into()), Field::Default, Field::Default]
        );
    }

    #[test]
    fn record_snapshots_expire_when_staged_row_positions_change() {
        use std::sync::Arc;
        let pager = Arc::new(
            db_wire::QueryResultPager::new(vec![ColumnMeta::new("id", DataType::Int8)]).unwrap(),
        );
        pager.append_row([Some(b"1".as_slice())]).unwrap();
        let mut grid =
            ui_grid::VirtualDataGrid::new(Some(pager.clone()), ui_core::theme::Theme::dark());
        let record = super::RecordView {
            pager,
            row: 0,
            first_col: 0,
            request: Arc::new(()),
            changeset_revision: grid.changeset_revision(),
            result: None,
        };
        assert!(record.is_current(&grid));
        grid.stage(|changeset| changeset.insert_row_at(0, vec![Some("2".into())]));
        assert!(!record.is_current(&grid));
    }

    #[test]
    fn previews_are_bounded_and_invalid_fetched_rows_report_an_error() {
        let text = "ế".repeat(PREVIEW_BYTES);
        let Field::Text(preview) = preview(text.as_bytes()) else {
            panic!("expected text")
        };
        assert!(preview.ends_with('…'));
        assert!(preview.len() <= PREVIEW_BYTES + '…'.len_utf8());
        let pager =
            db_wire::QueryResultPager::new(vec![ColumnMeta::new("id", DataType::Int8)]).unwrap();
        assert!(read_record(&pager, project_row(&ui_grid::Changeset::new(), 1, 0..1), 0).is_err());
    }
}
