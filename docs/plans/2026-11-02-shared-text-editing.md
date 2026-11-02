# Shared text editing

## Problem and scope

SQL scripts implement editing separately from the single-line fields. Connection
and filter containers also reset selections independently of character hit targets.
These separate paths allow Shift-click, clipboard and navigation behavior to drift.

## Design

Move the single-line editing and rendering primitives from the application into
`ui-editor`. Add a shared command dispatcher and mouse selection policy there.
Use the same dispatcher for SQL, connection fields (including masked passwords),
filter, fetch size, script rename/search and inline cell editing. Keep storage and
undo implementations specialized: Ropey for SQL and existing String snapshots for
short fields. Keep SQL completion and application commands outside the dispatcher.

Selection uses a stable anchor; Shift presses take priority over click counts.
Unmodified double-click selects a line (the entire value in a single-line field).
Dragging after a double-click retains complete units. Copy/cut use the selection;
with no selection they leave the value and clipboard unchanged. Paste and typing
replace selection. Navigation, select-all and clipboard command recognition share
one implementation. Single-line fields retain validation and Enter submission;
SQL retains newlines, completion, syntax styling and two-axis scrolling.

Escape clears selection while retaining text, caret and viewport, and dismisses
SQL completion suggestions. When a dialog or inline cell edit owns the selected
text, the first Escape clears that selection; a subsequent Escape keeps its
existing dismiss/cancel behavior. Menus and unrelated modal overlays retain their
Escape priority.

Route container/background mouse presses through the same picking callbacks as
characters, so mouse-up click handlers cannot overwrite character selection.
Read-only result cells retain grid range selection; they are not text inputs.

## Validation

Cover stable anchors, rapid Shift-click, reversed selections, full-line drag,
Unicode/password indexing, shared shortcuts, clipboard payloads and editing/undo.
Run formatting, workspace Clippy and tests, rebuild and launch the macOS app.
The user will perform interactive acceptance across the input surfaces. Do not
commit until explicitly requested; keep this plan in its own documentation commit.

## Results

- Shared `text_edit` commands dispatch to both `SqlEditorView` and `LineInput`;
  short-field snapshots and SQL edit-based undo retain their existing storage.
- Shared `line_input` primitives own mouse press/drag policy, stable anchors,
  selection ranges, hit targets and selection colors. Application callbacks own
  focus and field validation. All editable fields and their padding use these
  callbacks, including script chooser/rename and inline cell editing.
- The same clipboard/selection/replace/undo contract runs against both adapters.
  Additional tests cover numeric paste validation, single-line newline handling,
  shortcut recognition and rapid Shift-click after whole-line selection.
- Final workspace tests after Escape support: 282 passed, including live PostgreSQL
  tests on port 55438. Workspace Clippy with `-D warnings`, formatting and diff checks pass.
- Rebuilt the app, DMG and ZIP with `--no-install`; signature, Info.plist and ZIP
  integrity checks pass. Launched the rebuilt app for review.
- The user approved the final build after Escape support and requested a commit.
- Escape tests cover forward/reverse selections, repeated Escape, completion
  dismissal, unchanged text/caret/viewport, and subsequent typing/undo in both
  editor adapters.
