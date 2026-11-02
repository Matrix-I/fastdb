# Grid clipboard formats

## Scope

Add a cell context menu with Copy (TSV), Copy as CSV and Copy as SQL INSERT for
table and script results. A right click inside a selection preserves it; outside
selects the clicked cell. Copy only the selected rectangle and current displayed
values, including staged edits/insertions. CSV has no header and represents NULL
as an empty field. SQL quotes identifiers/literals using the tab's session provider,
uses the selected column names, and emits one INSERT per row (script target:
query_result). Like result export, values use their textual result representation.

## Implementation

1. Forward right-button cell events from ui-grid without changing left selection.
2. Snapshot the pager, selection bounds and staged changes at copy time; serialize
   in the background using shared CSV quoting and SQL dialect helpers. Limit
   clipboard output to 16 MiB, report errors without replacing the clipboard, and
   guard async completion with request/tab/result identities.
3. Add a window-clamped context menu with disabled items for unavailable selection,
   Escape/outside dismissal and footer status. Preserve Cmd/Ctrl+C as TSV.
4. Cover quoting, selected columns, reversed/out-of-range ranges, NULL/empty values,
   staged and inserted rows, output limits and SQL replay with tests. Update README.
5. Run fmt, workspace clippy/tests and package the macOS app for the user's GUI
   test, following their preference to perform interactive checks themselves.

The plan is saved before implementation. Commit only when requested.

## Implementation and validation

Implemented the cell menu, shared CSV field quoting, provider-aware selected-column
INSERT statements, immutable background snapshots, bounded clipboard output and
request/tab/pager completion checks. Existing keyboard TSV copy stays synchronous;
newer keyboard and button clipboard actions supersede pending menu copies.

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `FASTDB_TEST_PG_PORT=55438 cargo test --workspace --offline`: 253 passed. All
  seven live PostgreSQL tests ran; other external-provider/TLS/keychain tests
  retained their configured skip behavior.
- Six new clipboard tests cover CSV quoting, provider-specific SQL, selected
  columns, NULL versus empty text, Unicode, reversed/clamped/stale selection,
  staged edits/insertions, immutable snapshots, budget failure and SQLite replay.
- `CARGO_NET_OFFLINE=true ./bundle_macos.sh --no-install`: passed. Bundle
  signature, plist and ZIP integrity checks passed for updated artifacts in dist.
- Interactive GUI acceptance is pending the user's requested manual test: select
  cells, right-click inside/outside the range, copy all three formats, paste into
  the editor, check footer status and verify table/script result behavior.

Validation completed before the user requested commits. No installation or release was performed.
