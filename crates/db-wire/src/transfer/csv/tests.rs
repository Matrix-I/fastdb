use super::{CsvOptions, CsvReader, Encoding, MAX_COLUMNS, MAX_FIELD_BYTES, MAX_RECORD_BYTES};
use std::io::{self, Read};

fn rows(text: &[u8], options: CsvOptions) -> anyhow::Result<Vec<Vec<Option<String>>>> {
    let mut csv = CsvReader::new(text, options)?;
    let mut rows = Vec::new();
    while let Some(row) = csv.next_record()? {
        rows.push(row);
    }
    Ok(rows)
}

#[test]
fn quoting_multiline_crlf_and_null_empty_distinctions() {
    let input = b"a,b,c,d\r\n\\N,\"\\N\",,\"\"\r\n\"line\r\nnext\",\"a\"\"b\",tail,end\r\n";
    let mut csv = CsvReader::new(&input[..], CsvOptions::default()).unwrap();
    assert_eq!(csv.headers().unwrap(), ["a", "b", "c", "d"]);
    assert_eq!(
        csv.next_record().unwrap().unwrap(),
        vec![None, Some("\\N".into()), Some("".into()), Some("".into())]
    );
    assert_eq!(
        csv.next_record().unwrap().unwrap(),
        vec![
            Some("line\r\nnext".into()),
            Some("a\"b".into()),
            Some("tail".into()),
            Some("end".into())
        ]
    );
    assert_eq!(csv.next_record().unwrap(), None);
    assert_eq!(csv.record_number(), 3);
}

struct OneByte<'a>(&'a [u8]);
impl Read for OneByte<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.0.is_empty() || output.is_empty() {
            return Ok(0);
        }
        output[0] = self.0[0];
        self.0 = &self.0[1..];
        Ok(1)
    }
}

