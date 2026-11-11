# P2 search, plan and transfer completion

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
