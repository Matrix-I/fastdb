use super::value_editor::{ValueRead, read_value};
use super::value_format::{FormattedValue, ValueFormat, detect_format, format_value};
use super::*;
use gpui::prelude::FluentBuilder;
use std::sync::atomic::{AtomicBool, Ordering};
use ui_grid::RowRef;

/// Default width of the record value side panel
pub(super) const VALUE_PANEL_WIDTH: f32 = 340.0;
/// Lines past this are not rendered, so a huge value cannot stall the frame
const MAX_RENDERED_LINES: usize = 5_000;
/// Characters past this on a single line are not rendered
const MAX_LINE_CHARS: usize = 4_000;
const MONO_FONT: &str = ".AppleSystemUIFontMonospaced, Menlo, monospace";
const ACCENT: u32 = 0x388bfd;

/// View options of the Value panel, kept across cell selections
pub struct ValuePanelState {
    /// Format the user picked from the menu; `None` follows the detected format
    pub format_override: Option<ValueFormat>,
    pub word_wrap: bool,
    pub(super) file_encoding: super::value_files::Encoding,
    pub auto_format: bool,
    pub(super) utc_timestamps: bool,
    pub show_format_menu: bool,
    request: Option<PreviewKey>,
    preview: Option<Arc<CellValue>>,
    job: Arc<AtomicBool>,
}

