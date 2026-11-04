# P1-07 value editing

Add a bounded value editor for text, JSON, XML and hexadecimal binary values.
Open it from the focused result cell. Preserve SQL NULL, empty text, empty
binary and unset insert defaults as distinct states. Apply stages one undoable
grid change and leaves the existing Save transaction responsible for persistence.
Query-result type and key restrictions continue to apply.

Read selected values and files on workers, revalidate tab, pager, edit revision
and driver identity, and report page/read/write errors explicitly. Editing
supports at most 256 KiB of raw value data. UTF-8 is the default text file
encoding. Explicit UTF-16 LE/BE options decode and encode strictly. Binary files
always carry exact bytes. Date/time display preferences affect only presentation.
UTC formatting applies only to timezone-aware timestamps.

Make binary DML type-aware for each supported dialect. Validate staged hex
before generating SQL and preserve original binary values in optimistic row
predicates. Cover SQLite round-trips through real writes and PostgreSQL native
binary handling, plus editor-state and file-format edge cases.

Keep validation/formatting off the render path. Reuse the existing SQL editor
view as a multiline text surface, existing value-format helpers and the app's
file dialogs. Format is an explicit edit. GUI verification belongs to the user.
Run format, Clippy, full tests and build, obtain independent review, commit the
completed step, then remove obsolete build/test artifacts.

## Implementation and verification

The editor stages one change in grid history and reuses table Save. Its source
check covers tab, pager, driver, edit revision, schema staleness and deleted rows.
Text/JSON/XML source remains exact until an explicit edit; JSON formatting keeps
number lexemes and duplicate keys, and XML formatting is refused rather than
changing significant whitespace. The viewer performs bounded reads/formatting on
one worker, reports oversized/read failures and distinguishes unset defaults,
NULL, empty text and empty bytes. File operations use strict encodings and atomic
replacement. UTC display applies only to timezone-aware timestamps.

Regression evidence was captured before the corresponding fixes:

- `/tmp/fastdb-value-binary-before.log`: SQLite stored hex text instead of bytes.
- `/tmp/fastdb-p107-duplicate-prefix-before.log`: duplicating literal `\xdead`
  bytes stored `[222, 173]` instead of the original six bytes.
- `/tmp/fastdb-p107-escaping-before.log`: PostgreSQL Save failed on backslash/quote
  text with `standard_conforming_strings=off`.

Real SQLite and PostgreSQL/MySQL tests now exercise those paths. PostgreSQL also
preserves JSON numeric/duplicate-key source and XML mixed content. MySQL checks
literal behavior on pinned sessions in both backslash modes, plus table Save
through its normal pool. The first full-suite attempt exposed a missing local
`fastdb_test` database; creating that disposable fixture fixed the setup error.
No tests were weakened or skipped to obtain a passing result.

Final gates use `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets --locked`,
`cargo test --workspace --locked` and `cargo build --workspace --locked`.
Test environment: `FASTDB_TEST_PG_PORT=55439`, `FASTDB_TEST_MYSQL_PORT=55440`,
`FASTDB_TEST_MYSQL_DATABASE=fastdb_test`, `FASTDB_REQUIRE_DB_TESTS=1`.
Machine-readable outputs are `/tmp/fastdb-p107-{tests,clippy,build}.jsonl`.
The current binary starts with an isolated empty configuration and remains alive
after five seconds; this is startup evidence, not GUI acceptance.

SQL planning checks used disposable 100,000-row tables. PostgreSQL's typed binary
UPDATE uses the primary-key Index Scan, visits one row and three local scan
buffers (five at the update), with 0.034 ms execution in this run. MySQL's UTF-8
literal predicate uses PRIMARY range access and estimates one examined row.
No indexes were added and no speedup is claimed. Temporary tables are rolled
back or removed when their sessions close.

Independent review found and resolved XML expansion/content changes, JSON numeric
and duplicate-key changes, binary duplication/prefix ambiguity and the viewer's
unset/default label. The latest review reported no blocking findings. GUI
acceptance belongs to the user; vendor ODBC live verification remains deferred
to Linux as requested. Oracle empty binary writes are explicitly unavailable
until native RAW/BLOB metadata resolves Oracle's NULL semantics.

## Self-review

1. Requirements and correctness: real write/read-back and source-guard tests cover
   exact values, NULL/empty/default states, stale results and one-step Undo.
2. Performance: viewer/file workers and duplicate budgets are bounded; render does
   not read pages or pretty-print values; indexed Save plans were checked above.
3. Security: binary input is validated hex, text uses existing mode-safe literal
   encoding, and the diff introduces no credentials or authentication changes.
4. Robustness: worker ownership releases through Drop, file output is atomic,
   failures remain explicit and no dependencies were added.
5. Syntax and naming: workspace format, Clippy, tests and build are required gates;
   identifiers, messages and documentation follow existing English conventions.
6. Modern APIs: the pinned Rust toolchain and Clippy validate current APIs;
   byte pairs use checked sizes and `as_chunks`.
7. Business clarity: the documented 256 KiB limit, source-only XML behavior,
   timezone-only display change and provider capability limits are explicit.
8. Test coverage: regression failures preceded fixes; SQLite/PostgreSQL/MySQL
   read-back, file/encoding, bounds, source guards and Undo have automated coverage;
   GUI and ODBC acceptance remain explicitly assigned/deferred above.
9. Design: source reading, file encoding, editor state, rendering and dialect
   literals retain separate responsibilities and share existing Save machinery.
