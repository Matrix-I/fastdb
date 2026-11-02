# Explain and Analyze on persistent SQL-tab sessions

Route PostgreSQL Explain/Analyze through the SQL tab's existing session runner.
Reuse the autosave barrier, stable tab/run ownership, streaming, cancellation,
transaction failure handling and cleanup. Do not fall back to the shared driver.

Capture plan-result intent with the run, and apply it after StatementStarted
initializes the result (initialization clears pending plan state). Preserve raw
JSON, parsed tree, result switching and ordinary execution behavior. Analyze
continues to execute the original SQL and can modify the tab's open transaction.

Test result initialization and PostgreSQL plans over temporary tables, SET LOCAL,
uncommitted writes, savepoint recovery and tab isolation. Run workspace formatting,
clippy/tests and live PostgreSQL tests, launch the app with an isolated profile,
and document any GUI permission limitation. Update README and provider docs.
Keep changes uncommitted until requested.

## Validation

- Formatting, workspace clippy with warnings denied, workspace tests and diff
  whitespace checks passed.
- All seven PostgreSQL session integration tests passed on a disposable
  PostgreSQL 16 server. New tests cover estimated/analyzed plans over a temporary
  table, SET LOCAL planner settings, exactly-once Analyze writes within an open
  transaction, savepoint recovery, tab isolation, and Analyze cancellation with
  rollback followed by a successful fresh run.
- The result lifecycle test confirms plan intent survives initialization, raw
  JSON becomes a plan, ordinary execution clears it, result switching restores
  the matching plan and SQL errors clear pending intent.
- The updated app built and remained running during an isolated startup check,
  then the smoke-test process was closed. Interactive GUI verification remains
  pending because Computer Use permissions are not granted.
- Live TLS tests were not configured. Production driver/TLS code is unchanged.
