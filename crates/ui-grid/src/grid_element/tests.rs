#[cfg(test)]
mod visible_columns_tests {
    use super::super::{GUTTER_WIDTH, clip_cell_text, visible_columns};

    #[test]
    fn only_columns_in_view_are_kept_with_one_extra_each_side() {
        let widths = [100.0; 10];
        // Unscrolled, a 300px view past the gutter shows columns 0..=2
        let (range, offset) = visible_columns(&widths, 0.0, 300.0 + GUTTER_WIDTH);
        assert_eq!((range, offset), (0..4, 0.0));
        // Scrolled by 450px: columns 4..=7 are in view, 3 and 8 are the extras
        let (range, offset) = visible_columns(&widths, 450.0 + GUTTER_WIDTH, 300.0);
        assert_eq!((range, offset), (3..9, 300.0));
        // Scrolled to the end
        let (range, _) = visible_columns(&widths, 800.0 + GUTTER_WIDTH, 300.0);
        assert_eq!(range, 7..10);
        assert_eq!(visible_columns(&[], 0.0, 300.0).0, 0..0);
    }

    #[test]
    fn cell_text_is_clipped_to_the_column() {
        assert_eq!(clip_cell_text("short".to_string(), 100.0), "short");
        assert_eq!(clip_cell_text("x".repeat(500), 100.0).len(), 41);
        assert_eq!(clip_cell_text("ngày".repeat(100), 10.0), "ngàyn");
    }
}

#[cfg(test)]
mod state_and_width_tests {
    use super::super::*;
    use core::prelude::v1::test;
    use mem_engine::{ColumnMeta, DataType};

