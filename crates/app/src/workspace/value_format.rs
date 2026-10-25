//! Formatting of a single cell value for the Value side panel

use mem_engine::DataType;

/// Bytes per line of the binary hex dump
const HEX_DUMP_WIDTH: usize = 16;
const XML_INDENT: &str = "  ";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueFormat {
    Binary,
    Json,
    Text,
    Xml,
}

impl ValueFormat {
    pub const ALL: [ValueFormat; 4] = [ValueFormat::Binary, ValueFormat::Json, ValueFormat::Text, ValueFormat::Xml];

    pub fn label(self) -> &'static str {
        match self {
            ValueFormat::Binary => "Binary",
            ValueFormat::Json => "JSON",
            ValueFormat::Text => "Text",
            ValueFormat::Xml => "XML",
        }
    }

    pub fn glyph(self) -> &'static str {
        match self {
            ValueFormat::Binary => "01",
            ValueFormat::Json => "{ }",
            ValueFormat::Text => "T",
            ValueFormat::Xml => "</>",
        }
    }
}

/// A value rendered for display, split into lines for the numbered gutter
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedValue {
    pub lines: Vec<String>,
    /// Set when the chosen format could not be applied and the raw text is shown instead
    pub error: Option<String>,
}

/// Picks the format a value is shown in when the user has not chosen one
pub fn detect_format(bytes: &[u8], data_type: DataType) -> ValueFormat {
    match data_type {
        DataType::Json => return ValueFormat::Json,
        DataType::Bytea => {
            return match std::str::from_utf8(bytes) {
                Ok(s) if !is_pg_hex_literal(s) && !s.chars().any(|c| c.is_control() && !c.is_whitespace()) => {
                    detect_text_format(s)
                }
                _ => ValueFormat::Binary,
            };
        }
        _ => {}
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => detect_text_format(s),
        Err(_) => ValueFormat::Binary,
    }
}

fn detect_text_format(s: &str) -> ValueFormat {
    let t = s.trim();
    if (t.starts_with('{') && t.ends_with('}')) || (t.starts_with('[') && t.ends_with(']')) {
        if serde_json::from_str::<serde_json::Value>(t).is_ok() {
            return ValueFormat::Json;
        }
    }
    if t.starts_with('<') && t.ends_with('>') && t.len() > 2 {
        return ValueFormat::Xml;
    }
    ValueFormat::Text
}

/// Renders `bytes` in `format`; `auto_format` pretty-prints JSON and XML
pub fn format_value(bytes: &[u8], data_type: DataType, format: ValueFormat, auto_format: bool) -> FormattedValue {
    let text = || String::from_utf8_lossy(bytes).into_owned();
    let (body, error) = match format {
        ValueFormat::Text => (text(), None),
        ValueFormat::Binary => (hex_dump(&binary_bytes(bytes, data_type)), None),
        ValueFormat::Json if auto_format => match serde_json::from_slice::<serde_json::Value>(bytes) {
            Ok(v) => (serde_json::to_string_pretty(&v).unwrap_or_else(|_| text()), None),
            Err(e) => (text(), Some(format!("Invalid JSON: {e}"))),
        },
        ValueFormat::Xml if auto_format => match pretty_xml(&text()) {
            Ok(s) => (s, None),
            Err(e) => (text(), Some(format!("Invalid XML: {e}"))),
        },
        ValueFormat::Json | ValueFormat::Xml => (text(), None),
    };
    FormattedValue { lines: body.lines().map(str::to_owned).collect(), error }
}

/// Postgres sends bytea as a `\x`-prefixed hex literal; other drivers send the raw bytes
fn binary_bytes(bytes: &[u8], data_type: DataType) -> Vec<u8> {
    if data_type == DataType::Bytea {
        if let Ok(s) = std::str::from_utf8(bytes) {
            if is_pg_hex_literal(s) {
                return (2..s.len())
                    .step_by(2)
                    .filter_map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
                    .collect();
            }
        }
    }
    bytes.to_vec()
}

