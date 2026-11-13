use sql_intel::{SqlParser, TokenKind};
use std::ops::Range;

pub(super) struct DdlLine {
    pub range: Range<usize>,
    pub highlights: Vec<(Range<usize>, TokenKind)>,
}
pub(super) struct DdlDocument {
    pub lines: Vec<DdlLine>,
    pub max_chars: usize,
}
impl DdlDocument {
    pub fn new(source: &str) -> Self {
        let mut spans = match SqlParser::new() {
            Some(mut parser) => parser.highlight(source),
            None => {
                tracing::warn!("SQL parser unavailable for DDL highlighting");
                Vec::new()
            }
        };
        spans.sort_by_key(|span| (span.start_byte, span.end_byte));
        let mut covered = 0;
        spans.retain(|span| {
            if span.start_byte < covered {
                return false;
            }
            covered = span.end_byte;
            true
        });
        let mut lines = Vec::new();
        let mut offset = 0;
        let mut next = 0;
        let mut max_chars = 0;
        for chunk in source.split_inclusive('\n') {
            let text = chunk.strip_suffix('\n').unwrap_or(chunk);
            let text = text.strip_suffix('\r').unwrap_or(text);
            let end = offset + text.len();
            max_chars = max_chars.max(text.chars().count());
            while next < spans.len() && spans[next].end_byte <= offset {
                next += 1;
            }
            let mut highlights = Vec::new();
            for span in spans[next..]
                .iter()
                .take_while(|span| span.start_byte < end)
            {
                let start = span.start_byte.max(offset);
                let stop = span.end_byte.min(end);
                if start < stop {
                    highlights.push((start - offset..stop - offset, span.kind));
                }
            }
            lines.push(DdlLine {
                range: offset..end,
                highlights,
            });
            offset += chunk.len();
        }
        Self { lines, max_chars }
    }
}

#[cfg(test)]
mod tests {
    use super::DdlDocument;
    use sql_intel::TokenKind;
    #[test]
    fn ddl_document_preserves_unicode_blank_lines_and_multiline_comments() {
        let source = "-- note\r\nCREATE TABLE \"名\" (\n\n  value TEXT /* first\n  second */\n);\n";
        let document = DdlDocument::new(source);
        let lines: Vec<_> = document
            .lines
            .iter()
            .map(|line| &source[line.range.clone()])
            .collect();
        assert_eq!(
            lines,
            [
                "-- note",
                "CREATE TABLE \"名\" (",
                "",
                "  value TEXT /* first",
                "  second */",
                ");"
            ]
        );
        assert_eq!(
            document.max_chars,
            lines.iter().map(|line| line.chars().count()).max().unwrap()
        );
        for line in &document.lines {
            let text = &source[line.range.clone()];
            let mut end = 0;
            for (range, _) in &line.highlights {
                assert!(range.start >= end && range.end <= text.len());
                assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
                end = range.end;
            }
        }
        assert!(
            document.lines[1]
                .highlights
                .iter()
                .any(
                    |(range, kind)| &source[document.lines[1].range.clone()][range.clone()]
                        == "CREATE"
                        && *kind == TokenKind::Keyword
                )
        );
        assert!(
            document.lines[4]
                .highlights
                .iter()
                .any(|(_, kind)| *kind == TokenKind::Comment)
        );
        assert!(DdlDocument::new("").lines.is_empty());
    }
}
