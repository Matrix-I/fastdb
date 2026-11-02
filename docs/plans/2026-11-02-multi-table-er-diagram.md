# Multi-table ER diagram

Replace the single entity preview with a connection-scoped ER diagram. Load
structured FK column mappings in the background for PostgreSQL, MySQL/MariaDB
and SQLite; unsupported providers explicitly report unavailable relationships.
Never parse DDL strings or infer relationships from column names.

Build a deterministic cached graph during metadata application, with qualified
table identities, PK/FK columns, composite/self/cross-schema links and stable
non-overlapping positions. Render only visible cards; keep graph discovery and
layout off the render path. Each table tab owns zoom and scroll state. Provide
native two-axis wheel/trackpad scrolling with visible horizontal/vertical
scrollbar tracks, middle-button pan, zoom controls, fit/reset and focus-table.
Label FK source/target columns and direction; double-click a table to open it.
Refresh replaces cached graph data without mixing connection identities.

Test FK metadata, qualified identities, composite/self/cross-schema edges,
deterministic layout, visible-card selection and zoom limits. Update README and
provider docs. Run fmt, clippy, workspace tests, configured live tests and launch
an isolated app; report GUI access limitations. Do not commit unless asked.

## Validation

- Implemented structured FK discovery for PostgreSQL, MySQL/MariaDB and SQLite,
  cached graph/layout, qualified identities, PK/FK badges, directed column links
  and selectable constraint details. SQLite column metadata now binds table names
  safely through pragma_table_info, including names containing quotes.
- Added per-tab zoom, native two-axis scrolling and visible scrollbar tracks,
  middle-button pan, Fit/Reset/Focus table and connection-aware table navigation.
  Viewport changes invalidate card culling after initial layout and resize.
- Formatting, workspace clippy with warnings denied and whitespace checks passed.
- The complete workspace suite passed with disposable PostgreSQL 16 and MySQL 8.4
  enabled. Added live FK tests passed for both providers; SQLite tests cover
  composite implicit target PKs, self references and quoted identifiers.
- Graph tests cover schema identity, composite/self/external references, stable
  ordering, tall-card band layout, viewport culling, scroll reachability and zoom
  limits. The first scroll test assumed the last card occupied the rightmost
  column; it now checks the reachable offset of the actual last card.
- The initial MySQL container exited during initialization. A smaller replacement
  reached readiness and passed the live test and full suite. Test containers were
  stopped and removed.
- The final app built successfully. Interactive scroll/zoom/diagram acceptance
  remains pending because Computer Use permissions are not granted.
- Concurrent roadmap, Properties layout and Quit changes are outside this work;
  their files and unrelated README edits were preserved. No commits requested.


## Follow-up from GUI feedback

The supplied screenshot shows the previous single-table renderer. The installed
~/Applications bundle predates the new implementation; rebuild dist/FastDB.app
for review. Add Cmd/Ctrl plus/minus/reset and modifier-wheel zoom; bridge native
macOS magnify events because GPUI 0.2 does not expose them. The bridge is local to
this app, main-thread-owned and removed with the workspace. Pinch zoom is gated
to the diagram viewport and excludes modal overlays. Keep normal trackpad scroll.

Widen cards for complete names/native types, keep every column in the scene, and
provide a column inspector in the footer for nullable/PK/FK/default/collation/
comment metadata. Keep both scrollbars and real overflow extents after zoom.
Validate zoom anchoring and shortcut variants, rerun workspace checks, smoke
launch the updated build and package the macOS app without installation.

- Follow-up workspace tests and clippy passed. Binary-marker checks confirmed both the installed app and previous dist bundle lacked the multi-table toolbar. The macOS release bundle was regenerated without installation; app/DMG/ZIP output and strict code-signature verification passed. Binary markers confirm the packaged app contains multi-table rendering, column properties and native magnify-event handling. Native gesture behavior still requires interactive acceptance because Computer Use permissions are not granted.
