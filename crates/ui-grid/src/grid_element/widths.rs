use super::*;
impl VirtualDataGrid {
    pub fn update_viewport_height(&mut self, height: f32) {
        if (self.viewport_height - height).abs() > 2.0 {
            self.viewport_height = height;
        }
    }

    /// Accurately estimate rendered text width for GPUI proportional fonts (12px standard, scales with size)
    pub fn estimate_text_width(text: &str, font_size: f32) -> f32 {
        let scale = font_size / 12.0;
        let mut width = 0.0f32;
        for ch in text.chars() {
            let char_w = match ch {
                '.' | ':' | ',' | ';' | '\'' | '"' | '!' | '|' | 'i' | 'l' | 'j' | '1' | '`' => 3.8,
                'r' | 't' | 'f' | 'I' | ' ' | '-' | '(' | ')' | '[' | ']' | '{' | '}' | '/'
                | '\\' => 5.2,
                'm' | 'w' | 'M' | 'W' | '@' | '%' | '&' => 9.5,
                'A'..='Z' => 7.8,
                '0'..='9' => 6.8,
                _ => 6.5,
            };
            width += char_w * scale;
        }
        width
    }

    /// Content-fit width of each column, from its header and the sampled cells, capped at `MAX_AUTO_COL_WIDTH`.
    /// Cells measured with the window's fonts count as measured; the rest are estimated per character.
    pub fn calculate_content_widths(&self) -> Vec<f32> {
        let Some(ref pager) = self.pager else {
            return Vec::new();
        };

        let sample_rows = pager.total_rows().min(WIDTH_SAMPLE_ROWS);
        pager
            .columns
            .iter()
            .enumerate()
            .map(|(col_idx, col)| {
                // 1. Column header width; until it is measured with real fonts, estimated per character.
                // Name and badge render heavier than the estimator assumes, hence the 10% margin.
                let header_w = self.header_widths.get(col_idx).copied().unwrap_or_else(|| {
                    let name_w = Self::estimate_text_width(&col.name, 12.0) * 1.1;
                    let badge_w =
                        Self::estimate_text_width(dbeaver_type_badge(col.data_type), 10.0) * 1.1;
                    let is_pk = col.is_primary_key || col.name.eq_ignore_ascii_case("id");
                    let is_sorted = self.ordering.as_ref().is_some_and(|o| o.column == col.name);
                    header_cell_width(badge_w, name_w, is_pk, is_sorted.then_some(9.0))
                });

                // 2. Widest of the sampled cells: measured ones as measured, the rest estimated
                let (mut max_cell_w, first_estimated) = match self.cell_widths.get(col_idx) {
                    Some(&w) => (w, self.cell_widths_rows),
                    None => (0.0f32, 0),
                };
                for r in first_estimated..sample_rows {
                    let cell_str = resolve_cell(pager, &self.changeset, r, col_idx)
                        .unwrap_or_else(|| "NULL".to_string());
                    // The estimate runs a little narrow on most fonts, hence the 10% margin
                    let cell_w = data_cell_width(
                        Self::estimate_text_width(cell_measure_sample(&cell_str), 12.0) * 1.1,
                    );
                    max_cell_w = max_cell_w.max(cell_w);
                }

                header_w
                    .max(max_cell_w)
                    .clamp(MIN_COL_WIDTH, MAX_AUTO_COL_WIDTH)
            })
            .collect()
    }

    /// Measures the column headers and the sampled data cells with the window's fonts, so
    /// auto-sized columns fit their content exactly instead of by the per-character estimate.
    pub fn measure_widths(&mut self, window: &Window) {
        self.measure_header_widths(window);
        self.measure_cell_widths(window);
    }

