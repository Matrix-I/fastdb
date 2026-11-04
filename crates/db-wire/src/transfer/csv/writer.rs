use super::{CsvOptions, Encoding, MAX_COLUMNS, MAX_FIELD_BYTES, MAX_RECORD_BYTES};
use anyhow::{Result, anyhow, ensure};
use std::io::{BufWriter, Write};

pub struct CsvWriter<W: Write> {
    writer: BufWriter<W>,
    options: CsvOptions,
    width: usize,
}
impl<W: Write> CsvWriter<W> {
    pub fn new(writer: W, options: CsvOptions, headers: &[String]) -> Result<Self> {
        options.validate()?;
        ensure!(
            !headers.is_empty() && headers.len() <= MAX_COLUMNS,
            "CSV output requires 1–1,024 columns"
        );
        let mut csv = Self {
            writer: BufWriter::new(writer),
            options,
            width: headers.len(),
        };
        if csv.options.header {
            let header: Vec<_> = headers.iter().map(|name| Some(name.as_str())).collect();
            csv.write_record(&header)?;
        }
        Ok(csv)
    }

    pub fn write_record(&mut self, fields: &[Option<&str>]) -> Result<()> {
        ensure!(
            fields.len() == self.width,
            "Expected {} CSV output fields, found {}",
            self.width,
            fields.len()
        );
        let mut bytes = fields.len().saturating_sub(1); // Delimiters; the record terminator is excluded.
        for field in fields {
            let text = match field {
                Some(text) => text,
                None => self
                    .options
                    .null_token
                    .as_deref()
                    .ok_or_else(|| anyhow!("Choose a CSV NULL token to export SQL NULL"))?,
            };
            ensure!(text.len() <= MAX_FIELD_BYTES, "CSV field exceeds 256 KiB");
            bytes = bytes.saturating_add(text.len());
            if field.is_some() && quoted(text, &self.options) {
                bytes = bytes.saturating_add(2 + text.bytes().filter(|&byte| byte == b'"').count());
            }
            ensure!(bytes <= MAX_RECORD_BYTES, "CSV record exceeds 1 MiB");
        }
        let delimiter = [self.options.delimiter];
        let delimiter = std::str::from_utf8(&delimiter)?;
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                encoded(&mut self.writer, self.options.encoding, delimiter)?;
            }
            match field {
                None => encoded(
                    &mut self.writer,
                    self.options.encoding,
                    self.options
                        .null_token
                        .as_deref()
                        .expect("NULL token validated before output"),
                )?,
                Some(text) if quoted(text, &self.options) => {
                    encoded(&mut self.writer, self.options.encoding, "\"")?;
                    for (index, part) in text.split('"').enumerate() {
                        if index > 0 {
                            encoded(&mut self.writer, self.options.encoding, "\"\"")?;
                        }
                        encoded(&mut self.writer, self.options.encoding, part)?;
                    }
                    encoded(&mut self.writer, self.options.encoding, "\"")?;
                }
                Some(text) => encoded(&mut self.writer, self.options.encoding, text)?,
            }
        }
        encoded(&mut self.writer, self.options.encoding, "\n")
    }

    pub fn finish(mut self) -> Result<W> {
        self.writer.flush()?;
        Ok(self
            .writer
            .into_inner()
            .map_err(|error| error.into_error())?)
    }
}

fn quoted(text: &str, options: &CsvOptions) -> bool {
    options.null_token.as_deref() == Some(text)
        || text.starts_with(['\u{feff}', '\u{fffe}'])
        || text
            .bytes()
            .any(|byte| [options.delimiter, b'"', b'\r', b'\n'].contains(&byte))
}

fn encoded(writer: &mut impl Write, encoding: Encoding, text: &str) -> Result<()> {
    match encoding {
        Encoding::Utf8 => writer.write_all(text.as_bytes())?,
        Encoding::Utf16Le | Encoding::Utf16Be => {
            for unit in text.encode_utf16() {
                let bytes = if encoding == Encoding::Utf16Le {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                };
                writer.write_all(&bytes)?;
            }
        }
    }
    Ok(())
}
