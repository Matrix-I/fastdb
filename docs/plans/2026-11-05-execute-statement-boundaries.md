# Execute statement boundaries

- Remove whole-script live syntax diagnostics from the SQL editor; retain database execution errors and syntax highlighting.
- Preserve exact selected SQL as the Execute input. Without selection, find the statement containing the caret using lexical boundaries rather than tree-sitter error recovery.
- Honor semicolons outside literals/comments, and independent top-level statement starts without semicolons. Preserve nested SELECT, CTE bodies, INSERT SELECT and UNION chains.
- Add regression tests in sql-intel and ui-editor, update the user guide, run workspace formatting, lint, tests and build, launch the app, and obtain independent review. Manual GUI acceptance belongs to the user.

This plan is intentionally uncommitted because the user has not requested a commit.

## Verification

- Base: commit `88e124c`, macOS Apple Silicon, Rust 1.99.1. Existing concurrent connection, title-bar and provider changes were preserved.
- Before: `cargo test -p sql-intel execute_` failed for `SELECT 1\nSELECT 2`, returning `SELECT 1\nSELECT` instead of the first statement. The CTE fixture also included a fragment of the following SELECT.
- After: the same regression cases pass, including caret positions throughout nested queries, PostgreSQL E strings, CTEs, INSERT SELECT, UNION, transaction commands, compound bodies and foreign-key actions.
- Real database flow: an in-memory SQLite fixture executes the extracted INSERT between an earlier DELETE and a following SELECT. Stored IDs are `[1, 2]`, proving that the DELETE was not executed.
- `cargo fmt --all`: passed. `git diff --check`: passed for the shared working tree.
- `cargo test --workspace`: passed after allowing local sockets. The sandboxed attempt failed at the SSH forwarding test's listener with `Operation not permitted`; no test was weakened or skipped to bypass that failure. Provider-specific live tests still require their documented environment variables.
- `cargo clippy --workspace --all-targets`: passed.
- `cargo build -p fastdb-app`: passed; the desktop process was launched. macOS emitted XPC/notification messages, so launch is not evidence of GUI acceptance.
- Independent review: execution-scope findings were fixed and covered by regression tests; final review has no blocking findings.
- GUI end-to-end verification remains pending because project instructions prohibit Computer Use and assign manual acceptance to the user. Script: place the caret in either of two consecutive SELECT statements without semicolons; execute each; highlight a different SQL range and execute it; check that no live syntax-error banner appears while typing.

## Self-review

1. Requirements and correctness: exact selection retains priority; tested caret ranges exclude surrounding statements and preserve the covered compound SQL forms.
2. Performance: boundary scanning runs only when Execute/Explain requests SQL, uses linear token passes, and replaces whole-script syntax validation on the render path; no production database query was added.
3. Security: tests verify that unrelated DELETE statements are excluded; no credentials, SQL logging, authentication or authorization changes were introduced.
4. Robustness: incomplete tokens do not trigger whole-script fallback; existing strict vendor script splitting stays in place; no dependencies or external calls were added.
5. Syntax and naming: workspace formatting, Clippy, build and tests pass using the existing English Rust conventions.
6. Modern APIs: the change uses existing Rust, tree-sitter and rusqlite APIs already present in the project; no deprecated API was introduced.
7. Business clarity: the user confirmed that only independent top-level SQL starts are boundaries; SELECT within CTE, subquery, INSERT SELECT and UNION stays together. Additional non-query command starts use a new line as a conservative boundary convention.
8. Test coverage: original failures were recorded before implementation; regressions assert exact SQL and stored SQLite data; manual GUI and environment-dependent live-provider acceptance remain unverified.
9. Design: lexical scanning, boundary state transitions, caret selection and rendering remain separate; private helpers reuse the existing tokenizer without changing public signatures or persisted formats.

## Limits

Boundary inference does not replace a dialect-complete database parser. Existing vendor delimiter and SQL-mode restrictions remain as documented in the SQL editor guide. Use explicit selection or Run script for vendor compound batches. No commits or pushes were made.
