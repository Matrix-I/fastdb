//! Streaming CSV with strict encoding and bounded field/record allocation.
mod encoding;
mod writer;
use anyhow::{Result, anyhow, bail, ensure};
use encoding::Decoded;
pub use encoding::Encoding;
use std::io::Read;
pub use writer::CsvWriter;

pub const MAX_FIELD_BYTES: usize = 256 * 1024;
/// UTF-8 record content, including quoting/delimiters but excluding its terminator.
pub const MAX_RECORD_BYTES: usize = 1024 * 1024;
pub const MAX_COLUMNS: usize = 1024;
const MAX_NULL_TOKEN_BYTES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CsvOptions {
    pub delimiter: u8,
    pub encoding: Encoding,
    pub header: bool,
    /// Only an unquoted field can represent NULL; quoted tokens remain values.
    pub null_token: Option<String>,
}
impl Default for CsvOptions {
    fn default() -> Self {
        Self {
            delimiter: b',',
            encoding: Encoding::Utf8,
            header: true,
            null_token: Some("\\N".into()),
        }
    }
}
impl CsvOptions {
    fn validate(&self) -> Result<()> {
        ensure!(
            (self.delimiter.is_ascii_graphic() || b" \t".contains(&self.delimiter))
                && self.delimiter != b'"',
            "CSV delimiter must be a printable ASCII byte or tab, excluding quote"
        );
        if let Some(token) = &self.null_token {
            ensure!(
                !token.starts_with(['\u{feff}', '\u{fffe}']),
                "CSV NULL token cannot start with a byte order mark"
            );
            ensure!(
                token.len() <= MAX_NULL_TOKEN_BYTES,
                "CSV NULL token exceeds 4 KiB"
            );
            ensure!(
                !token
                    .bytes()
                    .any(|byte| [self.delimiter, b'"', b'\r', b'\n', 0].contains(&byte)),
                "CSV NULL token must fit in an unquoted field"
            );
        }
        Ok(())
    }
}

struct Field {
    text: String,
    quoted: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Start,
    Plain,
    Quoted,
    AfterQuote,
}

pub struct CsvReader<R> {
    input: Decoded<R>,
    options: CsvOptions,
    headers: Option<Vec<String>>,
    width: Option<usize>,
    record: u64,
    skip_lf: bool,
    failed: bool,
}
impl<R: Read> CsvReader<R> {
    pub fn new(reader: R, options: CsvOptions) -> Result<Self> {
        options.validate()?;
        let mut csv = Self {
            input: Decoded::new(reader, options.encoding),
            options,
            headers: None,
            width: None,
            record: 0,
            skip_lf: false,
            failed: false,
        };
        if csv.options.header {
            let header = csv
                .raw_record()?
                .ok_or_else(|| anyhow!("CSV record 1: header is missing"))?;
            csv.headers = Some(header.into_iter().map(|field| field.text).collect());
        }
        Ok(csv)
    }

    pub fn headers(&self) -> Option<&[String]> {
        self.headers.as_deref()
    }
    pub fn record_number(&self) -> u64 {
        self.record
    }

    pub fn next_record(&mut self) -> Result<Option<Vec<Option<String>>>> {
        Ok(self.raw_record()?.map(|fields| {
            fields
                .into_iter()
                .map(|field| {
                    if !field.quoted
                        && self.options.null_token.as_deref() == Some(field.text.as_str())
                    {
                        None
                    } else {
                        Some(field.text)
                    }
                })
                .collect()
        }))
    }

    fn raw_record(&mut self) -> Result<Option<Vec<Field>>> {
        ensure!(!self.failed, "CSV reader stopped after an earlier error");
        let number = self
            .record
            .checked_add(1)
            .ok_or_else(|| anyhow!("CSV record count overflow"))?;
        let result = self.read_record().and_then(|record| {
            if let Some(fields) = &record {
                if let Some(width) = self.width {
                    ensure!(
                        fields.len() == width,
                        "Expected {width} fields, found {}",
                        fields.len()
                    );
                }
                self.width = Some(fields.len());
                self.record = number;
            }
            Ok(record)
        });
        if result.is_err() {
            self.failed = true;
        }
        result.map_err(|error| {
            let message = format!("CSV record {number}: {error}");
            error.context(message)
        })
    }

    fn read_record(&mut self) -> Result<Option<Vec<Field>>> {
        let mut fields = Vec::new();
        let mut field = Vec::new();
        let mut quoted = false;
        let mut state = State::Start;
        let mut bytes = 0usize;
        let mut touched = false;
        loop {
            let Some(byte) = self.input.next()? else {
                ensure!(state != State::Quoted, "Unterminated quoted field");
                if !touched {
                    return Ok(None);
                }
                finish_field(&mut fields, &mut field, quoted)?;
                return Ok(Some(fields));
            };
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            touched = true;
            if state == State::Quoted || !b"\r\n".contains(&byte) {
                bytes += 1;
                ensure!(bytes <= MAX_RECORD_BYTES, "CSV record exceeds 1 MiB");
            }
            if state == State::Quoted {
                if byte == b'"' {
                    state = State::AfterQuote;
                } else {
                    push_byte(&mut field, byte)?;
                }
                continue;
            }
            if state == State::AfterQuote && byte == b'"' {
                push_byte(&mut field, byte)?;
                state = State::Quoted;
                continue;
            }
            if byte == self.options.delimiter {
                finish_field(&mut fields, &mut field, quoted)?;
                state = State::Start;
                quoted = false;
            } else if byte == b'\n' || byte == b'\r' {
                self.skip_lf = byte == b'\r';
                finish_field(&mut fields, &mut field, quoted)?;
                return Ok(Some(fields));
            } else if byte == b'"' && state == State::Start {
                state = State::Quoted;
                quoted = true;
            } else {
                if state == State::AfterQuote {
                    bail!("Unexpected bytes after closing quote");
                }
                ensure!(byte != b'"', "Quote inside an unquoted field");
                push_byte(&mut field, byte)?;
                state = State::Plain;
            }
        }
    }
}

fn push_byte(field: &mut Vec<u8>, byte: u8) -> Result<()> {
    ensure!(field.len() < MAX_FIELD_BYTES, "CSV field exceeds 256 KiB");
    field.push(byte);
    Ok(())
}
fn finish_field(fields: &mut Vec<Field>, bytes: &mut Vec<u8>, quoted: bool) -> Result<()> {
    ensure!(
        fields.len() < MAX_COLUMNS,
        "CSV record exceeds 1,024 fields"
    );
    fields.push(Field {
        text: String::from_utf8(std::mem::take(bytes))?,
        quoted,
    });
    Ok(())
}

#[cfg(test)]
mod tests;
