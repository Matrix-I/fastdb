# P1-06 grid productivity

Deliver record inspection first. It shows one loaded row from table or SQL
results with native column types, staged edits, SQL NULL, empty strings,
and unset defaults. Use Previous/Next within loaded rows and return to the grid
without changing edits. Read the row once off the render path, limit the number
and size of displayed previews, and reject stale results after a pager change.

Column visibility and reordering need a shared display-to-source projection
for rendering, selection, keyboard navigation, clipboard, Find and cell edits.
Keep that work separate from record inspection. Multi-column filtering/sorting,
foreign-key navigation, validated paste and keyed query editing remain open.

Verify the record projection against a real paged result and staged edits.
Run workspace formatting, Clippy, tests and build, then review the final diff.

## Record view verification

The owning tests exercise fetched and inserted rows in a real result pager.
They check staged text, SQL NULL, empty strings, unset defaults, deleted-row
status, bounded Unicode previews, invalid row handling and invalidation when a
staged insertion changes row positions. Before the implementation, the targeted
test build failed because the record projection and preview functions did not
exist. With the implementation, all three targeted tests pass.

Independent review found hidden-grid shortcuts, a conflicting Value pane and
a stale pager retained across result changes. Record mode now gates hidden-grid
keyboard actions, hides the Value pane and closes when its result is replaced or
switched. Back to grid preserves an existing selection of the same row.

1. Correctness. Record indexes resolve through the changeset, and snapshots
   expire after staged edits. Pager errors are shown separately from SQL NULL.
2. Performance. A worker reads at most one result page. The view builds at most
   64 previews of up to 512 bytes plus a truncation marker per request.
3. Security. Record display generates no SQL, clipboard writes or value logs.
4. Robustness. Tab and request identity discard late work. The worker owns only
   one pager snapshot and bounded staged field previews.
5. Syntax and naming. Formatting, Clippy and build are workspace gates.
6. Modern APIs. The feature uses existing GPUI task and scrolling APIs.
7. Business clarity. Inspection is read-only and limited to loaded rows.
   The field page size, preview cap and result replacement behavior are documented.
8. Test coverage. Pager and changeset tests cover the projection. Desktop
   interaction remains pending manual acceptance.
9. Design. Record projection, worker loading and rendering are separate from
   grid storage and database execution.

The final formatting check, workspace Clippy, workspace tests with the local
PostgreSQL 16 fixture, and app build passed. The harness reported 442 passes
and zero failures. Unconfigured vendor tests returned early. The built app
stayed running for five seconds using a fresh configuration directory with no
startup errors. This verified startup only. The Record controls still need
interactive desktop acceptance.

## Column display and sorting

Keep edits, metadata and cell coordinates indexed by the result schema. Map
visible display positions to source indexes for rendering, selection, copy,
edit navigation and Find. Store layout per grid, preserve it when refreshing
the same columns and reset it on a changed schema. The header menu moves a
column by one position, hides it, or restores all columns to source order.
Keep at least one column visible. Table header sorting will build an ordered
list of keys with explicit replace and append actions. Preserve that list in
filter history and block server requery while edits are pending.

## Grid controls delivery

- Display columns retain source indexes for edits and row keys. Layout is per
  result and survives same-schema refresh. Selection, copy, Find and edit
  navigation use visible order. Reset restores the source order.
- The table header can replace or append sort keys. Criteria provides compound
  filters. Known primary-key columns break ties for header-sorted paging.
- Plain TSV paste validates the entire rectangle on a worker and stages one
  undo step. Clipboard and original values each have a 256 KiB budget.
  Returning worker results require the same tab, pager, driver, layout, focused
  cell and changeset revision. Type coverage follows the result metadata.
- Explicit NULL for a new row is independent of omitting that field from the
  INSERT. A SQLite read-back test verifies both paths in one row.
- FK navigation handles qualified relationships in either direction. A chooser
  can stage a saved target row's outgoing key as one edit. The source result,
  driver and staged edits must match the snapshot. MySQL/MariaDB current-value
  navigation is restricted to text keys because the structured filter uses
  text literals and implicit numeric coercion can lose precision.

Independent review found and the implementation addressed hidden-column edit
targets, context actions targeting the prior focus, NULL/default loss in SQL
copy and duplicate, paste resource bounds, async revisions after Save,
connection replacement during paste, and picker closure on acceptance.

1. Correctness. Source coordinates survive layout changes. SQLite tests assert
   compound sort order, composite lookup and persisted NULL/default behavior.
