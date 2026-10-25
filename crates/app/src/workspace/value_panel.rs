use super::*;
use gpui::prelude::FluentBuilder;
use super::value_format::{detect_format, format_value, FormattedValue, ValueFormat};

/// Width of the record value side panel
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
    pub auto_format: bool,
    pub show_format_menu: bool,
    cache: Option<(CacheKey, Arc<FormattedValue>)>,
}

impl Default for ValuePanelState {
    fn default() -> Self {
        Self {
            format_override: None,
            word_wrap: false,
            auto_format: true,
            show_format_menu: false,
            cache: None,
        }
    }
}

#[derive(PartialEq, Eq)]
struct CacheKey {
    bytes: Vec<u8>,
    data_type: DataType,
    format: ValueFormat,
    auto_format: bool,
}

/// The focused cell of the active grid, read for display
struct CellValue {
    column: String,
    type_name: String,
    data_type: DataType,
    row: u64,
    /// `None` for SQL NULL
    bytes: Option<Vec<u8>>,
}

impl Workspace {
    /// Click on a grid cell: selects it, or extends the selection with Shift
    pub(super) fn select_grid_cell(&mut self, row: u64, col: usize, extend: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else { return };
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

    fn focused_cell_value(&self, active_tab_idx: usize) -> Option<CellValue> {
        let grid = &self.tabs.get(active_tab_idx)?.grid;
        let focus = grid.selection.focus?;
        let pager = grid.pager.as_ref()?;
        let col = pager.columns.get(focus.col)?;
        if focus.row >= pager.total_rows() {
            return None;
        }
        let bytes = match grid.changeset.get_modified_value(focus.row, focus.col) {
            Some(v) => Some(v.as_bytes().to_vec()),
            None => pager.get_cell(focus.row, focus.col),
        };
        Some(CellValue {
            column: col.name.clone(),
            type_name: col.display_type(),
            data_type: col.data_type,
            row: focus.row,
            bytes,
        })
    }

    fn formatted_cell(&mut self, cell: &CellValue, format: ValueFormat) -> Option<Arc<FormattedValue>> {
        let bytes = cell.bytes.as_ref()?;
        let key = CacheKey {
            bytes: bytes.clone(),
            data_type: cell.data_type,
            format,
            auto_format: self.value_panel.auto_format,
        };
        if let Some((cached_key, value)) = &self.value_panel.cache {
            if *cached_key == key {
                return Some(value.clone());
            }
        }
        let value = Arc::new(format_value(bytes, cell.data_type, format, key.auto_format));
        self.value_panel.cache = Some((key, value.clone()));
        Some(value)
    }

    /// DBeaver-style "Value" panel showing the focused grid cell in a chosen format
    pub(super) fn render_value_panel(&mut self, active_tab_idx: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme.clone();
        let cell = self.focused_cell_value(active_tab_idx);
        let detected = cell
            .as_ref()
            .and_then(|c| c.bytes.as_deref().map(|b| detect_format(b, c.data_type)))
            .unwrap_or(ValueFormat::Text);
        let format = self.value_panel.format_override.unwrap_or(detected);
        let formatted = cell.as_ref().and_then(|c| self.formatted_cell(c, format));
        let state = &self.value_panel;

        let mut panel = div()
            .relative()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(VALUE_PANEL_WIDTH))
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
                                .hover(|s| s.bg(theme.bg_surface_hover).text_color(theme.text_primary))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_right_panels = false;
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
                            this.show_right_panels = false;
                            this.value_panel.show_format_menu = false;
                            cx.notify();
                        })),
                ),
        );

        // Toolbar: format picker, copy, and the overflow toggle for word wrap
        let copy_text = formatted.as_ref().map(|f| f.lines.join("\n"));
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
                                    this.value_panel.show_format_menu = !this.value_panel.show_format_menu;
                                    cx.notify();
                                }))
                                .child(format_glyph(format, &theme))
                                .child(format.label())
                                .child(div().text_size(px(8.0)).text_color(theme.text_muted).child("▼")),
                        )
                        .child(
                            toolbar_button("btn-value-copy", "⧉", &theme)
                                .when_some(copy_text, |d, text| {
                                    d.on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(text.clone())))
                                }),
                        ),
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
            (None, _) => empty_message("Select a cell in the data grid to view its value.", &theme),
            (Some(_), None) => div()
                .p_2()
                .font_family(MONO_FONT)
                .text_size(px(12.0))
                .text_color(theme.text_muted)
                .child("[NULL]"),
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
            let size = cell.bytes.as_ref().map(|b| human_size(b.len())).unwrap_or_else(|| "NULL".to_string());
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
                    .child(div().min_w_0().truncate().child(format!("{} · {} · row {}", cell.column, cell.type_name, cell.row + 1)))
                    .child(div().flex_none().child(format!("{size} · UTF-8"))),
            );
        }

        if state.show_format_menu {
            panel = panel.child(self.render_format_menu(format, &theme, cx));
        }

        panel
    }

    fn render_format_menu(&self, current: ValueFormat, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
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
                    .child(div().w(px(22.0)).flex().justify_center().child(format_glyph(fmt, theme)))
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
                    .child("Encoding: UTF-8"),
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
        .child(div().w(px(14.0)).text_size(px(11.0)).child(if checked { "✓" } else { "" }))
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
    div().p_3().text_size(px(11.5)).text_color(theme.text_muted).child(text)
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
