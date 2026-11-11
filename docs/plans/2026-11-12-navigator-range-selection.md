# Navigator range selection

## Behavior

Plain click selects one relation and establishes an anchor. Shift+click selects
the inclusive range of visible relation rows in the active connection, in tree
order, preserving that anchor. Collapsed folders and filtered-out relations are
excluded. A missing or hidden anchor falls back to a single selection.
Escape clears the selection and range anchor after menu/dialog and selected-text handling.
Double-click continues to open only the clicked relation. Existing context-menu
actions remain scoped to their clicked target; bulk actions are outside this change.

## Implementation and verification

1. Add connection-local range state and deterministic selection tests.
2. Compute visible relations on click using the tree's schema/group ordering and
   expansion/filter rules; highlight the selected rows, retaining a single scroll anchor.
3. Reset range state when opening/revealing a relation or reloading a connection.
4. Update the guide; inspect the full diff, run formatting, workspace tests,
   Clippy and a build, then launch the app for manual GUI acceptance.
5. Obtain an independent diff review and address its findings.

## Verification

- Workspace tests: 699 passed.
- Workspace Clippy, formatting, build and diff checks passed.
- Independent review found no blocking issues.
- The user accepted the GUI behavior.
