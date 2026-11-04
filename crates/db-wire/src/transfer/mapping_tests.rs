use super::{ColumnMapping, Conversion, Mapping, Value};
use crate::{ColumnMeta, DataType};

fn column(source: usize, name: &str, kind: DataType) -> ColumnMapping {
    ColumnMapping {
        source,
        target: ColumnMeta::new(name, kind),
        conversion: Conversion::Auto,
    }
}

#[test]
fn maps_selected_columns_preserving_exact_decimal_binary_empty_and_null() {
    let mapping = Mapping::new(
        5,
        vec![
            column(3, "amount", DataType::Numeric),
            column(1, "bytes", DataType::Bytea),
            column(0, "text", DataType::Text),
            column(4, "optional", DataType::Text),
        ],
    )
    .unwrap();
    let decimal = "-12345678901234567890.12345678901234567890e-4";
    let values = [
        Value::Text("".into()),
        Value::Text("\\x5c786465616400ff".into()),
        Value::Text("ignored".into()),
        Value::Text(decimal.into()),
        Value::Null,
    ];
    assert_eq!(
        mapping.convert(&values).unwrap(),
        vec![
            Value::Text(decimal.into()),
            Value::Binary(b"\\xdead\0\xff".to_vec()),
            Value::Text("".into()),
            Value::Null
        ]
    );
    assert_eq!(
        mapping
            .columns()
            .iter()
            .map(|column| column.target.name.as_str())
            .collect::<Vec<_>>(),
        ["amount", "bytes", "text", "optional"]
    );
}

#[test]
fn rejects_invalid_indices_duplicate_targets_width_and_required_null() {
    assert!(Mapping::new(1, vec![column(1, "value", DataType::Text)]).is_err());
    assert!(
        Mapping::new(
            1,
            vec![
                column(0, "same", DataType::Text),
                column(0, "same", DataType::Text)
            ]
        )
        .is_err()
    );
    assert!(Mapping::new(1, vec![]).is_err());
    let mut required = column(0, "required", DataType::Int4);
    required.target.nullable = false;
    let mapping = Mapping::new(1, vec![required]).unwrap();
    assert!(mapping.convert(&[]).is_err());
    assert!(
        mapping
            .convert(&[Value::Null])
            .unwrap_err()
            .to_string()
            .contains("required")
    );
    assert!(mapping.convert(&[Value::Text("".into())]).is_err());
}

#[test]
fn validates_typed_values_without_host_timezone_or_numeric_rounding() {
    for (kind, valid, invalid) in [
        (DataType::Bool, "TRUE", "yes"),
        (DataType::Int2, "32767", "32768"),
        (DataType::Int4, "2147483647", "2147483648"),
        (DataType::Int8, "9223372036854775807", "9223372036854775808"),
        (DataType::Numeric, "+0.123456789012345678901e-40", "1e"),
        (DataType::Float4, "1.5e2", "1e100"),
        (DataType::Float8, "1e200", "NaN"),
        (DataType::Date, "2024-02-29", "2025-02-29"),
        (DataType::Time, "23:59:59.123", "24:01:00"),
        (DataType::Timestamp, "2026-11-04 01:02:03.123", "tomorrow"),
        (
            DataType::TimestampTz,
            "2026-11-04T01:02:03+07:00",
            "2026-11-04T01:02:03",
        ),
        (
            DataType::Json,
            r#"{"n":0.123456789012345678901,"k":1,"k":2}"#,
            "{bad}",
        ),
        (
            DataType::Uuid,
            "00000000-0000-4000-8000-000000000001",
            "not a uuid",
        ),
    ] {
        let mapping = Mapping::new(1, vec![column(0, "value", kind)]).unwrap();
        let converted = mapping.convert(&[Value::Text(valid.into())]).unwrap();
        assert_eq!(
            converted[0],
            if kind == DataType::Bool {
                Value::Boolean(true)
            } else {
                Value::Text(valid.into())
            }
        );
        assert!(
            mapping.convert(&[Value::Text(invalid.into())]).is_err(),
            "{kind}"
        );
    }
}