    #[test]
    fn test_selection_text_copies_cell_or_range() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("name", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager
            .append_row(vec![Some(&b"1"[..]), Some(&b"Alice"[..])])
            .unwrap();
        pager.append_row(vec![Some(&b"2"[..]), None]).unwrap();

        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&pager));
        assert_eq!(grid.selection_text(), None);

        grid.selection = GridSelection::single(0, 1);
        assert_eq!(grid.selection_text().as_deref(), Some("Alice"));

        grid.changeset
            .set_cell(0, 1, Some(b"Alice".to_vec()), Some("Alicia".to_string()));
        grid.selection.set_range(
            crate::GridCoord { row: 0, col: 0 },
            crate::GridCoord { row: 1, col: 1 },
        );
        assert_eq!(grid.selection_text().as_deref(), Some("1\tAlicia\n2\t"));
    }

    #[test]
    fn test_refill_keeps_column_widths_of_the_same_columns() {
        let cols = || {
            vec![
                ColumnMeta::new("id", DataType::Int8),
                ColumnMeta::new("name", DataType::Text),
            ]
        };
        let first = Arc::new(QueryResultPager::new(cols()).unwrap());
        first
            .append_row(vec![Some(&b"1"[..]), Some(&b"Alice"[..])])
            .unwrap();

        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&first));
        grid.set_column_width(1, 300.0);
        grid.selection = GridSelection::single(0, 1);
        let widths = grid.column_widths.clone();

        let reordered = Arc::new(QueryResultPager::new(cols()).unwrap());
        reordered
            .append_row(vec![
                Some(&b"2"[..]),
                Some(&b"a much longer name than before, by far"[..]),
            ])
            .unwrap();
        grid.refill_pager(Arc::clone(&reordered));
        grid.on_rows_appended();
        assert_eq!(grid.column_widths, widths);
        assert!(grid.manual_column_widths.contains(&1));
        assert_eq!(grid.selection_text(), None);
        assert_eq!(grid.cell_value(0, 0).as_deref(), Some("2"));

        // Other columns are laid out from scratch
        let other = Arc::new(
            QueryResultPager::new(vec![ColumnMeta::new("email", DataType::Text)]).unwrap(),
        );
        grid.refill_pager(other);
        assert!(grid.manual_column_widths.is_empty());
        assert_eq!(grid.column_widths.len(), 1);
    }

    #[test]
    fn test_staged_rows_and_edits_are_shown() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("name", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager
            .append_row(vec![Some(&b"1"[..]), Some(&b"Alice"[..])])
            .unwrap();

        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&pager));
        grid.changeset.set_cell(0, 1, Some(b"Alice".to_vec()), None);
        grid.changeset
            .insert_row_at(1, vec![None, Some("Bob".to_string())]);

        assert_eq!(grid.row_count(), 2);
        assert_eq!(grid.cell_value(0, 0).as_deref(), Some("1"));
        assert_eq!(
            grid.cell_value(0, 1),
            None,
            "a cell set to NULL shows as NULL"
        );
        assert_eq!(
            grid.cell_value(1, 0),
            None,
            "an unset cell of a new row is NULL"
        );
        assert_eq!(grid.cell_value(1, 1).as_deref(), Some("Bob"));
        assert_eq!(grid.cell_value(2, 1), None);
    }

    #[test]
    fn test_staged_edits_undo_and_redo() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("name", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager
            .append_row(vec![Some(&b"1"[..]), Some(&b"Alice"[..])])
            .unwrap();

        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&pager));
        grid.stage(|cs| cs.set_cell(0, 1, Some(b"Alice".to_vec()), Some("Alicia".to_string())));
        grid.stage(|cs| cs.insert_row_at(1, vec![None, None]));
        // A change that changes nothing is not an undo step
        grid.stage(|cs| cs.set_cell(0, 1, Some(b"Alice".to_vec()), Some("Alicia".to_string())));

        assert!(grid.step_staged_edits(false));
        assert_eq!(grid.row_count(), 1);
        assert_eq!(grid.cell_value(0, 1).as_deref(), Some("Alicia"));
        assert!(grid.step_staged_edits(false));
        assert_eq!(grid.cell_value(0, 1).as_deref(), Some("Alice"));
        assert!(!grid.step_staged_edits(false));

        assert!(grid.step_staged_edits(true));
        assert_eq!(grid.cell_value(0, 1).as_deref(), Some("Alicia"));
        grid.set_pager(pager);
        assert!(
            !grid.step_staged_edits(false),
            "a new result starts with no history"
        );
    }

    #[test]
    fn test_header_wider_than_cells_sets_the_column_width() {
        let cols = vec![ColumnMeta::new("username", DataType::Text)];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager.append_row(vec![Some(&b"FRONTENDUSER"[..])]).unwrap();
        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(pager);

        // Measured header widths replace the estimate, so the full name fits
        grid.header_widths = vec![240.0];
        grid.recalculate_all_widths();
        assert_eq!(grid.column_widths[0], 240.0);

        // Double-click auto-fit lands on the same width after a manual resize
        grid.set_column_width(0, 80.0);
        grid.auto_fit_single_column(0);
        assert_eq!(grid.column_widths[0], 240.0);
    }

    #[test]
    fn test_widths_follow_rows_streamed_after_schema() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("note", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());

        // The grid receives the pager as soon as the schema is known, before any rows
        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&pager));
        let header_only_width = grid.content_widths[1];

        pager
            .append_row(vec![
                Some(&b"1"[..]),
                Some(&b"A streamed note that is much wider than the column header text"[..]),
            ])
            .unwrap();
        grid.on_rows_appended();
        assert!(
            grid.content_widths[1] > header_only_width,
            "widths must grow with streamed rows"
        );

        // A column the user resized keeps its width while more rows stream in
        grid.set_column_width(1, 90.0);
        pager
            .append_row(vec![
                Some(&b"2"[..]),
                Some(&b"another long streamed note value here"[..]),
            ])
            .unwrap();
        grid.on_rows_appended();
        assert_eq!(grid.column_widths[1], 90.0);
    }

    #[test]
    fn test_auto_fit_content_widths() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("email", DataType::Text),
            ColumnMeta::new("long_description", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager.append_row(vec![
            Some(&b"1"[..]),
            Some(&b"alex@domain.org"[..]),
            Some(&b"This is a substantially longer text description that requires a wider column to display nicely."[..]),
        ]).unwrap();

        let grid = VirtualDataGrid::new(Some(pager), Theme::dark());
        assert_eq!(grid.content_widths.len(), 3);

        // id should be smallest, email medium, long_description widest
        assert!(
            grid.content_widths[0] < grid.content_widths[1],
            "id should be narrower than email"
        );
        assert!(
            grid.content_widths[1] < grid.content_widths[2],
            "email should be narrower than long_description"
        );

        // Bounds enforcement
        assert!(grid.content_widths[0] >= MIN_COL_WIDTH);
        assert!(grid.content_widths[2] <= MAX_AUTO_COL_WIDTH);
    }

    #[test]
    fn test_short_columns_stay_snug_without_whitespace() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("key", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager
            .append_row(vec![
                Some(&b"1"[..]),
                Some(&b"io.ubitec.id.service.PHONE_NUMBER_VALIDATION_RULES"[..]),
            ])
            .unwrap();

        let mut grid = VirtualDataGrid::new(Some(pager), Theme::dark());
        grid.viewport_width = 1400.0;
        grid.apply_viewport_distribution();

        // Neither column was capped, so both should stay snug ("khít") to their content
        let key_w = grid.column_widths[1];
        assert!(
            (340.0..=420.0).contains(&key_w),
            "key width ({}) should be snug to 47 chars, not inflated to 600+",
            key_w
        );
        assert!(
            grid.column_widths[0] < 120.0,
            "id width ({}) should be snug",
            grid.column_widths[0]
        );
    }

    #[test]
    fn test_long_content_horizontal_scrolling() {
        let cols = vec![
            ColumnMeta::new("key", DataType::Text),
            ColumnMeta::new("value", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        let long_json = "{\"errorCode\":9001,\"description\":\"Invalid phone number format\",\"regex\":\"^\\\\+?[0-9]{10,14}$\",\"details\":{\"service\":\"validation\",\"version\":\"2.4.1\",\"rules\":[\"E164\",\"NANP\",\"ITU-T\"]}}";
        pager
            .append_row(vec![Some(&b"phone_rules"[..]), Some(long_json.as_bytes())])
            .unwrap();

        let mut grid = VirtualDataGrid::new(Some(pager), Theme::dark());
        grid.viewport_width = 1600.0;
        grid.apply_viewport_distribution();

        // Long data is capped, not stretched into the free room; dragging is the way to widen it
        assert_eq!(grid.column_widths[1], MAX_AUTO_COL_WIDTH);
        grid.set_column_width(1, 900.0);
        grid.update_viewport_width(1000.0);
        assert_eq!(grid.column_widths[1], 900.0);
    }

    #[test]
    fn test_measured_cells_replace_the_estimate() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Uuid),
            ColumnMeta::new("note", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager
            .append_row(vec![
                Some(&b"ec6401e5-514b-4450-98e2-10fb73a65dd6"[..]),
                Some(&b"x"[..]),
            ])
            .unwrap();
        let mut grid = VirtualDataGrid::new(None, Theme::dark());
        grid.set_pager(Arc::clone(&pager));
        assert!(grid.cell_widths_stale);

        // What the window's fonts measured wins over the estimate, wider or narrower
        grid.cell_widths = vec![310.0, 20.0];
        grid.cell_widths_rows = 1;
        grid.cell_widths_stale = false;
        grid.content_widths = grid.calculate_content_widths();
        grid.apply_viewport_distribution();
        assert_eq!(grid.column_widths[0], 310.0);

        // Rows streamed in after the measurement are estimated until measured again
        pager
            .append_row(vec![Some(&b"1"[..]), Some("w".repeat(300).as_bytes())])
            .unwrap();
        grid.on_rows_appended();
        assert!(grid.cell_widths_stale);
        assert_eq!(grid.column_widths[0], 310.0);
        assert_eq!(grid.column_widths[1], MAX_AUTO_COL_WIDTH);
    }

    #[test]
    fn test_manual_column_resize_and_auto_fit() {
        let cols = vec![
            ColumnMeta::new("id", DataType::Int8),
            ColumnMeta::new("name", DataType::Text),
        ];
        let pager = Arc::new(QueryResultPager::new(cols).unwrap());
        pager
            .append_row(vec![Some(&b"1"[..]), Some(&b"Bob"[..])])
            .unwrap();

        let mut grid = VirtualDataGrid::new(Some(pager), Theme::dark());
        grid.viewport_width = 1000.0;
        grid.apply_viewport_distribution();

        // User drags column 0 to 250px
        grid.set_column_width(0, 250.0);
        assert_eq!(grid.column_widths[0], 250.0);
        assert!(grid.manual_column_widths.contains(&0));

        // Viewport resize shouldn't override manual column 0 width
        grid.update_viewport_width(1400.0);
        assert_eq!(grid.column_widths[0], 250.0);

        // Double-click auto-fit resets manual width
        grid.auto_fit_single_column(0);
        assert!(!grid.manual_column_widths.contains(&0));
    }
}

