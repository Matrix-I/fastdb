# Grid mouse drag selection

Add left-button drag selection to table and script result grids. Mouse down fixes
the anchor (Shift preserves the existing anchor); moving across cells extends the
rectangle, and release keeps that range selected for clipboard actions. Preserve
right-click selection/menu behavior and double-click cell editing.

Forward cell mouse moves through ui-grid's interactive callback API. Track drag
ownership by stable tab and pager identities, end on mouse-up (including outside
the grid), and reject stale drags after tab/result changes. Do not start from
header resizing, right clicks, double clicks or a drag begun outside the grid.
Notify only when the selection changes. Cover the selection lifecycle and stale
ownership in tests, update README, run fmt/clippy/workspace tests and rebuild the
macOS bundle for the user's manual GUI test. Commit only when requested.

## Validation

- Formatting, workspace clippy with warnings denied and all 257 workspace tests
  passed. Four new tests cover reversed/forward dragging, Shift anchors, unchanged
  and invalid coordinates, stale tab/result identities and editing isolation.
  PostgreSQL integration tests ran live; other provider/TLS/keychain tests kept
  their environment-variable skip behavior.
- Clicking a grid also transfers keyboard focus from the script editor/filter to
  that grid, so subsequent Cmd/Ctrl+C copies its selected cells.
- `CARGO_NET_OFFLINE=true ./bundle_macos.sh --no-install` passed. Updated app/DMG/
  ZIP artifacts are in dist. Bundle signature/plist and ZIP verification passed.
- GUI acceptance remains pending the user's manual check: left-drag a rectangle,
  release, hover without changing selection, copy TSV/CSV/SQL; test Shift-click,
  double-click editing, release outside the grid, and script result grids.

Validation completed before the user requested commits. The app was not installed.
