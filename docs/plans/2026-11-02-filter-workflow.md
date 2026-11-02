# Structured table filters, history and expanded workspace

Add provider-aware typed criteria using real cached column names, explicit
operators/NULL predicates, AND/OR grouping and optional ordering. Quote values
and identifiers using db-wire dialects; never treat typed values as SQL. A modal
editor supports searchable column selection, multiple criteria, source preview,
Apply (executes) and Cancel. Applying replaces filter text; raw SQL remains usable.

Record actual executed table filter/order/fetch-size snapshots in a bounded
per-tab timeline. Back/forward restores and executes the original snapshot once;
fetch-more and replay do not create history entries, and new execution truncates
the forward branch. Keep text-input undo separate. Capture stable tab/provider
identity; preserve pending edits and block inappropriate active-operation actions.

Wire the data Expand control to temporarily hide navigator/value panels, retain
normal widths and restore using the button or Escape. Hide/disable unavailable
history actions. Fix clause recognition so quoted keyword values do not disable
paging/order. Keep all row results streaming through the existing pager.

Tests: dialect quoting/NULL/value injection, generated SQLite query results,
quoted/nested clause detection, history branching/replay/bounds, workspace checks
and isolated startup smoke. Update README/roadmap and commit. GUI tests remain
pending for the user. This work also closes the known no-op toolbar controls.
