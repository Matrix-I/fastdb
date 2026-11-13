# Properties DDL Layout Implementation Plan

> For agentic workers, use executing-plans-strict to implement these steps.

Goal / Architecture / Tech Stack / Branch

- Match the user's Properties/DDL layout reference with a compact category list
  at left and readable SQL with a line-number gutter at right.
- Retain the existing GPUI layout, read-only source and copy/select-all actions.
  Prepare SQL token ranges during the existing background DDL load.
- Rust 2024, GPUI 0.2.2 and the existing sql-intel SQL parser.
- Branch `feature/p2-05-plans`.

## Confirmed Requirements

- The user explicitly selected the attached image as a Properties/DDL layout
  example. Categories remain on the left and SQL on the right.
- Continue P2-05 for PostgreSQL, MySQL and SQLite.
- Incremental local checkpoint commits are authorized in this session.
- The user verifies the GUI manually. Do not use Computer Use.

## Global Constraints

- Read the relevant code, tests and docs before editing.
- Keep parsing and metadata work off the render path.
- No public API, schema, dependency, persisted format or keyboard changes.
- Preserve existing source bytes, copy behavior, selection, scrolling, theme
  switching, category loading/error states and header-collapse control.
- Run `cargo fmt --all`, `cargo clippy --workspace --all-targets`,
  `cargo test --workspace`, and run the app for UI changes.
- Update docs/guide/navigator-and-tables.md and docs/guide/not-built-yet.md.
- Review the full diff and obtain independent review after verification.

## Technical Defaults

- Use existing theme colors and monospace font rather than copying screenshot
  raster colors or its unknown display scale.
- Use the existing SQL parser and GPUI StyledText highlights. Keep source in
  TableProperties and cache byte ranges, avoiding another full source copy.
- Keep all categories and the disabled Virtual explanation. Add category row
  separators, an accent selected marker and more vertical space.
- Retain the current editable SQL-tab editor separately. The DDL pane remains
  read-only and retains the Copy DDL toolbar.

## Risks & Rollback

- Incorrect token ranges could corrupt displayed Unicode. Use byte ranges and
  assert Unicode boundaries, CRLF, blank lines and multiline comments.
- Parsing large DDL during render would stall interaction. Prepare only in the
  background native DDL load and render cached spans with theme-aware colors.
- Layout acceptance requires the user's manual check. App startup and automated
  model tests verify source preservation, not visual appearance.
- No persisted data/API/format changes. Revert these scoped commits to roll back.

## Task 0 — Recognize the existing parser grammar token names

File crates/sql-intel/src/parser.rs, SqlParser::highlight and its test module.

- [x] Add this failing regression before changing the classifier:

```rust
#[test]
fn highlighting_recognizes_native_grammar_keywords_literals_and_comments() {
    let source = "CREATE TABLE t (n INTEGER DEFAULT 42, s TEXT DEFAULT 'value'); /* note */";
    let mut parser = SqlParser::new().unwrap();
    let spans = parser.highlight(source);
    for (text, kind) in [("CREATE", TokenKind::Keyword), ("42", TokenKind::NumberLiteral), ("'value'", TokenKind::StringLiteral), ("/* note */", TokenKind::Comment)] {
        assert!(spans.iter().any(|span| &source[span.start_byte..span.end_byte] == text && span.kind == kind), "missing {text}");
    }
}
```

- [x] Run `cargo test -p sql-intel highlighting_recognizes_native_grammar --locked`.
  Expect a missing CREATE classification because tree-sitter-sequel emits
  keyword_create, literal and marginalia names that the current match omits.
- [x] Add these match arms before the current cases:

```rust
_ if kind.starts_with("keyword_") => Some(TokenKind::Keyword),
"marginalia" => Some(TokenKind::Comment),
"literal" => {
    let text = &source[start..end];
    if matches!(text.to_ascii_uppercase().as_str(), "TRUE" | "FALSE" | "NULL") {
        Some(TokenKind::Keyword)
    } else if text.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | '.')) {
        Some(TokenKind::NumberLiteral)
    } else {
        Some(TokenKind::StringLiteral)
    }
}
```

- [x] Rerun the regression and the whole sql-intel suite. The parser signature
  stays unchanged. Its spans must now support the existing TokenKind enum.

## Task 1 — Prepare display ranges off the render path

Files