2. Performance. Visible order keeps row virtualization. Paste reads off the
   UI thread and has bounded input, cell count and original-value memory.
   SQLite EXPLAIN confirms indexed composite outgoing lookup in the fixture.
   Large production catalogs and incoming relationship plans remain unverified.
3. Security. Sort identifiers and FK filter values use provider-aware SQL helpers.
   Cell writes remain behind the existing explicit Save action.
4. Robustness. Paste and key selection revalidate result, edit and driver
   identity. Successful Save invalidates in-flight row positions.
5. Syntax and naming. Workspace formatting, Clippy and build are final gates.
6. Modern APIs. The changes reuse current GPUI tasks and existing pager access.
7. Business clarity. TSV empty text and the word NULL remain text. Separate
   menu actions stage SQL NULL or an unset new-row default.
8. Test coverage. Tests exercise display order, paste atomicity, lookup key
   staging and real SQLite queries. Native desktop acceptance remains open.
9. Design. Display projection, paste preparation and FK lookup are separate
   from database write orchestration.

## Query origin implementation

The query result editor starts with a provider that supplies
native origin metadata. It requires a single writable base relation and its
complete unique key. It rejects expressions and ambiguous output columns, and
revalidates the relation and key in the write transaction. Tests cover conflicts,
schema drift and concurrent writers.
Do not infer a write target from displayed column labels alone.

### PostgreSQL query result edits

Start with a direct, schema-qualified PostgreSQL `SELECT * FROM schema.table`
against a permanent ordinary table without inheritance or row security. Verify
every output column's native relation/attribute ID and the complete primary
key. Stage updates only. Save uses bound scalar values, checks the prior cell
value and requires exactly one affected row for every update. Revalidate the
origin under a table lock inside the write transaction. Unsupported forms,
types and providers stay read-only.


### Query edit evidence and final review

The PostgreSQL integration test exercises Load editable result, updates a
composite-key row with bound text and NULL, and reads stored values through
another connection. It rejects stale values, key edits, unsupported projections,
views, RLS, user triggers and missing primary keys. A valid first write followed
by an invalid second write rolls back the whole Save. Concurrent writers from
the same original snapshot produce exactly one successful Save.

EXPLAIN (ANALYZE, BUFFERS) on a disposable 100,000-row PostgreSQL 16 table used
the composite primary-key index for the UPDATE. It visited one row, with three
shared buffers in the index scan and ten for the update. Execution measured
0.178 ms. The measured write was rolled back. This is a fixture measurement,
not a production performance claim.

The source gate restricts the initial implementation to direct schema-qualified
SELECT * queries, supported scalar types and permanent ordinary tables with
complete primary keys. PK columns, expressions, views, triggers, rules, RLS and
inheritance remain read-only. Save uses a separate bounded connection and a
short transaction with table locking and exact source revalidation. A failed
Save retains staged values and leaves Cancel available. Successful Save reloads
the verified query, handling committed-but-reload-failed separately.

Independent review found unsolicited origin probes on normal SQL execution,
stale display after successful Save, row buttons enabled for an update-only
workflow, and original-value cloning before its size check. Those findings were
addressed. A second review found that invalidating the origin after failure hid
Cancel. The toolbar now remains visible whenever query edits are pending.

1. Correctness. Native relation/attribute IDs and complete composite keys gate
   edits. Live tests cover stored values, rollback, conflicts and concurrent writers.
2. Performance. Metadata is bounded by one table's columns. Edits cap at 1,024
   cells and 1 MiB. The realistic fixture uses the composite index.
3. Security. Values are bound parameters, identifiers are quoted, roles and
   database identity are checked, and unsupported write surfaces stay gated.
4. Robustness. Load and Save have 15-second bounds with shorter SQL/lock bounds.
   Socket guards discard cancelled work. Commit uncertainty requires verification.
5. Syntax and naming. Formatting, full workspace Clippy, tests and build passed.
6. Modern APIs. Native origin metadata uses the pinned tokio-postgres APIs.
7. Business clarity. Load editable result uses an isolated connection. Save
   commits separately from the SQL tab transaction and the UI states that scope.
8. Coverage. 453 harness passes with zero failures in the final full workspace
   run, including real PostgreSQL fixtures. Unconfigured vendor tests skip.
9. Design. Provider verification and writing live in query_edit. The app owns
   source identity, staged-state guards and result feedback.

Computer Use could not inspect the desktop because permissions were unavailable.
The user explicitly assigned GUI verification to themselves and requested that
Computer Use never be requested. This preference is recorded in CLAUDE.md.
Manual acceptance remains pending and is separate from implementation checkboxes.
