//! Read-only, bounded snapshots of one loaded grid row.
use super::script_tools::tool;
use super::*;
use gpui::prelude::FluentBuilder;
use ui_grid::{Changeset, RowRef};

const PREVIEW_BYTES: usize = 512;
const FIELDS_PER_PAGE: usize = 64;

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
        tab.view_mode = DataViewMode::Record;
        tab.editor_focused = false;
        tab.record_view = Some(RecordView {
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
            return panel.child("Choose Record to inspect a loaded row.");
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
                tool("Back to grid").on_click(cx.listener(move |this, _, _, cx| {
                    if let Some(tab) = this
                        .tabs
                        .get_mut(this.active_tab)
                        .filter(|tab| tab.id == id)
                    {
                        tab.view_mode = DataViewMode::Grid;
                        let current = tab
                            .record_view
                            .take()
                            .is_some_and(|record| record.is_current(&tab.grid));
                        if current
                            && tab
                                .grid
                                .selection
                                .focus
                                .is_none_or(|focus| focus.row != row)
                            && let Ok(index) = usize::try_from(row)
                        {
                            tab.grid.auto_fetch_paused.set(true);
                            tab.grid.selection = ui_grid::GridSelection::single(row, first);
                            tab.grid
                                .vertical_scroll_handle
                                .scroll_to_item(index, ScrollStrategy::Center);
                        }
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
            return panel.child(div().p_3().child(
                "The result or staged edits changed. Refresh record or return to the grid.",
            ));
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
        panel.child(
            div()
                .id("record-fields")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(record.values.iter().enumerate().map(|(offset, field)| {
                    let column = &view.pager.columns[first + offset];
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .px_3()
                        .py_2()
                        .border_b_1()
                        .border_color(theme.border)
                        .child(div().text_color(theme.text_muted).truncate().child(format!(
                            "{} · {}",
                            column.name,
                            column.display_type()
                        )))
                        .child(div().whitespace_normal().child(field.label().to_owned()))
                })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Field, PREVIEW_BYTES, preview, project_row, read_record};
    use db_wire::{ColumnMeta, DataType};

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
