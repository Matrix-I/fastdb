use std::io::{Read, Write};
use std::path::Path;

pub(super) const MAX_VALUE_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Encoding {
    #[default]
    Utf8,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Utf8 => Self::Utf16Le,
            Self::Utf16Le => Self::Utf16Be,
            Self::Utf16Be => Self::Utf8,
        }
    }
}

pub(super) fn decode_text(bytes: &[u8], encoding: Encoding) -> Result<String, String> {
    if bytes.len() > MAX_VALUE_BYTES * 2 {
        return Err("Encoded value file exceeds 512 KiB".into());
    }
    let text = match encoding {
        Encoding::Utf8 => std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|error| error.to_string())?,
        Encoding::Utf16Le | Encoding::Utf16Be => {
            if !bytes.len().is_multiple_of(2) {
                return Err("UTF-16 requires complete byte pairs".into());
            }
            let units = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    let pair = [pair[0], pair[1]];
                    if encoding == Encoding::Utf16Le {
                        u16::from_le_bytes(pair)
                    } else {
                        u16::from_be_bytes(pair)
                    }
                })
                .collect::<Vec<_>>();
            String::from_utf16(&units).map_err(|error| error.to_string())?
        }
    };
    if text.len() > MAX_VALUE_BYTES {
        return Err("Decoded value exceeds 256 KiB".into());
    }
    Ok(text)
}

pub(super) fn encode_text(text: &str, encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf16Le => text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        Encoding::Utf16Be => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
    }
}

pub(super) fn read_file(path: &Path, binary: bool, encoding: Encoding) -> Result<String, String> {
    if !std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Choose a regular value file".into());
    }
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    if !file
        .metadata()
        .map_err(|error| error.to_string())?
        .is_file()
    {
        return Err("Choose a regular value file".into());
    }
    let limit = if binary {
        MAX_VALUE_BYTES
    } else {
        MAX_VALUE_BYTES * 2
    };
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > limit {
        return Err("Value file exceeds the editor size limit".into());
    }
    if binary {
        Ok(db_wire::value::hex_text(&bytes))
    } else {
        decode_text(&bytes, encoding)
    }
}

pub(super) fn write_file(
    path: &Path,
    text: &str,
    binary: bool,
    encoding: Encoding,
) -> Result<(), String> {
    let bytes = if binary {
        let bytes = db_wire::value::parse_hex(text).map_err(|error| error.to_string())?;
        if bytes.len() > MAX_VALUE_BYTES {
            return Err("Binary value exceeds 256 KiB".into());
        }
        bytes
    } else {
        if text.len() > MAX_VALUE_BYTES {
            return Err("Text value exceeds 256 KiB".into());
        }
        encode_text(text, encoding)
    };
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))
        .map_err(|error| error.to_string())?;
    temporary
        .write_all(&bytes)
        .map_err(|error| error.to_string())?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| error.to_string())?;
    temporary.persist(path).map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Encoding, decode_text, encode_text};

    #[test]
    fn encodings_roundtrip_unicode_empty_and_reject_invalid_bytes() {
        for encoding in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be] {
            for text in ["", "Tiếng Việt\n名\0"] {
                assert_eq!(
                    decode_text(&encode_text(text, encoding), encoding).unwrap(),
                    text
                );
            }
        }
        assert!(decode_text(&[0xff], Encoding::Utf8).is_err());
        assert!(decode_text(&[0xff], Encoding::Utf16Le).is_err());
        assert!(decode_text(&[0x00, 0xd8], Encoding::Utf16Le).is_err());
    }
}

#[cfg(test)]
mod file_tests {
    use super::*;

    #[test]
    fn files_preserve_bytes_encoding_and_limits() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("value");
        for encoding in [Encoding::Utf8, Encoding::Utf16Le, Encoding::Utf16Be] {
            for text in ["", "名\nTiếng Việt"] {
                write_file(&path, text, false, encoding).unwrap();
                assert_eq!(read_file(&path, false, encoding).unwrap(), text);
            }
        }
        let bytes: Vec<u8> = (0..=255).collect();
        let hex = db_wire::value::hex_text(&bytes);
        write_file(&path, &hex, true, Encoding::Utf8).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(read_file(&path, true, Encoding::Utf8).unwrap(), hex);
        assert!(read_file(directory.path(), false, Encoding::Utf8).is_err());
        assert!(write_file(&path, "invalid", true, Encoding::Utf8).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        std::fs::write(&path, vec![0; MAX_VALUE_BYTES + 1]).unwrap();
        assert!(read_file(&path, true, Encoding::Utf8).is_err());
        assert!(read_file(&path, false, Encoding::Utf8).is_err());
    }
}

