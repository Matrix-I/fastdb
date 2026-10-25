use std::error::Error;
use std::fmt::Write as _;
use std::net::{Ipv4Addr, Ipv6Addr};

use mem_engine::DataType;
use postgres_types::{FromSql, Kind};
use tokio_postgres::types::Type;
use tokio_postgres::Row;

pub fn pg_type_to_data_type(t: &Type) -> DataType {
    match *t {
        Type::BOOL => DataType::Bool,
        Type::INT2 => DataType::Int2,
        Type::INT4 => DataType::Int4,
        Type::INT8 => DataType::Int8,
        Type::FLOAT4 => DataType::Float4,
        Type::FLOAT8 => DataType::Float8,
        Type::NUMERIC => DataType::Numeric,
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => DataType::Text,
        Type::DATE => DataType::Date,
        Type::TIME => DataType::Time,
        Type::TIMESTAMP => DataType::Timestamp,
        Type::TIMESTAMPTZ => DataType::TimestampTz,
        Type::UUID => DataType::Uuid,
        Type::JSON | Type::JSONB => DataType::Json,
        Type::BYTEA => DataType::Bytea,
        _ => DataType::Unknown,
    }
}

/// Borrowed binary wire bytes of a non-NULL value, accepted for every Postgres type
struct RawValue<'a>(&'a [u8]);

impl<'a> FromSql<'a> for RawValue<'a> {
    fn from_sql(_ty: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn Error + Sync + Send>> {
        Ok(RawValue(raw))
    }

    fn accepts(_ty: &Type) -> bool {
        true
    }
}

/// Decodes a single cell from a tokio-postgres Row into its display byte representation.
/// Returns None only for SQL NULL; values of unsupported types are never reported as NULL.
pub fn decode_pg_cell(row: &Row, col_idx: usize) -> Option<Vec<u8>> {
    let col_type = row.columns()[col_idx].type_();
    match row.try_get::<_, Option<RawValue>>(col_idx) {
        Ok(Some(raw)) => Some(format_pg_value(col_type, raw.0).into_bytes()),
        Ok(None) => None,
        Err(e) => Some(format!("<decode error: {e}>").into_bytes()),
    }
}

/// Formats the binary wire representation of a non-NULL value as display text
pub fn format_pg_value(ty: &Type, raw: &[u8]) -> String {
    try_format_pg_value(ty, raw).unwrap_or_else(|_| format_unknown(raw))
}

type DecodeResult = Result<String, Box<dyn Error + Sync + Send>>;

fn try_format_pg_value(ty: &Type, raw: &[u8]) -> DecodeResult {
    match *ty {
        Type::BOOL => Ok(bool::from_sql(ty, raw)?.to_string()),
        Type::INT2 => Ok(i16::from_sql(ty, raw)?.to_string()),
        Type::INT4 => Ok(i32::from_sql(ty, raw)?.to_string()),
        Type::INT8 => Ok(i64::from_sql(ty, raw)?.to_string()),
        Type::FLOAT4 => Ok(f32::from_sql(ty, raw)?.to_string()),
        Type::FLOAT8 => Ok(f64::from_sql(ty, raw)?.to_string()),
        Type::NUMERIC => format_numeric(raw),
        Type::OID | Type::XID | Type::CID | Type::REGPROC | Type::REGCLASS | Type::REGTYPE => {
            Ok(u32::from_be_bytes(fixed::<4>(raw)?).to_string())
        }
        Type::CHAR => Ok((fixed::<1>(raw)?[0] as char).to_string()),
        Type::MONEY => Ok(format_money(i64::from_be_bytes(fixed::<8>(raw)?))),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME | Type::XML | Type::JSON => {
            Ok(std::str::from_utf8(raw)?.to_string())
        }
        Type::JSONB => {
            // JSONB binary format: 1-byte version prefix followed by the JSON text
            let (_version, text) = raw.split_first().ok_or("empty jsonb value")?;
            Ok(std::str::from_utf8(text)?.to_string())
        }
        Type::BYTEA => Ok(format_hex(raw)),
        Type::UUID => Ok(uuid::Uuid::from_slice(raw)?.to_string()),
        Type::DATE => match i32::from_be_bytes(fixed::<4>(raw)?) {
            i32::MAX => Ok("infinity".to_string()),
            i32::MIN => Ok("-infinity".to_string()),
            _ => Ok(chrono::NaiveDate::from_sql(ty, raw)?.to_string()),
        },
        Type::TIME => Ok(chrono::NaiveTime::from_sql(ty, raw)?.to_string()),
        Type::TIMESTAMP | Type::TIMESTAMPTZ => match i64::from_be_bytes(fixed::<8>(raw)?) {
            i64::MAX => Ok("infinity".to_string()),
            i64::MIN => Ok("-infinity".to_string()),
            _ if *ty == Type::TIMESTAMP => Ok(chrono::NaiveDateTime::from_sql(ty, raw)?.to_string()),
            _ => Ok(chrono::DateTime::<chrono::Utc>::from_sql(ty, raw)?.to_string()),
        },
        Type::TIMETZ => format_timetz(raw),
        Type::INTERVAL => format_interval(raw),
        Type::INET | Type::CIDR => format_inet(raw),
        Type::MACADDR => Ok(fixed::<6>(raw)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(":")),
        _ => match ty.kind() {
            Kind::Array(elem_ty) => format_array(elem_ty, raw),
            Kind::Domain(base_ty) => try_format_pg_value(base_ty, raw),
            // Enums and text-like extension types (citext, ltree, ...) are sent as UTF-8 text
            _ => Ok(format_unknown(raw)),
        },
    }
}