fn is_pg_hex_literal(s: &str) -> bool {
    s.len() >= 2 && s.starts_with("\\x") && s.len() % 2 == 0 && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// Classic `offset  hex bytes  |ascii|` dump
pub fn hex_dump(bytes: &[u8]) -> String {
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(HEX_DUMP_WIDTH).enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(&format!("{:08x}  ", i * HEX_DUMP_WIDTH));
        for j in 0..HEX_DUMP_WIDTH {
            match chunk.get(j) {
                Some(b) => out.push_str(&format!("{b:02x} ")),
                None => out.push_str("   "),
            }
            if j == HEX_DUMP_WIDTH / 2 - 1 {
                out.push(' ');
            }
        }
        out.push_str(" |");
        out.extend(chunk.iter().map(|&b| if b.is_ascii_graphic() || b == b' ' { b as char } else { '.' }));
        out.push('|');
    }
    out
}

/// Re-indents XML one element per line; text-only elements stay on one line
pub fn pretty_xml(input: &str) -> Result<String, String> {
    let tokens = tokenize_xml(input)?;
    let mut out = String::new();
    let mut depth = 0usize;
    let mut stack: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        match tok {
            XmlToken::Open(name, raw) => {
                // <a>text</a> collapses onto a single line
                if let (Some(XmlToken::Text(t)), Some(XmlToken::Close(close, close_raw))) = (tokens.get(i + 1), tokens.get(i + 2)) {
                    if close == name {
                        push_line(&mut out, depth, &format!("{raw}{t}{close_raw}"));
                        i += 3;
                        continue;
                    }
                }
                if let Some(XmlToken::Close(close, close_raw)) = tokens.get(i + 1) {
                    if close == name {
                        push_line(&mut out, depth, &format!("{raw}{close_raw}"));
                        i += 2;
                        continue;
                    }
                }
                push_line(&mut out, depth, raw);
                stack.push(name.clone());
                depth += 1;
            }
            XmlToken::Close(name, raw) => {
                match stack.pop() {
                    Some(open) if &open == name => {}
                    Some(open) => return Err(format!("expected </{open}>, found </{name}>")),
                    None => return Err(format!("unexpected </{name}>")),
                }
                depth -= 1;
                push_line(&mut out, depth, raw);
            }
            XmlToken::Other(raw) | XmlToken::Text(raw) => push_line(&mut out, depth, raw),
        }
        i += 1;
    }
    if let Some(open) = stack.pop() {
        return Err(format!("unclosed <{open}>"));
    }
    Ok(out)
}

fn push_line(out: &mut String, depth: usize, line: &str) {
    if !out.is_empty() {
        out.push('\n');
    }
    out.push_str(&XML_INDENT.repeat(depth));
    out.push_str(line);
}

enum XmlToken {
    /// Start tag: name and raw text
    Open(String, String),
    /// End tag: name and raw text
    Close(String, String),
    /// Self-closing tag, comment, declaration, CDATA or processing instruction
    Other(String),
    Text(String),
}

fn tokenize_xml(input: &str) -> Result<Vec<XmlToken>, String> {
    let mut tokens = Vec::new();
    let mut rest = input;
    while !rest.is_empty() {
        if !rest.starts_with('<') {
            let end = rest.find('<').unwrap_or(rest.len());
            let text = rest[..end].trim();
            if !text.is_empty() {
                tokens.push(XmlToken::Text(text.to_string()));
            }
            rest = &rest[end..];
            continue;
        }
        let terminator = if rest.starts_with("<!--") {
            "-->"
        } else if rest.starts_with("<![CDATA[") {
            "]]>"
        } else if rest.starts_with("<?") {
            "?>"
        } else {
            ">"
        };
        let end = find_tag_end(rest, terminator).ok_or_else(|| "unterminated tag".to_string())?;
        let raw = &rest[..end];
        rest = &rest[end..];
        if terminator != ">" || raw.starts_with("<!") || raw.ends_with("/>") {
            tokens.push(XmlToken::Other(raw.to_string()));
        } else if let Some(name) = raw.strip_prefix("</") {
            tokens.push(XmlToken::Close(tag_name(name), raw.to_string()));
        } else {
            tokens.push(XmlToken::Open(tag_name(&raw[1..]), raw.to_string()));
        }
    }
    Ok(tokens)
}