impl Default for ValuePanelState {
    fn default() -> Self {
        Self {
            format_override: None,
            word_wrap: false,
            file_encoding: Default::default(),
            auto_format: true,
            utc_timestamps: false,
            show_format_menu: false,
            request: None,
            preview: None,
            job: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct PreviewKey {
    tab: u64,
    pager: usize,
    row: u64,
    column: usize,
    revision: u64,
    format: Option<ValueFormat>,
    auto_format: bool,
    utc: bool,
    default: bool,
}

struct CellValue {
    column: String,
    type_name: String,
    row: u64,
    bytes: Option<Vec<u8>>,
    byte_len: Option<usize>,
    format: ValueFormat,
    formatted: Option<Arc<FormattedValue>>,
    copy_text: Option<Arc<String>>,
    error: Option<String>,
    default: bool,
}

struct PreviewJob(Arc<AtomicBool>);
impl Drop for PreviewJob {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn preview_value(read: ValueRead, key: &PreviewKey, name: String, type_name: String) -> CellValue {
    let data_type = read.pager.columns[read.column].data_type;
    let mut cell = CellValue {
        column: name,
        type_name,
        row: key.row,
        bytes: None,
        byte_len: None,
        format: ValueFormat::Text,
        formatted: None,
        copy_text: None,
        error: None,
        default: key.default,
    };
    match read_value(read) {
        Err(error) => cell.error = Some(error),
        Ok((value, _)) => {
            cell.bytes = value.map(String::into_bytes);
            if let Some(bytes) = &cell.bytes {
                cell.byte_len = Some(if data_type == DataType::Bytea {
                    bytes.len().saturating_sub(2) / 2
                } else {
                    bytes.len()
                });
                cell.format = key.format.unwrap_or_else(|| {
                    if data_type == DataType::Bytea {
                        ValueFormat::Binary
                    } else {
                        detect_format(bytes, data_type)
                    }
                });
                let text = std::str::from_utf8(bytes).unwrap_or("");
                // Pretty printing is presentation only and must keep original JSON tokens.
                let result = if key.utc && data_type == DataType::TimestampTz {
                    super::value_files::timestamp_display(text, true)
                } else if key.auto_format && cell.format == ValueFormat::Json {
                    super::value_files::format_json(text)
                } else {
                    Ok(text.to_owned())
                };
                let mut formatted = match result {
                    Ok(text) => format_value(text.as_bytes(), data_type, cell.format, false),
                    Err(error) => {
                        let mut value = format_value(bytes, data_type, cell.format, false);
                        value.error = Some(error);
                        value
                    }
                };
                if key.auto_format && cell.format == ValueFormat::Xml {
                    formatted.error = Some(
                        "XML source shown unchanged to preserve significant whitespace".into(),
                    );
                }
                cell.copy_text = Some(Arc::new(formatted.lines.join("\n")));
                cell.formatted = Some(Arc::new(formatted));
            }
        }
    }
    cell
}

impl Workspace {
    /// Click on a grid cell: selects it, or extends the selection with Shift
    pub(super) fn select_grid_cell(
        &mut self,
        row: u64,
        col: usize,
        extend: bool,
        cx: &mut Context<Self>,
    ) {
        if self
            .tabs
            .get(self.active_tab)
            .and_then(|t| t.grid.editing_cell.as_ref())
            .is_some_and(|e| (e.row, e.col) != (row, col))
        {
            self.commit_cell_edit();
        }
        self.navigator_filter.field_focused = false;
        self.navigator_filter.menu_open = false;
        self.input.is_filter_focused = false;
        self.blur_fetch_size();
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        tab.editor_focused = false;
        let coord = ui_grid::GridCoord { row, col };
        let prev_col = tab.grid.selection.focus.map(|f| f.col);
        match tab.grid.selection.anchor {
            Some(anchor) if extend => tab.grid.selection.set_range(anchor, coord),
            _ => tab.grid.selection = ui_grid::GridSelection::single(row, col),
        }
        if prev_col != Some(col) {
            self.value_panel.format_override = None;
        }
        self.value_panel.show_format_menu = false;
        cx.notify();
    }

    fn focused_cell_value(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Option<Arc<CellValue>> {
        let tab = self.tabs.get(index)?;
        let grid = &tab.grid;
        let focus = grid.selection.focus?;
        let pager = grid.pager.as_ref()?;
        let column = pager.columns.get(focus.col)?;
        if focus.row >= grid.row_count() {
            return None;
        }
        let key = PreviewKey {
            tab: tab.id,
            pager: Arc::as_ptr(pager) as usize,
            row: focus.row,
            column: focus.col,
            revision: grid.changeset_revision(),
            format: self.value_panel.format_override,
            auto_format: self.value_panel.auto_format,
            utc: self.value_panel.utc_timestamps,
            default: match grid.changeset.resolve_row(focus.row) {
                RowRef::New(index) => {
                    let row = &grid.changeset.inserted_rows[index];
                    row.values[focus.col].is_none() && !row.explicit_nulls.contains(&focus.col)
                }
                _ => false,
            },
        };
        if self.value_panel.request.as_ref() == Some(&key) {
            return self.value_panel.preview.clone();
        }
        self.value_panel.preview = None;
        if self
            .value_panel
            .job
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return None;
        }
        let job = PreviewJob(self.value_panel.job.clone());
        let row = grid.changeset.resolve_row(focus.row);
        let staged = match row {
            RowRef::Fetched(row) => grid.changeset.edited_value(row, focus.col),
            RowRef::New(index) => {
                Some(grid.changeset.inserted_rows[index].values[focus.col].as_deref())
            }
        };
        // Check staged bytes before cloning on the UI thread.
        if staged
            .flatten()
            .is_some_and(|text| text.len() > super::value_files::MAX_VALUE_BYTES * 2 + 2)
        {
            self.value_panel.request = Some(key.clone());
            let cell = CellValue {
                column: column.name.clone(),
                type_name: column.display_type(),
                row: focus.row,
                bytes: None,
                byte_len: None,
                format: ValueFormat::Text,
                formatted: None,
                copy_text: None,
                error: Some("Staged value exceeds the 256 KiB viewer limit".into()),
                default: key.default,
            };
            self.value_panel.preview = Some(Arc::new(cell));
            drop(job);
            return self.value_panel.preview.clone();
        }
        let read = ValueRead {
            pager: pager.clone(),
            row,
            column: focus.col,
            staged: staged.map(|value| value.map(str::to_owned)),
            provider: self.tab_provider(index),
        };
        let name = column.name.clone();
        let type_name = column.display_type();
        self.value_panel.request = Some(key.clone());
        cx.spawn(async move |this, cx| {
            let cell = cx
                .background_executor()
                .spawn(async move {
                    let _job = job;
                    let cell = preview_value(read, &key, name, type_name);
                    (key, cell)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.value_panel.request.as_ref() == Some(&cell.0) {
                    this.value_panel.preview = Some(Arc::new(cell.1));
                }
                // A newer selection is scheduled on the next render.
                cx.notify();
            });
        })
        .detach();
        None
    }

    /// DBeaver-style "Value" panel showing the focused grid cell in a chosen format
    pub(super) fn render_value_panel(
        &mut self,
        active_tab_idx: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = self.theme.clone();
        let cell = self.focused_cell_value(active_tab_idx, cx);
        let format = cell
            .as_ref()
            .map(|cell| cell.format)
            .unwrap_or(ValueFormat::Text);
        let formatted = cell.as_ref().and_then(|cell| cell.formatted.clone());
        let state = &self.value_panel;

        let mut panel = div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(self.layout.value_panel_width))
            .h_full()
            .bg(theme.bg_editor)
            .border_l_1()
            .border_color(theme.border);

        // Tab strip: a single "Value" tab, and a minimize button that hides the panel
        panel = panel.child(
            div()
                .flex()
                .flex_none()
                .items_end()
                .justify_between()
                .h(px(30.0))
                .pl_1()
                .bg(theme.bg_surface)
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .h(px(26.0))
                        .px_2()
                        .bg(theme.bg_editor)
                        .border_t_2()
                        .border_color(rgb(ACCENT))
                        .border_x_1()
                        .text_size(px(12.0))
                        .text_color(theme.text_primary)
                        .child(img("icons/grid.svg").w(px(13.0)).h(px(13.0)))
                        .child("Value")
                        .child(
                            div()
                                .id("btn-close-value-panel")
                                .px_0p5()
                                .rounded_xs()
                                .text_size(px(11.0))
                                .text_color(theme.text_muted)
                                .cursor_pointer()
                                .hover(|s| {
                                    s.bg(theme.bg_surface_hover).text_color(theme.text_primary)
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.layout.show_right_panels = false;
                                    this.value_panel.show_format_menu = false;
                                    cx.notify();
                                }))
                                .child("✕"),
                        ),
                )
                .child(
                    div()
                        .id("btn-minimize-value-panel")
                        .mb(px(10.0))
                        .mr_2()
                        .w(px(11.0))
                        .h(px(6.0))
                        .border_1()
                        .border_color(theme.text_muted)
                        .cursor_pointer()
                        .hover(|s| s.border_color(theme.text_primary))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.layout.show_right_panels = false;
                            this.value_panel.show_format_menu = false;
                            cx.notify();
                        })),
                ),
        );

        // Toolbar: format picker, copy, and the overflow toggle for word wrap
        let copy_text = cell.as_ref().and_then(|cell| cell.copy_text.clone());
        panel = panel.child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_between()
                .h(px(28.0))
                .px_1()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .id("btn-value-format")
                                .flex()
                                .items_center()
                                .gap_1()
                                .px_1p5()
                                .py_0p5()
                                .rounded_xs()
                                .text_size(px(12.0))
                                .text_color(theme.text_secondary)
                                .cursor_pointer()
                                .when(state.show_format_menu, |d| d.bg(theme.bg_surface_selected))
                                .hover(|s| s.bg(theme.bg_surface_hover))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.value_panel.show_format_menu =
                                        !this.value_panel.show_format_menu;
                                    cx.notify();
                                }))
                                .child(format_glyph(format, &theme))
                                .child(format.label())
                                .child(
                                    div()
                                        .text_size(px(8.0))
                                        .text_color(theme.text_muted)
                                        .child("▼"),
                                ),
                        )
                        .child(toolbar_button("btn-value-copy", "⧉", &theme).when_some(
                            copy_text,
                            |d, text| {
                                d.on_click(cx.listener(move |this, _, _, cx| {
                                    this.clipboard_request_id += 1;
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        (*text).clone(),
                                    ))
                                }))
                            },
                        )),
                )
                .child(
                    toolbar_button("btn-value-wrap", "⋮", &theme)
                        .when(state.word_wrap, |d| d.text_color(theme.text_accent))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.value_panel.word_wrap = !this.value_panel.word_wrap;
                            cx.notify();
                        })),
                ),
        );

        // Content
        let content = match (&cell, &formatted) {
            (None, _) => empty_message(
                "Select a cell; values load in the background (256 KiB limit).",
                &theme,
            ),
            (Some(cell), _) if cell.error.is_some() => {
                div().p_2().child(cell.error.clone().unwrap())
            }
            (Some(cell), None) => div()
                .p_2()
                .font_family(MONO_FONT)
                .text_size(px(12.0))
                .text_color(theme.text_muted)
                .child(if cell.default {
                    "[DEFAULT · unset]"
                } else {
                    "[NULL]"
                }),
            (Some(_), Some(value)) => render_lines(value, state.word_wrap, &theme),
        };
        panel = panel.child(
            div()
                .id("value-panel-content")
                .flex_1()
                .min_h_0()
                .overflow_scroll()
                .child(content),
        );

        if let Some(error) = formatted.as_ref().and_then(|f| f.error.clone()) {
            panel = panel.child(
                div()
                    .flex_none()
                    .px_2()
                    .py_1()
                    .border_t_1()
                    .border_color(theme.border)
                    .text_size(px(11.0))
                    .text_color(theme.text_warning)
                    .child(error),
            );
        }

        // Footer: what is shown
        if let Some(cell) = &cell {
            let size = if cell.error.is_some() {
                "unavailable".into()
            } else if cell.default {
                "DEFAULT".into()
            } else {
                cell.byte_len
                    .map(human_size)
                    .unwrap_or_else(|| "NULL".into())
            };
            panel = panel.child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .h(px(22.0))
                    .px_2()
                    .bg(theme.bg_surface)
                    .border_t_1()
                    .border_color(theme.border)
                    .text_size(px(10.5))
                    .text_color(theme.text_muted)
                    .child(div().min_w_0().truncate().child(format!(
                        "{} · {} · row {}",
                        cell.column,
                        cell.type_name,
                        cell.row + 1
                    )))
                    .child(div().flex_none().child(size)),
            );
        }

        panel = panel.child(self.render_panel_splitter(PanelSplitter::ValuePanel, cx));

        if state.show_format_menu {
            panel = panel.child(self.render_format_menu(format, &theme, cx));
        }

        panel
    }

    fn render_format_menu(
        &self,
        current: ValueFormat,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let state = &self.value_panel;
        let mut menu = div()
            .id("value-format-menu")
            .absolute()
            .top(px(56.0))
            .left(px(6.0))
            .w(px(170.0))
            .py_1()
            .flex()
            .flex_col()
            .rounded_md()
            .bg(theme.bg_surface)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .occlude()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.value_panel.show_format_menu = false;
                cx.notify();
            }));

        for fmt in ValueFormat::ALL {
            menu = menu.child(
                menu_item(("value-format-item", fmt as usize), fmt == current, theme)
                    .child(
                        div()
                            .w(px(22.0))
                            .flex()
                            .justify_center()
                            .child(format_glyph(fmt, theme)),
                    )
                    .child(fmt.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.value_panel.format_override = Some(fmt);
                        this.value_panel.show_format_menu = false;
                        cx.notify();
                    })),
            );
        }

        menu.child(div().my_1().mx_2().h(px(1.0)).bg(theme.border))
            .child(
                menu_item("value-menu-wrap", state.word_wrap, theme)
                    .child("Word Wrap")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.value_panel.word_wrap = !this.value_panel.word_wrap;
                        this.value_panel.show_format_menu = false;
                        cx.notify();
                    })),
            )
            .child(
                menu_item("value-menu-auto-format", state.auto_format, theme)
                    .child("Auto Format")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.value_panel.auto_format = !this.value_panel.auto_format;
                        this.value_panel.show_format_menu = false;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(24.0))
                    .pl(px(26.0))
                    .text_size(px(12.5))
                    .text_color(theme.text_muted)
                    .child("Source: UTF-8; file encoding in Value editor"),
            )
    }
}

