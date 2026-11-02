# SQL editor scrolling and select-all

Fix clipped long SQL lines and add visible horizontal/vertical scrollbars while
keeping the editor inside its window pane. Cache glyph/line widths by buffer
revision, use a two-axis content viewport and reveal the caret on both axes.
Keep scrolling position independent of hover/blink renders and account for scroll
offsets when positioning completion popups.

Use the shared detail scrollbar rendering around the editor viewport. Preserve
mouse character picking and scrollbars' keyboard focus. Centralize Cmd/Ctrl+A in
the editor, keep the viewport steady during select-all and use a clear selection
color; make editor focus take priority over a stale hidden grid cell editor.
Add double-click whole-line selection (including line endings), preserving
whole-line selection during drag and avoiding viewport jumps. Retain the already
requested shared Explain/Analyze toolbar styling.

Shift-click takes priority over double-click detection, so rapid Shift-clicks
extend or shrink selection from its original anchor. Mouse and Shift-navigation
share that anchor, including after whole-line selection. Cover repeated clicks,
direction reversal, Unicode and mixed keyboard/mouse selection in editor tests.

Cover overflow geometry, caret reveal/clamping, Unicode select-all/copy/replace
and undo, plus double-click selection with CRLF, Unicode, empty/final lines and
whole-line dragging. Update README, run fmt/clippy/workspace tests, and rebuild the
macOS app for the user's interactive test. Leave commits to an explicit user request.

## Validation

- `cargo fmt --all` and workspace Clippy with `-D warnings` pass.
- Workspace tests pass: 275 tests, including live PostgreSQL tests against the
  temporary local instance on port 55438. The initial attempt found that instance
  stopped; restarting it resolved the connection failures.
- Rebuilt `dist/FastDB.app`, DMG and ZIP with `--no-install`; signature, Info.plist
  and ZIP integrity checks pass. Launched the rebuilt app for manual review.
- The user approved the final build after the shared editing and Escape changes;
  final validation is recorded in `2026-11-02-shared-text-editing.md`.
