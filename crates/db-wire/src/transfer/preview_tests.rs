use super::{
    csv::CsvOptions,
    mapping::{ColumnMapping, Conversion, Mapping, Value},
    preview::{PREVIEW_ROWS, Preview},
};
use crate::{ColumnMeta, DataType};

fn mapping() -> Mapping {
    Mapping::new(
        1,
        vec![ColumnMapping {
            source: 0,
            target: ColumnMeta::new("amount", DataType::Int4),
            conversion: Conversion::Auto,
        }],
    )
    .unwrap()
}

#[test]
fn preview_reports_conversion_error_and_preserves_null() {
    let preview = Preview::read(
        &b"amount\n42\nbad\n\\N\n"[..],
        CsvOptions::default(),
        &mapping(),
    )
    .unwrap();
    assert_eq!(preview.headers.unwrap(), ["amount"]);
    assert_eq!(preview.rows[0].record, 2);
    assert_eq!(
        preview.rows[0].converted.as_ref().unwrap(),
        &[Value::Text("42".into())]
    );
    assert!(preview.rows[1].converted.is_err());
    assert_eq!(preview.rows[2].converted.as_ref().unwrap(), &[Value::Null]);
    assert!(preview.reached_end);
}

#[test]
fn preview_never_parses_beyond_the_sample_limit() {
    let input = format!("amount\n{}\"unterminated", "1\n".repeat(PREVIEW_ROWS));
    let preview = Preview::read(input.as_bytes(), CsvOptions::default(), &mapping()).unwrap();
    assert_eq!(preview.rows.len(), PREVIEW_ROWS);
    assert!(!preview.reached_end);
}

#[test]
fn preview_propagates_structural_errors_in_the_sample() {
    let error =
        Preview::read(&b"amount\n1\n2,3\n"[..], CsvOptions::default(), &mapping()).unwrap_err();
    assert!(error.to_string().contains("CSV record 3"));
}
