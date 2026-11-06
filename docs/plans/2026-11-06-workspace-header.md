# Workspace header and transaction popup layout

## Scope

Separate the native title area, horizontal workspace toolbar and document tab row.
Keep window controls clear; align Connections with document tabs below the toolbar.
Place Commit/Rollback and a vector transaction mode selector on the horizontal
workspace toolbar, always visible even without an active tab. Disable Commit and
Rollback in Auto-Commit; enable them only in Manual mode on a supported connected
SQL tab. Keep the selector enabled in Auto mode when that SQL session is available.
Disable the whole group for table tabs, disconnected/unsupported sessions or busy
operations. Anchor its popup to the measured bottom-left of the selector,
not to pointer coordinates or a fixed window X. Keep Execute/Explain on the editor
rail and remove duplicate connection/transaction headers. Preserve transaction
execution semantics; smart commit is outside scope.

## Validation

Reproduce the fixed-X popup bug in a failing geometry regression test. Cover
button bounds at multiple offsets and sizes, header title/row dimensions, vector
mode icons and labels. Run formatter, Clippy, workspace tests, build and launch.
Obtain independent review. GUI acceptance remains manual per project preference.
Keep this plan uncommitted until the user explicitly requests commits.