pub(super) fn timestamp_display(text: &str, utc: bool) -> Result<String, String> {
    if !utc {
        return Ok(text.into());
    }
    db_wire::value::parse_timestamp_with_offset(text)
        .map(|value| value.with_timezone(&chrono::Utc).to_rfc3339())
        .map_err(|error| format!("Cannot display timestamp in UTC: {error}"))
}

#[cfg(test)]
mod timestamp_tests {
    #[test]
    fn utc_display_is_explicit_and_timezone_aware() {
        let source = "2026-11-04T07:30:00+07:00";
        assert_eq!(super::timestamp_display(source, false).unwrap(), source);
        assert_eq!(
            super::timestamp_display(source, true).unwrap(),
            "2026-11-04T00:30:00+00:00"
        );
        assert!(super::timestamp_display("2026-11-04T07:30:00", true).is_err());
    }
}

/// Format JSON whitespace without reserializing numbers or duplicate object keys.
pub(super) fn format_json(text: &str) -> Result<String, String> {
    db_wire::value::validate_json_source(text).map_err(|error| error.to_string())?;
    let mut output = String::new();
    let mut depth = 0usize;
    let mut quoted = false;
    let mut escaped = false;
    for character in text.chars() {
        if quoted {
            output.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                quoted = false;
            }
        } else {
            match character {
                '"' => {
                    quoted = true;
                    output.push(character);
                }
                '{' | '[' => {
                    output.push(character);
                    depth += 1;
                    output.push('\n');
                    output.push_str(&"  ".repeat(depth));
                }
                '}' | ']' => {
                    depth = depth.saturating_sub(1);
                    output.push('\n');
                    output.push_str(&"  ".repeat(depth));
                    output.push(character);
                }
                ',' => {
                    output.push_str(",\n");
                    output.push_str(&"  ".repeat(depth));
                }
                ':' => output.push_str(": "),
                ch if ch.is_whitespace() => {}
                ch => output.push(ch),
            }
        }
        if output.len() > MAX_VALUE_BYTES {
            return Err("Formatted JSON exceeds 256 KiB".into());
        }
    }
    Ok(output)
}

#[cfg(test)]
mod json_tests {
    #[test]
    fn formatting_preserves_numbers_duplicate_keys_and_string_escapes() {
        let input = r#"{"n":1.2345678901234567890123,"n":1e999,"text":"a \" b"}"#;
        // Native JSON accepts these tokens; formatting must not pass through f64.
        assert!(super::format_json(input).unwrap().contains("1e999"));
        assert!(super::format_json(r#"{"n":1e}"#).is_err());
        let input = r#"{"n":1.2345678901234567890123,"n":1e2,"text":"a \" b"}"#;
        let output = super::format_json(input).unwrap();
        assert!(output.contains("1.2345678901234567890123"));
        assert!(output.contains("1e2"));
        assert_eq!(output.matches("\"n\"").count(), 2);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(input).unwrap(),
            serde_json::from_str::<serde_json::Value>(&output).unwrap()
        );
    }
}

#[cfg(test)]
mod native_timestamp_tests {
    #[test]
    fn native_utc_timestamp_display_accepts_postgres_pager_values() {
        assert_eq!(
            super::timestamp_display("2026-11-04 00:08:09.123456 UTC", true).unwrap(),
            "2026-11-04T00:08:09.123456+00:00"
        );
        assert!(super::timestamp_display("2026-11-04 00:08:09", true).is_err());
    }
}

#[cfg(test)]
mod native_json_tests {
    #[test]
    fn native_json_numbers_are_not_limited_to_binary_float_range() {
        let input = r#"{"huge":1e999,"precise":0.12345678901234567890123456789}"#;
        let output = super::format_json(input).unwrap();
        assert!(output.contains("1e999"));
        assert!(output.contains("0.12345678901234567890123456789"));
        assert!(super::format_json(r#"{"bad":1e}"#).is_err());
    }
}