/// Byte index just past `terminator`, skipping `>` inside quoted attribute values
fn find_tag_end(s: &str, terminator: &str) -> Option<usize> {
    if terminator != ">" {
        return s.find(terminator).map(|i| i + terminator.len());
    }
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i + 1),
            _ => {}
        }
    }
    None
}

fn tag_name(s: &str) -> String {
    s.trim_end_matches('>')
        .split(|c: char| c.is_whitespace() || c == '/')
        .next()
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_format_from_content_and_type() {
        assert_eq!(detect_format(br#"[{"a":1}]"#, DataType::Text), ValueFormat::Json);
        assert_eq!(detect_format(b"{not json}", DataType::Text), ValueFormat::Text);
        assert_eq!(detect_format(b"<a><b/></a>", DataType::Varchar), ValueFormat::Xml);
        assert_eq!(detect_format(b"hello", DataType::Text), ValueFormat::Text);
        assert_eq!(detect_format(b"broken", DataType::Json), ValueFormat::Json);
        assert_eq!(detect_format(b"\\xdead", DataType::Bytea), ValueFormat::Binary);
        assert_eq!(detect_format(&[0xff, 0x00], DataType::Unknown), ValueFormat::Binary);
    }

    #[test]
    fn pretty_prints_json_keeping_key_order() {
        let v = format_value(br#"{"z":1,"a":{"de":"Schweiz"}}"#, DataType::Json, ValueFormat::Json, true);
        assert_eq!(v.lines, vec!["{", "  \"z\": 1,", "  \"a\": {", "    \"de\": \"Schweiz\"", "  }", "}"]);
        assert_eq!(v.error, None);
    }

    #[test]
    fn invalid_json_falls_back_to_raw_text() {
        let v = format_value(b"{oops", DataType::Text, ValueFormat::Json, true);
        assert_eq!(v.lines, vec!["{oops"]);
        assert!(v.error.unwrap().starts_with("Invalid JSON"));
    }

    #[test]
    fn auto_format_off_keeps_raw_text() {
        let v = format_value(br#"{"a":1}"#, DataType::Json, ValueFormat::Json, false);
        assert_eq!(v.lines, vec![r#"{"a":1}"#]);
    }

    #[test]
    fn pretty_prints_xml() {
        let xml = r#"<?xml version="1.0"?><root a="x>y"><item>one</item><empty/><!-- c --><n></n></root>"#;
        assert_eq!(
            pretty_xml(xml).unwrap(),
            "<?xml version=\"1.0\"?>\n<root a=\"x>y\">\n  <item>one</item>\n  <empty/>\n  <!-- c -->\n  <n></n>\n</root>"
        );
        assert!(pretty_xml("<a><b></a>").is_err());
        assert!(pretty_xml("<a>").is_err());
    }

    #[test]
    fn hex_dumps_postgres_bytea_literal() {
        let v = format_value(b"\\x48656c6c6f00", DataType::Bytea, ValueFormat::Binary, true);
        assert_eq!(
            v.lines,
            vec!["00000000  48 65 6c 6c 6f 00                                 |Hello.|"]
        );
    }

    #[test]
    fn hex_dump_wraps_every_sixteen_bytes() {
        let dump = hex_dump(&(0u8..20).collect::<Vec<_>>());
        let lines: Vec<&str> = dump.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].starts_with("00000010  10 11 12 13 "));
    }
}