fn fixed<const N: usize>(raw: &[u8]) -> Result<[u8; N], Box<dyn Error + Sync + Send>> {
    raw.try_into()
        .map_err(|_| format!("expected {N} bytes, got {}", raw.len()).into())
}

struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Box<dyn Error + Sync + Send>> {
        if self.buf.len() < n {
            return Err("truncated binary value".into());
        }
        let (head, tail) = self.buf.split_at(n);
        self.buf = tail;
        Ok(head)
    }

    fn i16(&mut self) -> Result<i16, Box<dyn Error + Sync + Send>> {
        Ok(i16::from_be_bytes(fixed::<2>(self.take(2)?)?))
    }

    fn u16(&mut self) -> Result<u16, Box<dyn Error + Sync + Send>> {
        Ok(u16::from_be_bytes(fixed::<2>(self.take(2)?)?))
    }

    fn i32(&mut self) -> Result<i32, Box<dyn Error + Sync + Send>> {
        Ok(i32::from_be_bytes(fixed::<4>(self.take(4)?)?))
    }

    fn i64(&mut self) -> Result<i64, Box<dyn Error + Sync + Send>> {
        Ok(i64::from_be_bytes(fixed::<8>(self.take(8)?)?))
    }
}

/// NUMERIC binary format: ndigits, weight, sign, dscale (all 16-bit), then base-10000 digits
fn format_numeric(raw: &[u8]) -> DecodeResult {
    let mut r = Reader { buf: raw };
    let ndigits = r.i16()?.max(0) as usize;
    let weight = r.i16()? as i32;
    let sign = r.u16()?;
    let dscale = r.u16()? as usize;
    let digits = (0..ndigits).map(|_| r.i16()).collect::<Result<Vec<_>, _>>()?;

    match sign {
        0xC000 => return Ok("NaN".to_string()),
        0xD000 => return Ok("Infinity".to_string()),
        0xF000 => return Ok("-Infinity".to_string()),
        _ => {}
    }

    let digit_at = |idx: i32| -> i16 {
        if idx >= 0 && (idx as usize) < digits.len() {
            digits[idx as usize]
        } else {
            0
        }
    };

    let mut out = String::new();
    if sign == 0x4000 {
        out.push('-');
    }

    if weight < 0 {
        out.push('0');
    } else {
        for idx in 0..=weight {
            if idx == 0 {
                write!(out, "{}", digit_at(idx))?;
            } else {
                write!(out, "{:04}", digit_at(idx))?;
            }
        }
    }

    if dscale > 0 {
        let mut frac = String::with_capacity(dscale + 4);
        let mut idx = weight + 1;
        while frac.len() < dscale {
            write!(frac, "{:04}", digit_at(idx))?;
            idx += 1;
        }
        frac.truncate(dscale);
        out.push('.');
        out.push_str(&frac);
    }

    Ok(out)
}

