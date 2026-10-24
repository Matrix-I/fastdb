use std::sync::Arc;
use gpui::*;
use mem_engine::QueryResultPager;
use ui_core::theme::Theme;

use crate::changeset::Changeset;
use crate::selection::GridSelection;

pub struct VirtualDataGrid {
    pub pager: Option<Arc<QueryResultPager>>,
    pub theme: Theme,
    pub table_name: String,
    pub scroll_top: f32,
    pub scroll_left: f32,
    pub viewport_height: f32,
    pub viewport_width: f32,
    pub column_widths: Vec<f32>,
    pub selection: GridSelection,
    pub changeset: Changeset,
    pub editing_cell: Option<(u64, usize, String)>,
}

impl VirtualDataGrid {
    pub fn new(pager: Option<Arc<QueryResultPager>>, theme: Theme) -> Self {
        let col_count = pager.as_ref().map(|p| p.num_cols() as usize).unwrap_or(0);
        let column_widths = vec![140.0; col_count];

        Self {
            pager,
            theme,
            table_name: "query_result".to_string(),
            scroll_top: 0.0,
            scroll_left: 0.0,
            viewport_height: 600.0,
            viewport_width: 1000.0,
            column_widths,
            selection: GridSelection::new(),
            changeset: Changeset::new(),
            editing_cell: None,
        }
    }

    pub fn set_pager(&mut self, pager: Arc<QueryResultPager>) {
        let col_count = pager.num_cols() as usize;
        self.column_widths = vec![140.0; col_count];
        self.pager = Some(pager);
        self.selection.clear();
        self.changeset.clear();
        self.editing_cell = None;
        self.scroll_top = 0.0;
        self.scroll_left = 0.0;
    }

    pub fn set_table_name(&mut self, name: impl Into<String>) {
        self.table_name = name.into();
    }

    pub fn set_column_width(&mut self, col: usize, width: f32) {
        if let Some(w) = self.column_widths.get_mut(col) {
            *w = width.max(50.0);
        }
    }

    pub fn scroll_by(&mut self, delta_y: f32, delta_x: f32) {
        let total_rows = self.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0);
        let max_scroll_y = (total_rows as f32 * self.theme.row_height - self.viewport_height).max(0.0);

        self.scroll_top = (self.scroll_top + delta_y).clamp(0.0, max_scroll_y);
        self.scroll_left = (self.scroll_left + delta_x).max(0.0);
    }
}

impl Render for VirtualDataGrid {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = &self.theme;
        let row_height = theme.row_height;
        let header_height = theme.header_height;
        let gutter_width = 52.0;

        let total_rows = self.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0);
        let num_cols = self.pager.as_ref().map(|p| p.num_cols() as usize).unwrap_or(0);

        // 2D Viewport Row Culling: compute only visible rows
        let first_row = (self.scroll_top / row_height).floor() as u64;
        let visible_count = (self.viewport_height / row_height).ceil() as u64 + 2;
        let last_row = (first_row + visible_count).min(total_rows);

        let mut grid_container = div()
            .flex()
            .flex_col()
            .size_full()
            .overflow_hidden()
            .bg(theme.bg_grid)
            .text_color(theme.text_primary);

        // 1. Sticky Header Row
        let mut header_row = div()
            .flex()
            .h(px(header_height))
            .bg(theme.bg_grid_header)
            .border_b_1()
            .border_color(theme.border)
            .child(
                // Gutter header corner
                div()
                    .w(px(gutter_width))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_r_1()
                    .border_color(theme.border)
                    .text_size(px(11.0))
                    .text_color(theme.text_muted)
                    .child("#")
            );

        if let Some(ref pager) = self.pager {
            for (col_idx, col) in pager.columns.iter().enumerate() {
                let width = self.column_widths.get(col_idx).copied().unwrap_or(140.0);
                header_row = header_row.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .w(px(width))
                        .h_full()
                        .px_2()
                        .border_r_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(
                                    div()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_size(px(12.0))
                                        .child(col.name.clone())
                                )
                                .child(
                                    div()
                                        .text_size(px(9.0))
                                        .text_color(theme.text_muted)
                                        .child(col.data_type.display_name())
                                )
                        )
                );
            }
        }

        grid_container = grid_container.child(header_row);

        // 2. Visible Data Rows (Zero-allocation / Culled)
        let mut body = div()
            .flex()
            .flex_col()
            .flex_1()
            .w_full()
            .overflow_hidden();

        if let Some(ref pager) = self.pager {
            for r in first_row..last_row {
                let is_alt = r % 2 == 1;
                let row_bg = if is_alt { theme.bg_grid_alt } else { theme.bg_grid };

                let mut row_div = div()
                    .flex()
                    .h(px(row_height))
                    .bg(row_bg)
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        // Gutter row index
                        div()
                            .w(px(gutter_width))
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .border_r_1()
                            .border_color(theme.border)
                            .text_size(px(10.0))
                            .text_color(theme.text_muted)
                            .child(format!("{}", r + 1))
                    );

                for c in 0..num_cols {
                    let width = self.column_widths.get(c).copied().unwrap_or(140.0);
                    let is_selected = self.selection.is_selected(r, c);
                    let is_modified = self.changeset.is_modified(r, c);

                    let mut cell_div = div()
                        .flex()
                        .items_center()
                        .w(px(width))
                        .h_full()
                        .px_2()
                        .border_r_1()
                        .border_color(theme.border)
                        .text_size(px(12.0))
                        .overflow_hidden();

                    if is_selected {
                        cell_div = cell_div.bg(theme.bg_grid_cell_selected);
                    } else if is_modified {
                        cell_div = cell_div.bg(theme.bg_grid_cell_modified);
                    }

                    // Display modified text if staged, otherwise fetch from zero-copy page
                    let cell_content = if let Some(mod_val) = self.changeset.get_modified_value(r, c) {
                        mod_val.to_string()
                    } else if let Some(cell_bytes) = pager.get_cell(r, c) {
                        String::from_utf8_lossy(&cell_bytes).to_string()
                    } else {
                        "NULL".to_string()
                    };

                    let is_null = cell_content == "NULL" && !self.changeset.is_modified(r, c);
                    if is_null {
                        cell_div = cell_div
                            .text_color(theme.text_muted)
                            .child(cell_content);
                    } else {
                        cell_div = cell_div.child(cell_content);
                    }

                    row_div = row_div.child(cell_div);
                }

                body = body.child(row_div);
            }
        }

        grid_container.child(body)
    }
}
