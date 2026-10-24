use mem_engine::QueryResultPager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridCoord {
    pub row: u64,
    pub col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridSelection {
    pub anchor: Option<GridCoord>,
    pub focus: Option<GridCoord>,
}

impl Default for GridSelection {
    fn default() -> Self {
        Self {
            anchor: None,
            focus: None,
        }
    }
}

impl GridSelection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn single(row: u64, col: usize) -> Self {
        let coord = GridCoord { row, col };
        Self {
            anchor: Some(coord),
            focus: Some(coord),
        }
    }

    pub fn set_range(&mut self, start: GridCoord, end: GridCoord) {
        self.anchor = Some(start);
        self.focus = Some(end);
    }

    pub fn clear(&mut self) {
        self.anchor = None;
        self.focus = None;
    }

    pub fn bounding_box(&self) -> Option<(u64, u64, usize, usize)> {
        let (a, f) = (self.anchor?, self.focus?);
        let min_row = a.row.min(f.row);
        let max_row = a.row.max(f.row);
        let min_col = a.col.min(f.col);
        let max_col = a.col.max(f.col);
        Some((min_row, max_row, min_col, max_col))
    }

    pub fn is_selected(&self, row: u64, col: usize) -> bool {
        if let Some((min_r, max_r, min_c, max_c)) = self.bounding_box() {
            row >= min_r && row <= max_r && col >= min_c && col <= max_c
        } else {
            false
        }
    }

    /// Formats selected cells as TSV (Tab-Separated Values) for clipboard copy
    pub fn to_tsv(&self, pager: &QueryResultPager) -> String {
        let Some((min_r, max_r, min_c, max_c)) = self.bounding_box() else {
            return String::new();
        };

        let mut out = String::new();
        for r in min_r..=max_r {
            for c in min_c..=max_c {
                if c > min_c {
                    out.push('\t');
                }
                if let Some(cell) = pager.get_cell(r, c) {
                    out.push_str(&String::from_utf8_lossy(&cell));
                }
            }
            out.push('\n');
        }
        out
    }

    /// Formats selected cells as CSV
    pub fn to_csv(&self, pager: &QueryResultPager) -> String {
        let Some((min_r, max_r, min_c, max_c)) = self.bounding_box() else {
            return String::new();
        };

        let mut out = String::new();
        for r in min_r..=max_r {
            for c in min_c..=max_c {
                if c > min_c {
                    out.push(',');
                }
                if let Some(cell) = pager.get_cell(r, c) {
                    let s = String::from_utf8_lossy(&cell);
                    if s.contains(',') || s.contains('"') || s.contains('\n') {
                        out.push('"');
                        out.push_str(&s.replace('"', "\"\""));
                        out.push('"');
                    } else {
                        out.push_str(&s);
                    }
                }
            }
            out.push('\n');
        }
        out
    }

    /// Formats selected cells as SQL INSERT statement
    pub fn to_sql_insert(&self, table_name: &str, pager: &QueryResultPager) -> String {
        let Some((min_r, max_r, min_c, max_c)) = self.bounding_box() else {
            return String::new();
        };

        let col_names: Vec<&str> = (min_c..=max_c)
            .map(|c| pager.columns.get(c).map(|col| col.name.as_str()).unwrap_or("col"))
            .collect();

        let mut out = format!("INSERT INTO \"{table_name}\" ({}) VALUES\n", col_names.join(", "));

        for r in min_r..=max_r {
            out.push_str("  (");
            for c in min_c..=max_c {
                if c > min_c {
                    out.push_str(", ");
                }
                if let Some(cell) = pager.get_cell(r, c) {
                    let s = String::from_utf8_lossy(&cell);
                    out.push('\'');
                    out.push_str(&s.replace('\'', "''"));
                    out.push('\'');
                } else {
                    out.push_str("NULL");
                }
            }
            if r == max_r {
                out.push_str(");\n");
            } else {
                out.push_str("),\n");
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mem_engine::{ColumnMeta, DataType};

    #[test]
    fn test_grid_selection_and_tsv_csv() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int4),
            ColumnMeta::new("name", DataType::Text),
        ];
        let pager = QueryResultPager::new(cols).unwrap();
        pager.append_row(vec![Some(&b"1"[..]), Some(&b"Alice"[..])]).unwrap();
        pager.append_row(vec![Some(&b"2"[..]), Some(&b"Bob, Jr."[..])]).unwrap();

        let mut sel = GridSelection::new();
        sel.set_range(GridCoord { row: 0, col: 0 }, GridCoord { row: 1, col: 1 });

        assert!(sel.is_selected(0, 0));
        assert!(sel.is_selected(1, 1));
        assert!(!sel.is_selected(2, 0));

        let tsv = sel.to_tsv(&pager);
        assert_eq!(tsv, "1\tAlice\n2\tBob, Jr.\n");

        let csv = sel.to_csv(&pager);
        assert_eq!(csv, "1,Alice\n2,\"Bob, Jr.\"\n");
    }
}
