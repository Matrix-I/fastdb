# SQL editor roadmap extras

## Scope

Complete the SQL editor roadmap item: selection/clipboard, whole-script execution,
multiple result tabs, script rename/delete, query history, and SQL formatting.
Keep all result rows in the existing paged/spilling store and preserve connection
and tab ownership. Do not commit implementation unless requested.

## Implementation sequence

1. Add UTF-8-safe buffer selections, keyboard/Shift-click selection, clipboard editing
   and selection highlighting, retaining undo/redo and background autosave.
2. Add a quote/comment-aware SQL scanner shared by statement splitting and a
   conservative whitespace formatter. Preserve literal/comment bytes; reject ambiguous
   procedural script syntax instead of executing fragments.
3. Run scripts sequentially, stopping on first error or cancellation, with one result
   per statement. Retain each result's pager and metadata when switching result tabs.
   Transaction control and unsupported procedural/batch syntax are rejected in Run script:
   pooled drivers do not yet guarantee a pinned session across statements. Single-statement
   execution remains available. No implicit transaction is added.
4. Add bounded per-connection persistent query history with recall, plus script naming
   and recoverable deletion. Serialize file operations with autosaves so renamed/deleted
   scripts cannot be recreated by queued writes. Surface failures in the UI.
5. Document behavior and shortcuts. Add focused regression tests for selections,
   SQL splitting/formatting, result ownership, history and script file operations.

## Verification

Run cargo fmt --all, cargo clippy --workspace --all-targets and cargo test --workspace.
Launch the app with an isolated profile for UI changes; record the limits of automated
UI inspection if Computer Use remains unavailable. Live database tests skip when
FASTDB_TEST_* is unset, and should not be claimed as live verification in that case.

## Implementation record

- Added UTF-8 byte-boundary selections, keyboard/mouse extension, copy/cut/paste,
  selection replacement and undo/redo coverage.
- Added conservative shared scanning for ordinary scripts and layout-only formatting.
  Backslash quoting and MySQL hash/executable comments are rejected because semantics
  depend on session mode. Client directives, transaction control and unquoted procedural
  blocks are outside Run script's supported scope.
- Added sequential execution with stop-on-error/cancel and at most 20 retained results.
  Inactive results own their entire grid/pager, fetch state, SQL, errors and timing.
- Added a serialized background history worker (100 unique entries per connection,
  64 KiB SQL limit), history recall, and autosave-ordered rename/recoverable deletion.
  History shares immutable snapshots during render rather than copying all SQL.
- README documents shortcuts, plaintext history storage and execution limitations.

Validation on 2026-11-01:

- cargo fmt --all -- --check: passed.
- cargo clippy --workspace --all-targets --offline -- -D warnings: passed.
- cargo test --workspace --offline: passed, including 91 app tests, SQLite script
  execution/stop-on-error, result state preservation, history persistence, serialized
  file mutation/no overwrite, UTF-8 selection and formatter/scanner regressions.
- cargo build -p fastdb-app --offline: passed.
- Launched target/debug/fastdb from `/` with FASTDB_CONFIG_DIR set to
  `/tmp/fastdb-sql-editor-smoke/profile`; a disposable SQLite database and sample SQL
  script are available there. Startup remained running. Interactive UI behavior has
  not been agent-verified: Computer Use permission is unavailable in this session.
- External provider tests skip when their environment variables are unset; this run
  does not claim live PostgreSQL/MySQL/ODBC verification.

The user subsequently requested committing the implementation. Keep this plan in a
separate documentation commit from the implementation and README behavior changes.
