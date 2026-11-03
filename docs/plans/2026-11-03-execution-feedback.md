# P1-04 execution feedback and controls

## Agreed scope

Keep a bounded execution log in each open SQL tab, surviving runs and result eviction,
but not tab closure or application restart. Add per-statement timeout presets (off by
default), retain workspace-wide single-operation execution, and explain busy refusals.
Internal driver API/event extensions are approved. Do not commit without user request.

## Implementation

1. Add additive command-count and notice events; preserve existing row streaming.
   Capture native PostgreSQL error positions and notices, MySQL warning counts, and
   native affected counts where available; report unavailable counts truthfully.
2. Add bounded tab execution records with SQL previews, elapsed time, row/count
   distinction, messages and errors. Preserve prior records through failures.
3. Wire log/timeout controls into SQL tabs and apply deadlines to pinned and ordinary
   execution without allowing a subsequent run to race cancellation cleanup.
4. Add owning-crate tests and provider integration checks, update guide/roadmap,
   run format, workspace tests, clippy/build and desktop smoke checks.
5. Independently review the diff and requirements; address findings.

## Limits

ODBC informational diagnostics depend on vendor support; do not invent notices or
positions unavailable through a provider. A timeout does not undo committed writes.

## Manual desktop acceptance (deferred to user)

Run `cargo run -p fastdb-app` from this worktree. In a PostgreSQL SQL tab:

1. Toggle Execution log; expect an empty log. Run script:
   ```sql
   CREATE TEMP TABLE fastdb_p104 (id integer);
   INSERT INTO fastdb_p104 VALUES (1), (2);
   SELECT id FROM fastdb_p104 ORDER BY id;
   DO $$ BEGIN RAISE NOTICE 'p104 notice'; END $$;
   SELECT missing FROM fastdb_p104;
   SELECT 999;
   ```
   Expect 2 affected rows for INSERT, 2 returned rows for SELECT, the server notice,
   and a character-8 error on the missing column. SELECT 999 must not run.
   Earlier result tabs and log outcomes must remain readable.
2. Cycle Timeout to 5s, run `SELECT pg_sleep(15); SELECT 999;` as a script. Expect
   timeout feedback, no second statement, and the pinned session closed. Execute
   `SELECT 1` afterward; expect a fresh usable session. Earlier commits may persist.
3. With timeout off, start `SELECT pg_sleep(15)`; try Run script or the execute
   shortcut in another SQL tab. Expect a busy explanation and no queued execution.
   Cancel, wait for cleanup, then run a fresh query.
4. Run more than 20 statements; older grids are evicted but execution log entries
   remain (up to 100). Switch tabs/results and toggle log visibility. Close and reopen
   the SQL tab; the execution log and timeout choice must reset.
5. On MySQL/MariaDB, execute `SELECT CAST('not-a-number' AS UNSIGNED)` and inspect
   warning count/code/text in the log. Check long messages and notice flooding remain
   scrollable and show truncation rather than growing without limit.

Timeout requests cancellation; a race at the final check/send boundary or an
unresponsive vendor may prevent immediate termination. It never guarantees rollback
of already committed SQL. The workspace stays busy until the provider stops.

## Review and verification record

Independent review identified pre-acquisition cancellation and unbounded notice
forwarding; both were addressed with run-scoped driver controls and source-level
notice caps, with deterministic regressions. A second review found no further
material log/count/UI issues; best-effort timeout semantics are documented above.
GUI verification was explicitly deferred by the user, who will exercise these steps.
The user subsequently authorized committing and rebasing this feature onto main.

## Self-review

1. Requirements/correctness: per-statement feedback separates rows/counts, survives later errors and result eviction; native error and zero-row cases have assertions.
2. Performance: row data still streams to the paged store; log/display caches are capped, progress is coalesced and the new relay has fixed capacity. SHOW WARNINGS is a bounded administrative command, not a table query with an applicable EXPLAIN plan.
3. Security: SQL is passed through existing drivers; the new warning command is constant, with no credentials or connection details added to logs or persisted files.
4. Robustness/dependencies: no dependencies or persisted formats changed; notices are capped at source, monotonic deadlines request cancellation once, and operation ownership lasts through cleanup.
5. Syntax/naming: English identifiers follow surrounding Rust patterns; formatting, workspace clippy and build are final gates.
6. Modern APIs: native RowStream rows_affected, connection poll_message, warning and SQLite change APIs were checked against the pinned local dependency sources; no deprecated API added.
7. Business clarity: tab-only retention, timeout-off default, presets and single-operation policy are explicit user-approved choices.
8. Coverage: unit tests cover bounds, timeout acquisition/cleanup, queued SQLite writes and result preservation; integration tests exercise real SQLite/PostgreSQL/MySQL. GUI is deferred explicitly to the user; ODBC/TLS live fixtures remain unconfigured.
9. Design: execution logging, driver deadline control and UI wiring have separate small units; no new dependencies or cross-provider SQL special cases in workspace rendering.
