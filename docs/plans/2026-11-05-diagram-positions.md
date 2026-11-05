# Saved diagram positions

Remove redundant relationship rows. Drag cards by their headers, recompute connector geometry, and save on release. Store each root diagram separately under its connection ID in the active configuration directory. Restore positions on reopen and delete the connection's layout directory when its profile is deleted. Verify persistence, isolation, and cleanup with temporary directories and run workspace checks. GUI acceptance is manual.

Review addressed long identifier paths, stale tabs overwriting other cards, and cleanup before a connection deletion was saved. Filesystem cleanup failures are logged and are not retried automatically.

1. Correctness: drag translates screen deltas into world coordinates. Layout identity includes connection, schema and root table.
2. Performance: no file writes during pointer movement. Geometry updates for the focused graph only.
3. Security: encoded path segments keep identifiers out of filesystem path syntax. No credentials enter the layout.
4. Robustness: bounded reads, atomic replacement and explicit load/save errors. Cleanup follows the successful connection config write.
5. Syntax and naming: use the workspace formatter and Clippy.
6. APIs: use existing GPUI pointer handlers and tempfile persistence.
7. Business rules: drop autosaves. Closing a tab or disconnecting keeps positions.
8. Coverage: tests exercise drop, reopen at a different zoom, long names and connection-scoped cleanup. Manual GUI acceptance remains pending.
9. Design: layout storage is separate from geometry and pointer handling.

The plan is uncommitted because the user did not request a commit.

Card width follow-up: the old formula returned 554 px in a deterministic fixture whose widest row plus spacing requires 222 px. GPUI now measures the displayed labels once per focused graph, then rebuilds geometry before restoring saved positions. The regression checks the widest row, and multiline labels are converted to visible single-line markers before both measuring and rendering. Review found and resolved the newline issue. Manual GUI acceptance remains pending.
