# Two-step new connection dialog

Split Add new connection into database selection followed by configuration.
Use Next to continue and Back to choose another provider. Preserve the draft
when returning to the same provider; use existing presets when changing type.
Open saved connection edits directly in configuration. Keep deferred providers
disabled and prevent keyboard input from editing hidden configuration fields.

Implement and test navigation in app connection state, update dialog and keyboard
routing, and document the flow. Run formatting, workspace lint/tests, build and
launch the app with an isolated profile. GUI acceptance remains manual.

This plan is left uncommitted because commits were not requested.

## Fixed-size layout revision

Use the same 840 × 600 dialog frame for both steps, bounded by the workspace.
Keep header, result feedback and footer outside the content area. Show provider
cards in a grid. Split configuration into Main, SSH, TLS and Advanced tabs so
all fields fit without a scrolling form. Keep keyboard traversal within the
visible tab, reset focus on tab changes and retain drafts across navigation.
Test tab visibility, traversal, reset and data preservation. Run workspace
checks and startup smoke; desktop layout acceptance remains manual.

## Layout verification and self-review

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets`: passed.
- `cargo test --workspace`: 580 passed, zero failures; local socket access enabled
  for the existing SSH forwarding tests. App suite: 242 passed.
- `cargo build -p fastdb-app`: passed.
- `FASTDB_CONFIG_DIR=/tmp/fastdb-connection-layout-smoke FASTDB_LOCAL_CREDENTIALS=1 cargo run -p fastdb-app`:
  built binary started and remained running until intentionally stopped. macOS
  emitted `getApplicationProperty: called with invalid property` and an
  `IMKCFRunLoopWakeUpReliable` Mach-port diagnostic; GUI interaction was not verified.
- The retained-identity regression failed on the missing TLS tab before the fix
  and passed after it. Other tests cover visible-field forward/reverse traversal,
  disabled/empty tabs, provider-specific fields, draft preservation and reset.
- Independent review found narrow-window card wrapping and inaccessible retained
  TLS identity fields; both were addressed and re-reviewed without open findings.

| Review item | Evidence / limit |
| --- | --- |
| Requirements and correctness | Shared frame, fixed actions, provider cards and configuration tabs; drafts survive navigation and retained identity can be cleared. |
| Performance | No database/network work added; tab/field lists are bounded by four tabs and six fields. |
| Security | Credential storage and validation unchanged; secret fields remain masked and no logging added. |
| Robustness | Tab changes reset field selection; traversal excludes hidden fields, including when toggles hide controls. No dependencies added. |
| Syntax and naming | Rust formatter, Clippy and build passed; English names follow the existing workspace style. |
| Modern APIs | Uses the existing GPUI rendering, line input and clipboard APIs; no deprecated APIs introduced. |
| Business clarity | Scope is layout/navigation only; provider availability and connection semantics are preserved. |
| Test coverage | 580 automated tests pass; newly covered retained-identity regression was verified failing before its fix. GUI layout, copy feedback and real interaction acceptance remain manual and pending. |
| Design | Rendering is separated into main fields, tab strip, tab content, feedback and actions; navigation belongs to form state. |

Manual acceptance: open Add new connection, check both steps retain dimensions;
check all provider cards and footer at 1280 × 800 and 800 × 600 workspace sizes;
visit Main/SSH/TLS/Advanced (including enabled SSH/TLS), long field values and
long test errors; verify Tab/Shift+Tab, Back/Next, Edit and SQLite's smaller form.
Computer Use was not used, as required by the project. Layout acceptance remains
incomplete until these manual checks pass. Live-provider acceptance is not claimed.
The plan-only commit remains omitted because the user has not requested commits.