/// Menu row with a leading check mark column
fn menu_item(id: impl Into<ElementId>, checked: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap_1()
        .h(px(24.0))
        .mx_1()
        .px_1()
        .rounded_sm()
        .text_size(px(12.5))
        .text_color(theme.text_primary)
        .cursor_pointer()
        .hover(|s| s.bg(rgb(ACCENT)).text_color(rgb(0xffffff)))
        .child(
            div()
                .w(px(14.0))
                .text_size(px(11.0))
                .child(if checked { "✓" } else { "" }),
        )
}

fn toolbar_button(id: &'static str, label: &'static str, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(22.0))
        .h(px(20.0))
        .rounded_xs()
        .text_size(px(13.0))
        .text_color(theme.text_secondary)
        .cursor_pointer()
        .hover(|s| s.bg(theme.bg_surface_hover).text_color(theme.text_primary))
        .child(label)
}

fn format_glyph(format: ValueFormat, theme: &Theme) -> Div {
    div()
        .font_family(MONO_FONT)
        .font_weight(FontWeight::BOLD)
        .text_size(px(10.0))
        .text_color(theme.text_accent)
        .child(format.glyph())
}

fn empty_message(text: &'static str, theme: &Theme) -> Div {
    div()
        .p_3()
        .text_size(px(11.5))
        .text_color(theme.text_muted)
        .child(text)
}