#[test]
fn strict_encodings_survive_single_byte_reads_and_optional_boms() {
    let text = "name;value\n名😀;Tiếng Việt\n";
    for encoding in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be] {
        for bom in [false, true] {
            let text = format!("{}{text}", if bom { "\u{feff}" } else { "" });
            let bytes = match encoding {
                Encoding::Utf8 => text.into_bytes(),
                Encoding::Utf16Le => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
                Encoding::Utf16Be => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
            };
            let mut csv = CsvReader::new(
                OneByte(&bytes),
                CsvOptions {
                    delimiter: b';',
                    encoding,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(csv.headers().unwrap(), ["name", "value"]);
            assert_eq!(
                csv.next_record().unwrap().unwrap(),
                vec![Some("名😀".into()), Some("Tiếng Việt".into())]
            );
            assert!(csv.next_record().unwrap().is_none());
        }
    }
    for (encoding, bytes) in [
        (Encoding::Utf8, vec![0xff]),
        (Encoding::Utf16Le, vec![0xff]),
        (Encoding::Utf16Le, vec![0, 0xd8]),
        (Encoding::Utf16Be, vec![0xdc, 0]),
        (Encoding::Utf16Le, vec![0xfe, 0xff]),
    ] {
        assert!(
            rows(
                &bytes,
                CsvOptions {
                    header: false,
                    encoding,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}

#[test]
fn malformed_records_widths_and_limits_fail_with_record_numbers() {
    for input in [
        "a,b\n\"unterminated,x",
        "a,b\nplain\"quote,x",
        "a,b\n\"closed\"junk,x",
        "a,b\none",
        "a,b\none,two,three",
    ] {
        let error = rows(input.as_bytes(), CsvOptions::default()).unwrap_err();
        assert!(error.to_string().contains("record 2"), "{error}");
    }
    let field = "x".repeat(MAX_FIELD_BYTES + 1);
    assert!(
        rows(
            field.as_bytes(),
            CsvOptions {
                header: false,
                ..Default::default()
            }
        )
        .is_err()
    );
    let wide = vec!["x"; MAX_COLUMNS + 1].join(",");
    assert!(
        rows(
            wide.as_bytes(),
            CsvOptions {
                header: false,
                ..Default::default()
            }
        )
        .is_err()
    );
    let record =
        vec!["x".repeat(MAX_FIELD_BYTES); MAX_RECORD_BYTES / MAX_FIELD_BYTES + 1].join(",");
    assert!(
        rows(
            record.as_bytes(),
            CsvOptions {
                header: false,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn headerless_custom_null_blank_records_and_trailing_fields_are_explicit() {
    let options = CsvOptions {
        header: false,
        delimiter: b'\t',
        null_token: Some("NULL".into()),
        ..Default::default()
    };
    assert_eq!(
        rows(b"NULL\t\"NULL\"\t\n", options).unwrap(),
        vec![vec![None, Some("NULL".into()), Some("".into())]]
    );
    let options = CsvOptions {
        header: false,
        null_token: Some("".into()),
        ..Default::default()
    };
    assert_eq!(
        rows(b",\"\"", options).unwrap(),
        vec![vec![None, Some("".into())]]
    );
    assert_eq!(
        rows(
            b"\n",
            CsvOptions {
                header: false,
                ..Default::default()
            }
        )
        .unwrap(),
        vec![vec![Some("".into())]]
    );
    assert!(
        rows(
            b"",
            CsvOptions {
                header: false,
                ..Default::default()
            }
        )
        .unwrap()
        .is_empty()
    );
    assert!(CsvReader::new(&b""[..], CsvOptions::default()).is_err());
    for delimiter in [b'"', 0, b'\n', 0xff] {
        assert!(
            CsvReader::new(
                &b"a"[..],
                CsvOptions {
                    delimiter,
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}

#[test]
fn io_errors_are_not_reported_as_end_of_file() {
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("read failed"))
        }
    }
    assert!(
        CsvReader::new(Broken, CsvOptions::default())
            .err()
            .unwrap()
            .to_string()
            .contains("read failed")
    );
}

#[test]
fn export_and_import_roundtrip_null_tokens_empty_and_unicode_in_every_encoding() {
    use super::CsvWriter;
    let expected = vec![
        vec![
            None,
            Some("\\N".into()),
            Some("".into()),
            Some("名😀\r\n\"quoted\"".into()),
        ],
        vec![
            Some("\u{feff}starts with BOM".into()),
            Some("a;b".into()),
            None,
            Some("tail".into()),
        ],
    ];
    let mut expected = expected;
    expected.insert(
        0,
        vec![
            Some("\u{feff}leading".into()),
            Some("a;b".into()),
            None,
            Some("".into()),
        ],
    );
    expected.insert(
        0,
        vec![
            Some("\u{fffe}literal noncharacter".into()),
            None,
            Some("".into()),
            Some("end".into()),
        ],
    );
    for encoding in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be] {
        for header in [true, false] {
            let options = CsvOptions {
                delimiter: b';',
                encoding,
                header,
                ..Default::default()
            };
            let headers = ["\u{feff}one", "two", "three", "four"].map(str::to_owned);
            let mut writer = CsvWriter::new(Vec::new(), options.clone(), &headers).unwrap();
            for row in &expected {
                writer
                    .write_record(&row.iter().map(Option::as_deref).collect::<Vec<_>>())
                    .unwrap();
            }
            let output = writer.finish().unwrap();
            let csv = CsvReader::new(&output[..], options.clone()).unwrap();
            assert_eq!(csv.headers(), header.then_some(headers.as_slice()));
            assert_eq!(rows(&output, options).unwrap(), expected);
        }
    }
}

#[test]
fn writer_checks_width_limits_null_representation_and_output_failures() {
    use super::CsvWriter;
    let header = ["value".to_owned()];
    let mut writer = CsvWriter::new(Vec::new(), CsvOptions::default(), &header).unwrap();
    assert!(writer.write_record(&[Some("x"), Some("y")]).is_err());
    assert!(
        writer
            .write_record(&[Some(&"x".repeat(MAX_FIELD_BYTES + 1))])
            .is_err()
    );
    let options = CsvOptions {
        header: false,
        null_token: None,
        ..Default::default()
    };
    let mut writer = CsvWriter::new(Vec::new(), options, &header).unwrap();
    assert!(writer.write_record(&[None]).is_err());
    struct Broken;
    impl io::Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("disk full"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("disk full"))
        }
    }
    let writer = CsvWriter::new(Broken, CsvOptions::default(), &header).unwrap();
    assert!(writer.finish().is_err());
}

#[test]
fn null_tokens_cannot_be_confused_with_a_document_bom() {
    for token in ["\u{feff}NULL", "\u{fffe}NULL"] {
        assert!(
            CsvReader::new(
                &b"value\n"[..],
                CsvOptions {
                    null_token: Some(token.into()),
                    ..Default::default()
                }
            )
            .is_err()
        );
    }
}

#[test]
fn utf8_noncharacters_are_valid_text_and_reader_errors_are_terminal() {
    assert_eq!(
        rows(
            "\u{fffe}literal\n".as_bytes(),
            CsvOptions {
                header: false,
                ..Default::default()
            }
        )
        .unwrap(),
        vec![vec![Some("\u{fffe}literal".into())]]
    );
    let mut csv =
        CsvReader::new(&b"header\n\"bad\"junk\ngood\n"[..], CsvOptions::default()).unwrap();
    assert!(csv.next_record().is_err());
    assert!(csv.next_record().is_err());
}

#[test]
fn exact_field_record_and_column_boundaries_roundtrip() {
    use super::CsvWriter;
    let options = CsvOptions {
        header: false,
        ..Default::default()
    };
    let fields = [
        "x".repeat(MAX_FIELD_BYTES),
        "y".repeat(MAX_FIELD_BYTES),
        "z".repeat(MAX_FIELD_BYTES),
        "t".repeat(MAX_FIELD_BYTES - 3),
    ];
    let input = fields.join(",");
    assert_eq!(input.len(), MAX_RECORD_BYTES);
    assert_eq!(
        rows(format!("{input}\r\n").as_bytes(), options.clone()).unwrap()[0].len(),
        4
    );
    assert_eq!(rows(input.as_bytes(), options.clone()).unwrap()[0].len(), 4);
    let headers = ["one", "two", "three", "four"].map(str::to_owned);
    let mut writer = CsvWriter::new(Vec::new(), options.clone(), &headers).unwrap();
    writer
        .write_record(
            &fields
                .iter()
                .map(|field| Some(field.as_str()))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    let output = writer.finish().unwrap();
    assert_eq!(rows(&output, options.clone()).unwrap()[0], fields.map(Some));
    let input = ",".repeat(MAX_COLUMNS - 1);
    assert_eq!(
        rows(input.as_bytes(), options).unwrap()[0].len(),
        MAX_COLUMNS
    );
}

#[test]
fn delayed_io_and_encoding_failures_keep_the_correct_record_number() {
    struct PrefixThenError<'a>(&'a [u8]);
    impl Read for PrefixThenError<'_> {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if self.0.is_empty() {
                return Err(io::Error::other("late read failure"));
            }
            self.0.read(output)
        }
    }
    let mut reader =
        CsvReader::new(PrefixThenError(b"header\nok\n"), CsvOptions::default()).unwrap();
    assert_eq!(
        reader.next_record().unwrap().unwrap(),
        vec![Some("ok".into())]
    );
    let error = reader.next_record().unwrap_err().to_string();
    assert!(
        error.contains("record 3") && error.contains("late read failure"),
        "{error}"
    );
    assert!(reader.next_record().is_err());
    let mut encoded: Vec<_> = "header\nok\n"
        .encode_utf16()
        .flat_map(u16::to_le_bytes)
        .collect();
    encoded.push(0xab);
    let mut reader = CsvReader::new(
        &encoded[..],
        CsvOptions {
            encoding: Encoding::Utf16Le,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(reader.next_record().unwrap().is_some());
    let error = reader.next_record().unwrap_err().to_string();
    assert!(
        error.contains("record 3") && error.contains("UTF-16 byte pair"),
        "{error}"
    );
}

#[test]
fn space_delimiters_preserve_quoted_whitespace() {
    let options = CsvOptions {
        delimiter: b' ',
        header: false,
        ..Default::default()
    };
    assert_eq!(
        rows(b"\" hello world \" plain\n", options).unwrap(),
        vec![vec![Some(" hello world ".into()), Some("plain".into())]]
    );
}
