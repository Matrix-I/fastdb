# Transactions for editable table tabs

## Contract

Each table tab owns a dedicated session, as SQL tabs already do. In Auto-Commit,
Save retains its atomic immediate-commit behavior. In Manual mode Save sends the
staged edits without committing; Commit first applies all pending cell/row edits
then commits the tab transaction. Rollback discards both staged edits and writes
previously sent by Save, then reloads the table. Refresh and paging in Manual mode
read the same session and can see its uncommitted writes. Close/disconnect closes
the session and rolls back. Failed Save must not commit or partially apply a batch.

## Implementation

Reuse the existing db-wire TabSession/run_tab_script API without changing public
traits or adding dependencies. Run generated row statements sequentially with
native affected-row checks, a batch savepoint and bounded deadlines. Failed batches
roll back to their savepoint; failed cleanup closes the owned session. Do not retry
writes or uncertain commits. Preserve table identity/grid when applying session
settings and transaction boundaries. Update transaction UI availability for tables.

## Validation

Use real file-backed SQLite sessions for Save isolation, read-your-writes,
Commit-with-pending-edits, Rollback, batch conflicts/errors, session loss and
independent tab ownership. Add optional PostgreSQL/MySQL live cases that skip when
FASTDB_TEST variables are absent. Run full workspace tests, formatting, Clippy,
build and launch; get independent review. User performs manual GUI acceptance.
Keep this plan in a separate docs commit when the user requests committing.