/// Numbered lines in a monospace font; without wrap each line keeps its full width
fn render_lines(value: &FormattedValue, word_wrap: bool, theme: &Theme) -> Div {
    let total = value.lines.len();
    let shown = total.min(MAX_RENDERED_LINES);
    let gutter_w = 10.0 + 7.5 * shown.max(1).to_string().len() as f32;

    let mut rows = div()
        .flex()
        .flex_col()
        .py_1()
        .font_family(MONO_FONT)
        .text_size(px(12.0))
        .line_height(px(18.0))
        .text_color(theme.text_primary)
        .when(word_wrap, |d| d.w_full());

    for (i, line) in value.lines.iter().take(shown).enumerate() {
        let text = match line.char_indices().nth(MAX_LINE_CHARS) {
            Some((cut, _)) => format!("{}…", &line[..cut]),
            None => line.clone(),
        };
        rows = rows.child(
            div()
                .flex()
                .flex_none()
                .items_start()
                .child(
                    div()
                        .flex_none()
                        .w(px(gutter_w))
                        .pr_2()
                        .flex()
                        .justify_end()
                        .text_color(theme.text_muted)
                        .child((i + 1).to_string()),
                )
                .child(
                    div()
                        .pr_2()
                        .when(word_wrap, |d| d.flex_1().min_w_0())
                        .when(!word_wrap, |d| d.flex_none().whitespace_nowrap())
                        .child(text),
                ),
        );
    }

    if total > shown {
        rows = rows.child(
            div()
                .pl(px(gutter_w))
                .text_color(theme.text_muted)
                .child(format!("… {} more lines not shown", total - shown)),
        );
    }
    rows
}

