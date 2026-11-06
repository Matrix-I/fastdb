# Script result panel visibility and vertical resizing

## Requirements

- Hide the script result area and its footer before SQL execution has created a result.
- Show results during execution and afterward, including empty queries, command completion,
  errors and plans; hide the area when the last retained result is closed.
- Let users drag a horizontal divider to adjust the editor/result height split.
- Keep the split per tab during its lifetime, without changing persisted configuration.
- Leave table tabs and the accepted result rail labels/icons unchanged.

## Implementation

1. Derive result visibility from the existing retained-result lifecycle.
2. Add a small per-tab split state with bounded ratios and deterministic drag math.
3. Wire the divider and mouse release handling into the script view. Fill the editor
   area when there is no result; preserve existing execution status and error feedback.
4. Cover initial visibility, execution/close lifecycle, drag direction, bounds,
   invalid sizes, release, and tab isolation with unit tests.
5. Update the SQL editor guide and manual acceptance status.

## Verification

Capture the current premature visibility with a failing regression test. Run formatting,
workspace Clippy and workspace tests, independently review the diff, and run the app.
The user performs manual GUI acceptance; do not use Computer Use.