- Add crates/app/src/workspace/properties/ddl_document.rs.
- crates/app/src/workspace/properties/mod.rs, module declarations.
- crates/app/src/workspace/properties/model.rs, TableProperties/new/with_metadata.
- crates/app/src/workspace/catalog_panel.rs, ensure_relation_ddl background result.

Interfaces

- Consume `sql_intel::{SqlParser, TokenKind, HighlightSpan}` and unchanged DDL.
- Produce internal `DdlDocument` and `DdlLine` with source-relative byte ranges,
  per-line highlight ranges, and maximum displayed character count.
- TableProperties owns `Option<Arc<DdlDocument>>`, initially None and reset when
  with_metadata replaces source. `prepare_ddl` prepares it in the background.

- [x] Add a failing unit test to the new module with this complete behavior:

```rust
#[test]
fn ddl_document_preserves_unicode_blank_lines_and_multiline_comments() {
    let source = "-- note\r\nCREATE TABLE \"名\" (\n\n  value TEXT /* first\n  second */\n);\n";
    let document = DdlDocument::new(source);
    let lines: Vec<_> = document.lines.iter().map(|line| &source[line.range.clone()]).collect();
    assert_eq!(lines, ["-- note", "CREATE TABLE \"名\" (", "", "  value TEXT /* first", "  second */", ");"]);
    assert_eq!(document.max_chars, lines.iter().map(|line| line.chars().count()).max().unwrap());
    for line in &document.lines {
        let text = &source[line.range.clone()];
        let mut end = 0;
        for (range, _) in &line.highlights {
            assert!(range.start >= end && range.end <= text.len());
            assert!(text.is_char_boundary(range.start) && text.is_char_boundary(range.end));
            end = range.end;
        }
    }
    assert!(document.lines[1].highlights.iter().any(|(range, kind)|
        &source[document.lines[1].range.clone()][range.clone()] == "CREATE" && *kind == TokenKind::Keyword));
    assert!(document.lines[4].highlights.iter().any(|(_, kind)| *kind == TokenKind::Comment));
    assert!(DdlDocument::new("").lines.is_empty());
}
```

- [x] Run `cargo test -p fastdb-app ddl_document --locked`. Expect failure because
  DdlDocument does not yet exist. Implement this complete internal model:

```rust
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
        let mut spans = SqlParser::new().map(|mut parser| parser.highlight(source)).unwrap_or_default();
        spans.sort_by_key(|span| (span.start_byte, span.end_byte));
        let mut covered = 0;
        spans.retain(|span| {
            if span.start_byte < covered { return false; }
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
            while next < spans.len() && spans[next].end_byte <= offset { next += 1; }
            let mut highlights = Vec::new();
            for span in spans[next..].iter().take_while(|span| span.start_byte < end) {
                let start = span.start_byte.max(offset);
                let stop = span.end_byte.min(end);
                if start < stop { highlights.push((start - offset..stop - offset, span.kind)); }
            }
            lines.push(DdlLine { range: offset..end, highlights });
            offset += chunk.len();
        }
        Self { lines, max_chars }
    }
}
```

- [x] Declare the module, add `ddl_document: Option<Arc<super::ddl_document::DdlDocument>>`
  to TableProperties, set None in new and at the end of with_metadata.
- [x] Add internal `prepare_ddl(mut self) -> Self` assigning
  `Some(Arc::new(DdlDocument::new(&self.ddl)))` and returning self.
- [x] Expand ensure_relation_ddl's existing provider guard to PostgreSQL/MySQL/
  SQLite. Capture `let kind = session.kind` before spawning.
- [x] In the background closure, let PostgreSQL reuse its already-loaded model,
  while MySQL/SQLite retain their native scoped source query. Return the prepared
  model in both branches:

```rust
if kind == DbDriverKind::Postgres {
    return Ok(Arc::new((*model).clone().prepare_ddl()));
}
```

  Append `.prepare_ddl()` after the scoped `.with_metadata(...)`. Generation and
  driver guards remain intact. No additional PostgreSQL SQL query is needed.
- [x] Add this model regression for all three provider paths:

```rust
#[test]
fn ddl_preparation_preserves_source_and_invalidates_on_metadata_refresh() {
    for kind in [DbDriverKind::Postgres, DbDriverKind::MySql, DbDriverKind::Sqlite] {
        let source = "CREATE TABLE t (id INTEGER);\n";
        let model = TableProperties::new(kind, "main".into(), "t".into(), vec![], vec![], vec![])
            .with_metadata(Some(db_wire::TableDetails { ddl: Some(source.into()), ..Default::default() }), None)
            .prepare_ddl();
        assert_eq!(model.ddl, source);
        assert!(model.ddl_document.is_some());
        let model = model.with_metadata(Some(db_wire::TableDetails { ddl: Some("SELECT 1;".into()), ..Default::default() }), None);
        assert!(model.ddl_document.is_none());
        assert_eq!(model.ddl, "SELECT 1;");
    }
}
```
- [x] Rerun the focused test and verify parser ranges plus empty source.
- [x] Commit the verified feature with Task 2 after final gates.

