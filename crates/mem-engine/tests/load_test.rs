use mem_engine::*;

#[test]
fn test_large_dataset_spill_and_accuracy() {
    let mut cols = Vec::new();
    for i in 0..15 {
        cols.push(ColumnMeta::new(format!("col_{}", i), DataType::Text));
    }

    // Limit cache to 128 resident pages so spilling triggers rapidly
    let pager = QueryResultPager::new(cols)
        .unwrap()
        .with_max_resident_pages(128);

    // Pump 100,000 rows x 15 columns in this unit test to run quickly and thoroughly
    let total_test_rows = 100_000u64;

    for r in 0..total_test_rows {
        let r_str = r.to_string();
        let bytes = r_str.as_bytes();
        let row: Vec<Option<&[u8]>> = (0..15)
            .map(|c| {
                if (r + c as u64) % 7 == 0 {
                    None
                } else {
                    Some(bytes)
                }
            })
            .collect();
        pager.append_row(row).unwrap();
    }

    assert_eq!(pager.total_rows(), total_test_rows);

    // Verify random access across all rows
    let step = 1379u64; // arbitrary prime step
    for r in (0..total_test_rows).step_by(step as usize) {
        let r_str = r.to_string();
        for c in 0..15 {
            let expected_null = (r + c as u64) % 7 == 0;
            let val = pager.get_cell(r, c);
            if expected_null {
                assert_eq!(val, None, "Expected NULL at row {}, col {}", r, c);
            } else {
                assert!(val.is_some(), "Expected value at row {}, col {}", r, c);
                assert_eq!(
                    std::str::from_utf8(&val.unwrap()).unwrap(),
                    r_str,
                    "Mismatch at row {}, col {}",
                    r,
                    c
                );
            }
        }
    }
}
