//! Presentation exports over the loaded result snapshot.
use db_wire::{DataType, DbDriverKind, QueryResultPager, value};
use std::io::Write;

#[derive(Clone, Copy)]
pub(super) enum Markup {
    Html,
    Markdown,
}

impl Markup {
    pub(super) fn write(
        self,
        pager: &QueryResultPager,
        writer: &mut impl Write,
        rows: u64,
        provider: DbDriverKind,
    ) -> anyhow::Result<u64> {
        let rows = rows.min(pager.total_rows());
        match self {
            Self::Html => writer.write_all(
                b"<!doctype html>\n<html lang=\"en\"><meta charset=\"utf-8\">\n\
                  <title>FastDB result</title><style>th,td{white-space:pre-wrap}</style>\n\
                  <table><thead><tr>",
            )?,
            Self::Markdown => writer.write_all(b"|")?,
        }
        for column in &pager.columns {
            self.cell(writer, Some(&column.name), true)?;
        }
        match self {
            Self::Html => writer.write_all(b"</tr></thead>\n<tbody>\n")?,
            Self::Markdown => {
                writer.write_all(b"\n|")?;
                for _ in &pager.columns {
                    writer.write_all(b" --- |")?;
                }
                writer.write_all(b"\n")?;
            }
        }

        for row in 0..rows {
            writer.write_all(match self {
                Self::Html => b"<tr>",
                Self::Markdown => b"|",
            })?;
            for (index, column) in pager.columns.iter().enumerate() {
                let text = pager
                    .get_cell(row, index)
                    .map(|bytes| {
                        if column.data_type == DataType::Bytea {
                            let bytes = value::source_binary(&bytes, provider)?;
                            Ok::<_, anyhow::Error>(value::hex_text(&bytes))
                        } else {
                            Ok(String::from_utf8(bytes)
                                .unwrap_or_else(|error| value::hex_text(error.as_bytes())))
                        }
                    })
                    .transpose()?;
                self.cell(writer, text.as_deref(), false)?;
            }
            writer.write_all(match self {
                Self::Html => b"</tr>\n",
                Self::Markdown => b"\n",
            })?;
        }
        if matches!(self, Self::Html) {
            writer.write_all(b"</tbody></table></html>\n")?;
        }
        Ok(rows)
    }

    fn cell(
        self,
        writer: &mut impl Write,
        text: Option<&str>,
        header: bool,
    ) -> std::io::Result<()> {
        let (start, end, null) = match (self, header) {
            (Self::Html, true) => ("<th>", "</th>", ""),
            (Self::Html, false) => ("<td>", "</td>", "<em>NULL</em>"),
            (Self::Markdown, _) => (" ", " |", "*NULL*"),
        };
        writer.write_all(start.as_bytes())?;
        if let Some(text) = text {
            let mut chars = text.chars().peekable();
            while let Some(ch) = chars.next() {
                match (self, ch) {
                    (Self::Html, '&') => writer.write_all(b"&amp;")?,
                    (Self::Html, '<') => writer.write_all(b"&lt;")?,
                    (Self::Html, '>') => writer.write_all(b"&gt;")?,
                    (Self::Markdown, '\r' | '\n') => {
                        if ch == '\r' && chars.peek() == Some(&'\n') {
                            chars.next();
                        }
                        writer.write_all(b"<br>")?;
                    }
                    // Entities are decoded as text after the table and inline syntax are parsed.
                    (Self::Markdown, ch) if ch.is_ascii_punctuation() => {
                        write!(writer, "&#{};", u32::from(ch))?;
                    }
                    (_, ch) => write!(writer, "{ch}")?,
                }
            }
        } else {
            writer.write_all(null.as_bytes())?;
        }
        writer.write_all(end.as_bytes())
    }
}