#[test]
fn explicit_text_conversion_and_unsigned_integer_metadata_remain_exact() {
    let mut text = column(0, "value", DataType::Text);
    let mapping = Mapping::new(1, vec![text.clone()]).unwrap();
    assert!(mapping.convert(&[Value::Binary(vec![0, 255])]).is_err());
    text.conversion = Conversion::Text;
    let mapping = Mapping::new(1, vec![text]).unwrap();
    assert_eq!(
        mapping.convert(&[Value::Binary(vec![0, 255])]).unwrap(),
        [Value::Text("\\x00ff".into())]
    );
    let mut unsigned = column(0, "id", DataType::Int8);
    unsigned.target.raw_type = Some("bigint unsigned".into());
    let mapping = Mapping::new(1, vec![unsigned]).unwrap();
    assert!(
        mapping
            .convert(&[Value::Text(u64::MAX.to_string())])
            .is_ok()
    );
    assert!(mapping.convert(&[Value::Text("-1".into())]).is_err());
}

#[test]
fn json_validation_preserves_numbers_outside_binary_float_range() {
    let mapping = Mapping::new(1, vec![column(0, "document", DataType::Json)]).unwrap();
    let json = r#"{"huge":1e999,"precise":0.12345678901234567890123456789}"#;
    assert_eq!(
        mapping.convert(&[Value::Text(json.into())]).unwrap(),
        [Value::Text(json.into())]
    );
}

#[test]
fn mapping_checks_both_input_and_expanded_output_budgets() {
    use crate::transfer::csv::{MAX_COLUMNS, MAX_FIELD_BYTES};
    let mapping = Mapping::new(1, vec![column(0, "value", DataType::Text)]).unwrap();
    assert!(
        mapping
            .convert(&[Value::Text("x".repeat(MAX_FIELD_BYTES + 1))])
            .is_err()
    );
    assert!(Mapping::new(MAX_COLUMNS + 1, vec![column(0, "value", DataType::Text)]).is_err());
    let columns = (0..5)
        .map(|index| column(0, &format!("value_{index}"), DataType::Text))
        .collect();
    let mapping = Mapping::new(1, columns).unwrap();
    assert!(
        mapping
            .convert(&[Value::Text("x".repeat(MAX_FIELD_BYTES))])
            .is_err()
    );
    let mut text = column(0, "encoded", DataType::Text);
    text.conversion = Conversion::Text;
    let mapping = Mapping::new(1, vec![text]).unwrap();
    assert!(
        mapping
            .convert(&[Value::Binary(vec![0; MAX_FIELD_BYTES])])
            .is_err()
    );
}

#[test]
fn boolean_sources_use_native_values_and_explicit_json_text() {
    let boolean = Mapping::new(1, vec![column(0, "flag", DataType::Bool)]).unwrap();
    assert_eq!(
        boolean.convert(&[Value::Boolean(false)]).unwrap(),
        [Value::Boolean(false)]
    );
    let json = Mapping::new(1, vec![column(0, "document", DataType::Json)]).unwrap();
    assert_eq!(
        json.convert(&[Value::Boolean(true)]).unwrap(),
        [Value::Text("true".into())]
    );
}

#[test]
fn boolean_numeric_conversions_emit_numeric_values_for_every_destination() {
    for conversion in [Conversion::Integer, Conversion::Decimal, Conversion::Float] {
        for kind in [
            DataType::Text,
            DataType::Json,
            DataType::Int4,
            DataType::Numeric,
        ] {
            let mut target = column(0, "value", kind);
            target.conversion = conversion;
            let mapping = Mapping::new(1, vec![target]).unwrap();
            assert_eq!(
                mapping.convert(&[Value::Boolean(true)]).unwrap(),
                [Value::Text("1".into())]
            );
            assert_eq!(
                mapping.convert(&[Value::Boolean(false)]).unwrap(),
                [Value::Text("0".into())]
            );
        }
    }
}

#[test]
fn integer_boolean_targets_bind_native_boolean_and_reject_unsupported_modes() {
    let mut target = column(0, "flag", DataType::Bool);
    target.conversion = Conversion::Integer;
    let mapping = Mapping::new(1, vec![target]).unwrap();
    assert_eq!(
        mapping.convert(&[Value::Text("1".into())]).unwrap(),
        [Value::Boolean(true)]
    );
    assert_eq!(
        mapping.convert(&[Value::Text("-0".into())]).unwrap(),
        [Value::Boolean(false)]
    );
    assert!(mapping.convert(&[Value::Text("42".into())]).is_err());
    for conversion in [Conversion::Text, Conversion::Decimal, Conversion::Float] {
        let mut target = column(0, "flag", DataType::Bool);
        target.conversion = conversion;
        assert!(Mapping::new(1, vec![target]).is_err());
    }
}
