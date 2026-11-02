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


## Verification

Implemented searchable structured criteria, provider literals, ordering, exact
Apply/Cancel, bounded executed-state history with form state and branch/replay
handling, guarded row edits, Expand/Restore and Escape. Preview generation runs
on controller changes and is cached; display is bounded with full-SQL copy.
Inactive values use bounded summaries and active Unicode input windows retain
absolute caret/selection/clipboard semantics.

Formatting, clippy with warnings denied and full workspace tests with PostgreSQL
16/MySQL 8.4 enabled passed. Added real SQLite result tests, PostgreSQL string-mode
round trips, MySQL predicates/numeric comparisons under both backslash modes,
history branching/bounds, Unicode input-window mapping and quoted/nested clause
tests. Other unconfigured provider/TLS/keychain tests retain documented skips.
Manual GUI acceptance remains pending for the user.


## Final checks and review

- `cargo fmt --all` and `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `FASTDB_TEST_PG_PORT=55439 FASTDB_TEST_MYSQL_PORT=55440 cargo test --workspace --offline`: passed, including real PostgreSQL/MySQL fixtures; unconfigured ODBC/TLS/keychain cases retain their documented skips.
- `cargo build -p fastdb-app --offline`: passed; isolated-profile startup remained running for four seconds and the test process was closed. This is startup evidence, not interactive GUI acceptance.
- Independent diff review found cross-field undo leakage after selecting NULL operators. A regression test failed with the former value restored into Search, then passed after clearing the value history on that transition. No other blocking findings were reported.
- On 100,000-row temporary tables, generated numeric range/order filters used the primary index and returned 11 rows without a sort. PostgreSQL estimated 10 rows, read four local buffers, had no spill and took 0.026 ms execution time; MySQL estimated 11 rows and used a covering index range scan (0.14 ms total in the recorded manual run). These validate sargability for these fixtures, not arbitrary user predicates or a performance improvement.

Self-review:
1. Requirements: criteria Apply/Cancel, executed history, branching and Expand/Restore are wired; interactive GUI acceptance is pending by user request.
2. Performance: criteria/values/history/previews are bounded; generated numeric predicates use indexes on 100,000-row fixtures; existing paging is unchanged as approved.
3. Security: operators and columns are validated, identifiers are quoted and values encoded by dialect; injection-shaped text returns only its matching row in live fixtures; no credentials are added.
4. Robustness: provider string modes, stale metadata and staged-edit guards are covered; no dependency or persisted format changes are introduced.
5. Syntax/naming: workspace formatting, warnings-denied clippy, tests and build pass; identifiers follow existing English conventions.
6. Modern APIs: no new deprecated APIs; the touched native gesture guard retains an existing deprecated cocoa bridge, whose migration needs separately approved dependency work.
7. Business clarity: Apply explicitly replaces raw filter text, LIKE remains user-directed and existing paging is preserved per approval.
8. Coverage: SQLite/PG/MySQL result and plan fixtures, timeline, scanner and input-window tests pass; NULL undo regression was observed failing before the fix. ODBC and interactive GUI remain unverified.
9. Design: provider serialization, timeline state and UI controller/rendering are separate; column choices and criterion rows have focused render helpers.
