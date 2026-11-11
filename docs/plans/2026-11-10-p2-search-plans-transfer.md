# P2 search, plan and transfer completion

## Status audit — 2026-11-12

Source baseline: `76e8815`. Checked items below mean implemented in the stated
scope with regression tests present, not a fresh live-provider or GUI pass.
The original delivery outline below is retained as historical intent.

### P2-05 — Partial

- [x] SQLite estimated plans on the existing SQL-tab session: native
  `EXPLAIN QUERY PLAN`, operator tree and raw rows, without invented metrics.
  `crates/app/src/workspace/query_plan.rs` includes a real indexed TEMP-table
  session test, hierarchy validation and statement/PRAGMA/Analyze guards.
- [ ] MySQL/MariaDB and ODBC plan adapters and live-engine verification.
- [ ] Expensive-node navigation, saved plan comparison and remaining
  provider/relation metadata and DDL coverage.
- [ ] Manual desktop acceptance of SQLite Explain.

### P2-04 — Implemented subset; original scoped UI no longer exposed

- [x] PostgreSQL, MySQL and SQLite definition-search driver implementations
  and regression fixtures (`db-wire/src/{postgres,mysql,sqlite}/` and
  `db-wire/tests/{catalog_test,mysql_test}.rs`).
- [x] Single-table data search from the open table footer, cancellation,
  bounded hits, connection/metadata guards and row previews. PostgreSQL
  reopens the matching row version; MySQL/SQLite use captured previews.
  Evidence: `table_view.rs`, `definition_search/data.rs` and provider tests.
- [x] Historical live and desktop search acceptance recorded in the
  [search plan](2026-11-10-p2-04-search.md); not rerun by this source audit.
- [ ] Original navigator definition search and multi-table scope workflow:
  these entry points were removed. Driver support and old acceptance do not
  establish a currently reachable UI. Restoring them requires a scope decision.

### P2-07 — Partial

- [x] Loaded-result JSON array-of-objects export; duplicate column names
  rejected, numeric tokens preserved, SQL NULL distinct from JSON source
  text and empty text. File-content and failure-preservation tests are in
  `crates/app/src/workspace/export.rs`.
- [x] Loaded-result HTML/Markdown export with escaping and NULL/binary tests.
- [x] Focused ER SVG export captures card positions and connector routes;
  `diagram_panel/svg.rs` tests exported contents and moved positions.
- [ ] JSON import and full-source JSON export (streaming full export still
  supports CSV/SQL only).
- [ ] Spreadsheet import/export, additional format controls and result
  aggregates/charts.
- [ ] Manual desktop acceptance of the new export formats and SVG.

## Audit validation

This documentation-only audit did not change runtime code. Formatting,
workspace Clippy and workspace tests were rerun. The initial sandbox test
failed because the SSH fixture could not bind a local socket; the complete
workspace rerun outside the sandbox passed. Live PostgreSQL/MySQL/ODBC
checks remain conditional on the FASTDB_TEST_* environment; passing harnesses
that return early do not establish live-provider acceptance. No new GUI,
large-fixture query-plan measurements or app startup is claimed. An independent
source/diff review corrected the P2-04 completion checkbox and export scope.

## Original delivery outline

The first P2-04 slice searches PostgreSQL definitions in one schema. The
roadmap remains the completion checklist for all three items.

1. P2-05: adapt native SQLite, MySQL/MariaDB and ODBC plans to the existing
   plan viewer. Preserve the tab session, separate estimated and actual
   metrics, retain raw output, and verify against live engines. Add expensive
   node navigation and saved plan comparison. Audit missing metadata and DDL
   by provider and relation kind.
2. P2-04: extend definition and data search to scoped server work with
   deadlines, cancellation, bounded results and navigation to the exact
   object or row. Check authorization, literal matching, type semantics and
   large-schema query plans with isolated fixtures.
3. P2-07: extend streaming import/export with JSON and spreadsheets, add
   HTML/Markdown and format controls, and verify contents and type/NULL
   fidelity. Add result aggregates/charts and ER image export with documented
   scope and limits.

Start with SQLite estimated plans. Use one SQLite statement and native
`EXPLAIN QUERY PLAN` on the SQL tab's existing connection. Show the tree
and raw rows without inventing costs, estimates or actual timings. Restrict
SQLite Explain to queries and DML because PRAGMAs may run during preparation. Verify
against a real indexed TEMP table, malformed plan rows and statement
boundaries, then run workspace checks and app startup. Manual desktop
acceptance remains separate. Keep this plan in the working tree because
commits require a separate request.


JSON uses an array of objects keyed by column name, as selected by the user.
Reject duplicate names instead of discarding values. Preserve numeric tokens
and distinguish SQL NULL from JSON column source and empty text. The SVG
export takes an immutable snapshot of the focused diagram, including current
card positions and connector routes.
