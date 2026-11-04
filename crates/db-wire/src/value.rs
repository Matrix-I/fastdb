//! Exact binary value representation shared by editors and dialect DML.
use crate::DbDriverKind;

pub fn hex_text(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(2 + bytes.len() * 2);
    text.push_str("\\x");
    for byte in bytes {
        use std::fmt::Write;
        write!(text, "{byte:02x}").expect("writing to a String cannot fail");
    }
    text
}

pub fn parse_hex(text: &str) -> anyhow::Result<Vec<u8>> {
    let text = text
        .strip_prefix("\\x")
        .ok_or_else(|| anyhow::anyhow!("Binary edits require a \\x-prefixed hex value"))?;
    anyhow::ensure!(
        text.len().is_multiple_of(2),
        "Binary hex must contain complete byte pairs"
    );
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| char::from(byte).to_digit(16).map(|value| value as u8);
            digit(pair[0])
                .zip(digit(pair[1]))
                .map(|(high, low)| high * 16 + low)
                .ok_or_else(|| anyhow::anyhow!("Invalid binary hex digit"))
        })
        .collect()
}

pub fn source_binary(bytes: &[u8], provider: DbDriverKind) -> anyhow::Result<Vec<u8>> {
    if provider == DbDriverKind::Postgres || provider.uses_odbc() {
        parse_hex(std::str::from_utf8(bytes)?)
    } else {
        Ok(bytes.to_vec())
    }
}

pub fn binary_literal(bytes: &[u8], provider: DbDriverKind) -> String {
    let text = hex_text(bytes);
    let hex = &text[2..];
    match provider {
        DbDriverKind::Postgres => format!("pg_catalog.decode('{hex}', 'hex')"),
        DbDriverKind::SqlServer => format!("0x{hex}"),
        DbDriverKind::Oracle => format!("HEXTORAW('{hex}')"),
        _ => format!("X'{hex}'"),
    }
}

#[cfg(test)]
mod tests {
    use super::{hex_text, parse_hex, source_binary};
    use crate::DbDriverKind;

    #[test]
    fn binary_representation_roundtrips_every_byte_and_preserves_literal_prefix_bytes() {
        let bytes: Vec<_> = (0..=255).collect();
        assert_eq!(parse_hex(&hex_text(&bytes)).unwrap(), bytes);
        assert_eq!(parse_hex("\\x").unwrap(), b"");
        assert!(parse_hex("\\xa").is_err());
        assert!(parse_hex("\\xno").is_err());
        assert_eq!(
            source_binary(b"\\xdead", DbDriverKind::Sqlite).unwrap(),
            b"\\xdead"
        );
        assert_eq!(
            source_binary(b"\\xdead", DbDriverKind::Postgres).unwrap(),
            [0xde, 0xad]
        );
    }
}

#[cfg(test)]
mod dialect_tests {
    use super::binary_literal;
    use crate::DbDriverKind;

    #[test]
    fn binary_literals_use_native_provider_syntax() {
        for (provider, expected) in [
            (DbDriverKind::Postgres, "pg_catalog.decode('00ff', 'hex')"),
            (DbDriverKind::MySql, "X'00ff'"),
            (DbDriverKind::MariaDb, "X'00ff'"),
            (DbDriverKind::Sqlite, "X'00ff'"),
            (DbDriverKind::Db2, "X'00ff'"),
            (DbDriverKind::Oracle, "HEXTORAW('00ff')"),
            (DbDriverKind::SqlServer, "0x00ff"),
        ] {
            assert_eq!(binary_literal(&[0, 255], provider), expected);
        }
    }
}
