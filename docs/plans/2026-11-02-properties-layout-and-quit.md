# Properties layout and Quit follow-up

## Scope

Fix the reported overlapping Columns metadata text and the Quit menu/shortcut dispatch path. Document DBeaver object-editor workflows separately; a working schema editor requires a dedicated provider-aware implementation and is not delivered by this layout fix.

1. Keep metadata cells on one clipped line with explicit line height/padding, preserving horizontal scrolling and column widths. Remove the nonfunctional collation link affordance.
2. Defer the global Quit action until GPUI releases its active-window dispatch borrow; retain script/config save barriers and log dispatch failures.
3. Add the screenshot findings, object-editor lifecycle and screen/workflow inventory to the design audit and roadmap.
4. Run formatting, Clippy, workspace tests and an isolated app launch. Verify keyboard/menu Quit and narrow/wide Columns layout on a desktop when Computer Use permission is available; report any blocked visual verification explicitly.

No commits are made unless requested.