#[cfg(test)]
mod scrollbar_tests {
    use super::super::scrollbars::vertical_thumb;

    #[test]
    fn thumb_reaches_both_ends_of_actual_track() {
        // The track includes the header, while the list viewport does not.
        let track = 468.0;
        let viewport = 472.0;
        let max_scroll = 5200.0 - viewport;
        let (top, height) = vertical_thumb(track, viewport, max_scroll, 0.0);
        assert_eq!(top, 0.0);
        let (bottom_top, _) = vertical_thumb(track, viewport, max_scroll, max_scroll);
        assert!((bottom_top + height - track).abs() < 0.001);
        let (middle, _) = vertical_thumb(track, viewport, max_scroll, max_scroll / 2.0);
        assert!((middle * 2.0 - bottom_top).abs() < 0.001);
    }

    #[test]
    fn thumb_stays_inside_short_tracks_and_clamps_offsets() {
        assert_eq!(vertical_thumb(20.0, 50.0, 100.0, 100.0), (0.0, 20.0));
        assert_eq!(vertical_thumb(0.0, 0.0, 0.0, 0.0), (0.0, 0.0));
        assert_eq!(vertical_thumb(100.0, 50.0, 100.0, -10.0).0, 0.0);
        let (top, height) = vertical_thumb(100.0, 50.0, 100.0, 200.0);
        assert!((top + height - 100.0).abs() < 0.001);
    }
}