## Task 2 — Render cached syntax with the reference layout

Files and interfaces

- crates/app/src/workspace/properties/ddl.rs, render_ddl.
- crates/app/src/workspace/properties_panel.rs, category list.
- Consume prepared source ranges. Produce GPUI StyledText per source line.

- [x] Replace render_ddl's full-source clone/width scan with the cached width and
  line ranges. Source is unchanged. When no prepared document is present, retain
  the existing plain-source fallback without parsing on the render path.
- [x] Before implementation add this style test in ddl.rs's test module. Use
  explicit imports to avoid the GPUI test macro collision:

```rust
#[test]
fn ddl_token_colors_follow_the_active_theme() {
    for theme in [Theme::dark(), Theme::light()] {
        assert_eq!(token_style(TokenKind::Keyword, &theme).color, Some(theme.text_accent.into()));
        assert_eq!(token_style(TokenKind::Comment, &theme).color, Some(theme.text_success.into()));
        assert_eq!(token_style(TokenKind::StringLiteral, &theme).color, Some(theme.text_success.into()));
        assert_eq!(token_style(TokenKind::NumberLiteral, &theme).color, Some(theme.text_warning.into()));
    }
}
```

  Run `cargo test -p fastdb-app ddl_token_colors --locked`, expect missing
  token_style. Implement the following complete style mapper, then rerun:

```rust
fn token_style(kind: TokenKind, theme: &Theme) -> HighlightStyle {
    let color = match kind {
        TokenKind::Keyword => theme.text_accent,
        TokenKind::Comment | TokenKind::StringLiteral => theme.text_success,
        TokenKind::NumberLiteral => theme.text_warning,
        TokenKind::Identifier => theme.text_primary,
        _ => theme.text_secondary,
    };
    HighlightStyle { color: Some(color.into()),
        font_weight: (kind == TokenKind::Keyword).then_some(FontWeight::SEMIBOLD),
        ..Default::default()
    }
}
```

- [x] Render each prepared line with `StyledText::new(text.to_owned()).with_highlights(...)`.
  Map Keyword to text_accent with SEMIBOLD, Comment to text_success,
  StringLiteral to text_success, NumberLiteral to text_warning, Identifier to
  text_primary, and all remaining kinds to text_secondary. Use the current theme.
  Retain source line numbers and selected-all background. Replace line-level
  coloring with this child construction inside the existing row iterator:

```rust
let prepared = self.ddl_document.as_ref().and_then(|document| document.lines.get(idx));
let text = prepared.map(|line| &self.ddl[line.range.clone()]).unwrap_or(line);
let highlights = prepared.into_iter().flat_map(|line| line.highlights.iter())
    .map(|(range, kind)| (range.clone(), token_style(*kind, theme)));
let code = StyledText::new(text.to_owned()).with_highlights(highlights);
```

  Use `document.max_chars` for prepared width. For an unprepared metadata preview
  keep the existing text width calculation and plain source-line iterator.
  This fallback never calls the parser.
- [x] Keep gutter width and code text separate with a border on the gutter.
  Use fixed 18px line height and no added gap between lines, including blank lines.
- [x] In the existing category builder, use 160px navigation width and 26px rows.
  Add a bottom border using theme.border and a 2px left selection marker using
  theme.text_accent for the selected category and transparent otherwise.
  Preserve every category's callback and reset behavior. The exact row style
  additions to the current builder are:

```rust
.h(px(26.0))
.border_b_1()
.border_color(theme.border)
.child(div().w(px(2.0)).h_full().flex_shrink_0()
    .bg(if is_selected { theme.text_accent } else { theme.bg_surface }))
```

  Place the marker before the icon, replacing left row padding with child
  spacing to keep the marker flush with the navigation edge.
- [x] Run the app and report manual visual acceptance pending. Do not use CUA.

## Task 3 — Verification, docs and checkpoint

- [x] Read the entire tracked/untracked diff and check secrets/scope.
- [x] Run `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked`,
  `cargo test --workspace --locked`, and `cargo build --workspace --locked`.