fn format_money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    format!("{sign}{}.{:02}", abs / 100, abs % 100)
}

fn format_clock(micros: u64) -> String {
    let secs = micros / 1_000_000;
    let frac = micros % 1_000_000;
    let mut out = format!("{:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60);
    if frac > 0 {
        let frac_str = format!("{frac:06}");
        out.push('.');
        out.push_str(frac_str.trim_end_matches('0'));
    }
    out
}

/// TIMETZ binary format: i64 microseconds since midnight, i32 zone offset in seconds west of UTC
fn format_timetz(raw: &[u8]) -> DecodeResult {
    let mut r = Reader { buf: raw };
    let micros = r.i64()?;
    let zone_west = r.i32()?;
    let offset_east = -zone_west;
    let sign = if offset_east < 0 { '-' } else { '+' };
    let abs = offset_east.unsigned_abs();
    let mut out = format_clock(micros.max(0) as u64);
    write!(out, "{sign}{:02}", abs / 3600)?;
    if abs % 3600 != 0 {
        write!(out, ":{:02}", (abs % 3600) / 60)?;
    }
    Ok(out)
}

/// INTERVAL binary format: i64 microseconds, i32 days, i32 months
fn format_interval(raw: &[u8]) -> DecodeResult {
    let mut r = Reader { buf: raw };
    let micros = r.i64()?;
    let days = r.i32()?;
    let months = r.i32()?;

    let mut parts = Vec::new();
    let (years, mons) = (months / 12, months % 12);
    let plural = |n: i32, one: &str, many: &str| format!("{n} {}", if n.abs() == 1 { one } else { many });
    if years != 0 {
        parts.push(plural(years, "year", "years"));
    }
    if mons != 0 {
        parts.push(plural(mons, "mon", "mons"));
    }
    if days != 0 {
        parts.push(plural(days, "day", "days"));
    }
    if micros != 0 || parts.is_empty() {
        let sign = if micros < 0 { "-" } else { "" };
        parts.push(format!("{sign}{}", format_clock(micros.unsigned_abs())));
    }
    Ok(parts.join(" "))
}

/// INET/CIDR binary format: family, prefix bits, is_cidr flag, address length, address bytes
fn format_inet(raw: &[u8]) -> DecodeResult {
    let mut r = Reader { buf: raw };
    let header = r.take(4)?;
    let (family, bits, is_cidr, len) = (header[0], header[1], header[2] != 0, header[3] as usize);
    let addr = r.take(len)?;
    let (text, max_bits) = match family {
        2 => (Ipv4Addr::from(fixed::<4>(addr)?).to_string(), 32),
        3 => (Ipv6Addr::from(fixed::<16>(addr)?).to_string(), 128),
        _ => return Err(format!("unknown inet family {family}").into()),
    };
    if is_cidr || bits != max_bits {
        Ok(format!("{text}/{bits}"))
    } else {
        Ok(text)
    }
}

/// Array binary format: ndim, has_null flag, element oid, (size, lower bound) per dimension,
/// then each element as an i32 length (-1 for NULL) followed by its bytes
fn format_array(elem_ty: &Type, raw: &[u8]) -> DecodeResult {
    let mut r = Reader { buf: raw };
    let ndim = r.i32()?.max(0) as usize;
    let _has_null = r.i32()?;
    let _elem_oid = r.i32()?;
    if ndim == 0 {
        return Ok("{}".to_string());
    }
    let dims = (0..ndim)
        .map(|_| {
            let size = r.i32()?.max(0) as usize;
            let _lower_bound = r.i32()?;
            Ok(size)
        })
        .collect::<Result<Vec<_>, Box<dyn Error + Sync + Send>>>()?;

    let mut out = String::new();
    format_array_dim(elem_ty, &dims, &mut r, &mut out)?;
    Ok(out)
}

fn format_array_dim(
    elem_ty: &Type,
    dims: &[usize],
    r: &mut Reader<'_>,
    out: &mut String,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    out.push('{');
    for i in 0..dims[0] {
        if i > 0 {
            out.push(',');
        }
        if dims.len() > 1 {
            format_array_dim(elem_ty, &dims[1..], r, out)?;
            continue;
        }
        let len = r.i32()?;
        if len < 0 {
            out.push_str("NULL");
        } else {
            let elem = format_pg_value(elem_ty, r.take(len as usize)?);
            push_array_element(out, &elem);
        }
    }
    out.push('}');
    Ok(())
}

fn push_array_element(out: &mut String, elem: &str) {
    let needs_quotes = elem.is_empty()
        || elem.eq_ignore_ascii_case("NULL")
        || elem
            .chars()
            .any(|c| matches!(c, '{' | '}' | ',' | '"' | '\\') || c.is_whitespace());
    if !needs_quotes {
        out.push_str(elem);
        return;
    }
    out.push('"');
    for c in elem.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
}

fn format_hex(raw: &[u8]) -> String {
    let mut out = String::with_capacity(2 + raw.len() * 2);
    out.push_str("\\x");
    for b in raw {
        let _ = write!(out, "{b:02x}");
    }
    out
}

/// Shows printable UTF-8 as text and anything else as a bytea-style hex literal
fn format_unknown(raw: &[u8]) -> String {
    match std::str::from_utf8(raw) {
        Ok(s) if !s.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r')) => s.to_string(),
        _ => format_hex(raw),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numeric_bytes(weight: i16, sign: u16, dscale: u16, digits: &[i16]) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(&(digits.len() as i16).to_be_bytes());
        buf.extend_from_slice(&weight.to_be_bytes());
        buf.extend_from_slice(&sign.to_be_bytes());
        buf.extend_from_slice(&dscale.to_be_bytes());
        for d in digits {
            buf.extend_from_slice(&d.to_be_bytes());
        }
        buf
    }

    #[test]
    fn test_numeric_decoding() {
        let fmt = |w, s, d, digits: &[i16]| format_pg_value(&Type::NUMERIC, &numeric_bytes(w, s, d, digits));

        assert_eq!(fmt(0, 0x0000, 2, &[123, 4500]), "123.45");
        assert_eq!(fmt(0, 0x4000, 2, &[123, 4500]), "-123.45");
        assert_eq!(fmt(0, 0x0000, 0, &[]), "0");
        assert_eq!(fmt(0, 0x0000, 2, &[]), "0.00");
        assert_eq!(fmt(1, 0x0000, 0, &[1]), "10000");
        assert_eq!(fmt(1, 0x0000, 0, &[12, 3456]), "123456");
        assert_eq!(fmt(-1, 0x0000, 3, &[10]), "0.001");
        assert_eq!(fmt(-2, 0x0000, 6, &[1200]), "0.000012");
        assert_eq!(fmt(-2, 0x0000, 8, &[12]), "0.00000012");
        assert_eq!(fmt(2, 0x0000, 4, &[1, 0, 0, 5]), "100000000.0005");
        assert_eq!(fmt(0, 0xC000, 0, &[]), "NaN");
        assert_eq!(fmt(0, 0xD000, 0, &[]), "Infinity");
        assert_eq!(fmt(0, 0xF000, 0, &[]), "-Infinity");
    }

    #[test]
    fn test_interval_and_time_decoding() {
        let interval = |micros: i64, days: i32, months: i32| {
            let mut buf = micros.to_be_bytes().to_vec();
            buf.extend_from_slice(&days.to_be_bytes());
            buf.extend_from_slice(&months.to_be_bytes());
            format_pg_value(&Type::INTERVAL, &buf)
        };
        assert_eq!(interval(14_706_789_000, 3, 14), "1 year 2 mons 3 days 04:05:06.789");
        assert_eq!(interval(0, 1, 0), "1 day");
        assert_eq!(interval(0, 0, 0), "00:00:00");
        assert_eq!(interval(-3_600_000_000, 0, 0), "-01:00:00");

        let mut timetz = 45_296_000_000i64.to_be_bytes().to_vec(); // 12:34:56
        timetz.extend_from_slice(&(-25_200i32).to_be_bytes()); // UTC+07
        assert_eq!(format_pg_value(&Type::TIMETZ, &timetz), "12:34:56+07");

        assert_eq!(format_pg_value(&Type::MONEY, &(-12_345i64).to_be_bytes()), "-123.45");
        assert_eq!(format_pg_value(&Type::DATE, &i32::MAX.to_be_bytes()), "infinity");
        assert_eq!(format_pg_value(&Type::TIMESTAMPTZ, &i64::MIN.to_be_bytes()), "-infinity");
    }

    #[test]
    fn test_network_and_misc_decoding() {
        assert_eq!(format_pg_value(&Type::INET, &[2, 32, 0, 4, 192, 168, 1, 10]), "192.168.1.10");
        assert_eq!(format_pg_value(&Type::CIDR, &[2, 24, 1, 4, 10, 0, 0, 0]), "10.0.0.0/24");
        let mut v6 = vec![3, 128, 0, 16];
        v6.extend_from_slice(&Ipv6Addr::LOCALHOST.octets());
        assert_eq!(format_pg_value(&Type::INET, &v6), "::1");
        assert_eq!(format_pg_value(&Type::MACADDR, &[8, 0, 0x2b, 1, 2, 3]), "08:00:2b:01:02:03");
        assert_eq!(format_pg_value(&Type::OID, &16_384u32.to_be_bytes()), "16384");
        assert_eq!(format_pg_value(&Type::CHAR, b"r"), "r");
        assert_eq!(format_pg_value(&Type::JSONB, b"\x01{\"a\": 1}"), "{\"a\": 1}");
        assert_eq!(format_pg_value(&Type::BYTEA, &[0xde, 0xad]), "\\xdead");
    }

    #[test]
    fn test_array_decoding() {
        // int4[] = {1,NULL,3}
        let mut buf = Vec::new();
        for v in [1i32, 1, Type::INT4.oid() as i32, 3, 1] {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        for elem in [Some(1i32), None, Some(3)] {
            match elem {
                Some(n) => {
                    buf.extend_from_slice(&4i32.to_be_bytes());
                    buf.extend_from_slice(&n.to_be_bytes());
                }
                None => buf.extend_from_slice(&(-1i32).to_be_bytes()),
            }
        }
        assert_eq!(format_pg_value(&Type::INT4_ARRAY, &buf), "{1,NULL,3}");

        // text[][] = {{a,"b c"},{"",x}}
        let mut buf = Vec::new();
        for v in [2i32, 0, Type::TEXT.oid() as i32, 2, 1, 2, 1] {
            buf.extend_from_slice(&v.to_be_bytes());
        }
        for s in ["a", "b c", "", "x"] {
            buf.extend_from_slice(&(s.len() as i32).to_be_bytes());
            buf.extend_from_slice(s.as_bytes());
        }
        assert_eq!(format_pg_value(&Type::TEXT_ARRAY, &buf), "{{a,\"b c\"},{\"\",x}}");

        let empty = [0i32, 0, Type::INT4.oid() as i32]
            .iter()
            .flat_map(|v| v.to_be_bytes())
            .collect::<Vec<_>>();
        assert_eq!(format_pg_value(&Type::INT4_ARRAY, &empty), "{}");
    }

    #[test]
    fn test_unknown_types_never_decode_as_null() {
        let enum_ty = Type::new(
            "mood".to_string(),
            90_001,
            Kind::Enum(vec!["happy".to_string()]),
            "public".to_string(),
        );
        assert_eq!(format_pg_value(&enum_ty, b"happy"), "happy");

        let opaque = Type::new("opaque".to_string(), 90_002, Kind::Simple, "public".to_string());
        assert_eq!(format_pg_value(&opaque, &[0x00, 0xff]), "\\x00ff");

        // Malformed payload for a known type falls back instead of failing
        assert_eq!(format_pg_value(&Type::INT4, &[0x01]), "\\x01");
    }
}