fn human_size(bytes: usize) -> String {
    match bytes {
        b if b < 1024 => format!("{b} B"),
        b if b < 1024 * 1024 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
    }
}

#[cfg(test)]
mod tests {
    use super::{PreviewKey, preview_value};
    use crate::workspace::value_editor::ValueRead;
    use db_wire::{ColumnMeta, DataType, DbDriverKind, QueryResultPager};
    use std::sync::Arc;
    use ui_grid::RowRef;

    fn preview(bytes: Option<&[u8]>, kind: DataType, provider: DbDriverKind) -> super::CellValue {
        let pager = QueryResultPager::new(vec![ColumnMeta::new("value", kind)]).unwrap();
        pager.append_row([bytes]).unwrap();
        let key = PreviewKey {
            tab: 1,
            pager: 0,
            row: 0,
            column: 0,
            revision: 0,
            format: None,
            auto_format: true,
            utc: false,
            default: false,
        };
        preview_value(
            ValueRead {
                pager: Arc::new(pager),
                row: RowRef::Fetched(0),
                column: 0,
                staged: None,
                provider,
            },
            &key,
            "value".into(),
            "BLOB".into(),
        )
    }

    #[test]
    fn binary_preview_keeps_literal_prefix_bytes_and_rejects_oversized_values() {
        for provider in [DbDriverKind::Sqlite, DbDriverKind::MySql] {
            let value = preview(Some(b"\\xdead"), DataType::Bytea, provider);
            let rendered = value.formatted.unwrap().lines.join("\n");
            assert!(rendered.contains("5c 78 64 65 61 64"));
            assert!(value.error.is_none());
        }
        let value = preview(
            Some(&vec![b'a'; 1024 * 1024]),
            DataType::Text,
            DbDriverKind::Sqlite,
        );
        assert!(value.error.is_some());
        assert!(value.bytes.is_none());
        assert!(value.formatted.is_none());
        let pager = Arc::new(
            QueryResultPager::new(vec![ColumnMeta::new("value", DataType::Text)]).unwrap(),
        );
        let key = PreviewKey {
            tab: 1,
            pager: 0,
            row: 0,
            column: 0,
            revision: 0,
            format: None,
            auto_format: false,
            utc: false,
            default: true,
        };
        let default = preview_value(
            ValueRead {
                pager,
                row: RowRef::New(0),
                column: 0,
                staged: Some(None),
                provider: DbDriverKind::Sqlite,
            },
            &key,
            "value".into(),
            "TEXT".into(),
        );
        assert!(default.default);
        assert!(default.formatted.is_none());
        let null = preview(None, DataType::Bytea, DbDriverKind::Sqlite);
        assert!(null.error.is_none());
        assert!(!null.default);
        assert!(null.formatted.is_none());
        let empty = preview(Some(b""), DataType::Bytea, DbDriverKind::Sqlite);
        assert!(empty.error.is_none());
        assert!(empty.formatted.is_some());
    }
}