- [x] Start the built app with an isolated FASTDB_CONFIG_DIR. Verify it stays
  running and records initialization. Stop only the owned process and remove
  the temporary config. Retain the runnable binary/dependencies.
- [x] Update the two Properties guide pages and the P2-05 completion context.
- [x] Self-review correctness, performance, security, robustness, naming/APIs,
  requirements, test coverage and design with concrete evidence.
- [x] Use code-review-strict on the whole change. Reverify any corrective edit.
- [x] Commit implementation/docs for user-visible behavior together, then a
  separate checkpoint-only documentation commit if needed. Do not push.

## Plan review

The Opus CLI review timed out after 120 seconds without a verdict. The available
independent reviewer reviewed the plan in two rounds. Round 1 identified the
missing PostgreSQL background path and missing style/cache test steps. Round 2
approved the corrected plan with no Critical or Important issue. Prepared
rendering should iterate the cached source ranges directly. The required Opus
model remains unverified because that invocation failed.


## Implementation evidence

- Baseline SQLite relation suite passed 13 tests before implementation.
- At source baseline 7b97cb9 on the pinned stable Rust toolchain, the parser
  regression failed with missing CREATE. The grammar emits keyword_create,
  literal and marginalia node kinds, which the existing classifier omitted.
- The new display tests initially failed because DdlDocument/token_style did
  not exist. After implementation all 42 sql-intel tests and five focused DDL
  app tests passed, including Unicode/CRLF/comment clipping, theme mapping,
  all-provider source preservation and metadata-refresh cache invalidation.
- Prepared rendering iterates cached source-relative line ranges directly.
  No parser is stored in the model or constructed during rendering.
- PostgreSQL prepares its already-loaded model using the same background
  generation/driver guards as scoped MySQL/SQLite source loading.

## Final verification and self-review

Formatter, workspace Clippy, workspace tests and workspace build passed with
`--locked`. The built app recorded initialization and stayed running for the
five-second isolated-config startup check. macOS logged notification service
diagnostics. Visual, scrolling and clipboard acceptance remains manual.

1. Requirements/correctness. All three native provider paths prepare the
   original source. Unicode ranges, blank lines, multiline comments and source
   replacement are covered. Cached previews retain native-metadata warnings.
2. Performance. Parsing, sorting and line-width preparation occur during the
   background load. Prepared rendering consumes cached ranges. No SQL or
   metadata query was added for PostgreSQL.
3. Security. SQL stays plain text in StyledText, and the parser log contains no
   source or connection details. This change adds no execution action.
4. Robustness. Existing generation/driver guards reject stale background
   results. Metadata changes clear the display cache. Parser initialization
   failure logs a warning and leaves source readable without colors.
5. Syntax/naming. Formatter, Clippy and build passed. Internal model names
   match the existing Properties modules.
6. Modern APIs. The implementation uses installed GPUI 0.2.2 StyledText and
   the repository's existing sql-intel parser API.
7. Business clarity. Existing theme/font and read-only actions are retained.
   The image defines the layout reference. Pixel and gesture acceptance stays
   with the user.
8. Coverage. The grammar regression failed before its fix with missing CREATE.
   Source/cache/theme/comment tests pass in the workspace suite. GUI interaction
   was not performed because the user explicitly reserves it for manual checks.
9. Design. Display preparation is isolated from source metadata and rendering.
   No dependency, public API, schema or persisted format changed.

Self-review corrected the metadata-warning predicate to check for actual native
DDL, so a prepared PostgreSQL metadata preview cannot suppress its warning.


## Checkpoint fa34848

Implementation and automated verification are complete. The final internal
review approved the whole diff in one round with no Critical/Important finding.
No code changed after that review. The reviewer checked keyboard copy, GPUI
UTF-8 ranges, stale-result guards and verification logs outside the diff.

The code-review-strict Opus invocation was rejected by automatic approval review
because the external CLI could transmit repository contents to an unapproved
external destination. It was not executed and was not retried through another
external path. The built-in independent reviewer was used as the safer
alternative. The exact Opus-model requirement is the skipped skill rule. The
Opus-specific review remains unverified. The earlier plan Opus invocation had
timed out. No other rule was skipped.

Manual visual/scroll/clipboard acceptance and large-source runtime benchmarking
remain unverified. No speedup is claimed. Existing theme and typography were
chosen to translate the screenshot reference to the application's scale.