    /// Measures the sampled data cells after the sample changed and re-fits the auto-sized columns
    pub(super) fn measure_cell_widths(&mut self, window: &Window) {
        if !self.cell_widths_stale {
            return;
        }
        self.cell_widths_stale = false;
        let Some(pager) = self.pager.clone() else {
            return;
        };

        let base = window.text_style();
        let font = base.font();
        let sample_rows = pager.total_rows().min(WIDTH_SAMPLE_ROWS);
        self.cell_widths = (0..pager.columns.len())
            .map(|c| {
                (0..sample_rows)
                    .map(|r| {
                        let text = resolve_cell(&pager, &self.changeset, r, c)
                            .unwrap_or_else(|| "NULL".to_string());
                        let text = cell_measure_sample(&text);
                        let run = TextRun {
                            len: text.len(),
                            font: font.clone(),
                            color: base.color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line = window.text_system().shape_line(
                            SharedString::from(text.to_string()),
                            px(12.0),
                            &[run],
                            None,
                        );
                        data_cell_width(f32::from(line.width).ceil())
                    })
                    .fold(0.0f32, f32::max)
            })
            .collect();
        self.cell_widths_rows = sample_rows;
        self.content_widths = self.calculate_content_widths();
        self.apply_viewport_distribution();
    }

    /// Measures the column headers with the window's fonts, once per result and ordering, and
    /// widens auto-sized columns whose header the per-character estimate underrated.
    /// Only ever widens, so a re-ordered result keeps its widths apart from room for the sort glyph.
    pub(super) fn measure_header_widths(&mut self, window: &Window) {
        let Some(pager) = self.pager.clone() else {
            return;
        };
        let key = (Arc::as_ptr(&pager) as usize, self.ordering.clone());
        if self.header_widths_key.as_ref() == Some(&key) {
            return;
        }
        self.header_widths_key = Some(key);

        let base = window.text_style();
        let text_width = |text: &str, size: f32, weight: FontWeight| -> f32 {
            let run = TextRun {
                len: text.len(),
                font: Font {
                    weight,
                    ..base.font()
                },
                color: base.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let line = window.text_system().shape_line(
                SharedString::from(text.to_string()),
                px(size),
                &[run],
                None,
            );
            f32::from(line.width).ceil()
        };

        self.header_widths = pager
            .columns
            .iter()
            .map(|col| {
                let badge_w = text_width(dbeaver_type_badge(col.data_type), 10.0, FontWeight::BOLD);
                let name_w = text_width(&col.name, 12.0, FontWeight::SEMIBOLD);
                let is_pk = col.is_primary_key || col.name.eq_ignore_ascii_case("id");
                let sort_glyph = self
                    .ordering
                    .as_ref()
                    .filter(|o| o.column == col.name)
                    .map(|o| if o.descending { "↓" } else { "↑" });
                let sort_w = sort_glyph.map(|g| text_width(g, 12.0, FontWeight::BOLD));
                header_cell_width(badge_w, name_w, is_pk, sort_w)
            })
            .collect();

        if self.content_widths.len() != self.header_widths.len() {
            return;
        }
        for (c, &header_w) in self.header_widths.iter().enumerate() {
            self.content_widths[c] = self.content_widths[c].max(header_w.min(MAX_AUTO_COL_WIDTH));
        }
        self.apply_viewport_distribution();
    }

    /// Recalculates the content widths and lays the columns out with them; the cells are
    /// measured with real fonts on the next `measure_widths`
    pub fn recalculate_all_widths(&mut self) {
        self.width_sampled_rows = self
            .pager
            .as_ref()
            .map(|p| p.total_rows().min(WIDTH_SAMPLE_ROWS))
            .unwrap_or(0);
        self.cell_widths_stale = self.width_sampled_rows > self.cell_widths_rows;
        self.content_widths = self.calculate_content_widths();
        self.apply_viewport_distribution();
    }

    /// Re-measures column widths while the leading sample rows are still streaming in.
    /// Cheap once the sample is complete, so it can be called on every appended batch.
    pub fn on_rows_appended(&mut self) {
        let total_rows = self.pager.as_ref().map(|p| p.total_rows()).unwrap_or(0);
        if self.width_sampled_rows < WIDTH_SAMPLE_ROWS && total_rows > self.width_sampled_rows {
            self.recalculate_all_widths();
        }
    }

    /// Update the grid's available viewport width and distribute columns if width changed
    pub fn update_viewport_width(&mut self, width: f32) {
        if (self.viewport_width - width).abs() > 2.0 {
            self.viewport_width = width;
            self.apply_viewport_distribution();
        }
    }

    /// Lays the columns out: each fits its content (up to `MAX_AUTO_COL_WIDTH`) unless the user
    /// resized it, in which case it keeps that width. Free room in the viewport stays empty.
    pub fn apply_viewport_distribution(&mut self) {
        let num_cols = self
            .pager
            .as_ref()
            .map(|p| p.num_cols() as usize)
            .unwrap_or(0);
        if num_cols == 0 {
            self.column_widths.clear();
            self.content_widths.clear();
            return;
        }

        if self.content_widths.len() != num_cols {
            self.content_widths = self.calculate_content_widths();
        }

        self.column_widths = (0..num_cols)
            .map(|c| {
                if self.manual_column_widths.contains(&c) {
                    self.column_widths
                        .get(c)
                        .copied()
                        .unwrap_or(MIN_COL_WIDTH)
                        .max(MIN_COL_WIDTH)
                } else {
                    self.content_widths[c]
                }
            })
            .collect();
    }

    /// Manually resize a column with the mouse, pinning its width
    pub fn set_column_width(&mut self, col: usize, width: f32) {
        if col < self.column_widths.len() {
            self.column_widths[col] = width.max(MIN_COL_WIDTH);
            self.manual_column_widths.insert(col);
        }
    }

    /// Double-click action: resets a column to auto-fit its content and unpins it
    pub fn auto_fit_single_column(&mut self, col: usize) {
        self.manual_column_widths.remove(&col);
        // A refilled result has no measured cells yet; measure them on the next frame
        self.cell_widths_stale |= self.cell_widths.is_empty();
        self.content_widths = self.calculate_content_widths();
        self.apply_viewport_distribution();
    }

    pub fn scroll_by(&mut self, delta_y: f32, delta_x: f32) {
        let total_rows = self.row_count();
        let max_scroll_y =
            (total_rows as f32 * self.theme.row_height - self.viewport_height).max(0.0);

        self.scroll_top = (self.scroll_top + delta_y).clamp(0.0, max_scroll_y);
        self.scroll_left = (self.scroll_left + delta_x).max(0.0);
    }
}
