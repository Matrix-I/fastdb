# Pinned PostgreSQL script sessions and explicit transactions

## Objective

Run a PostgreSQL script on one dedicated physical connection, so transaction
blocks, temporary tables and session settings work across its statements without
sharing state with navigator metadata, table browsing or row-edit Save operations.

For example, Run script should execute the following exactly once, preserve each
statement result and commit the inserted row:

```sql
BEGIN;
CREATE TEMP TABLE batch_input (value text);
INSERT INTO batch_input VALUES ('example');
SELECT * FROM batch_input;
COMMIT;
```

## First delivery and boundaries

- PostgreSQL first, through provider capabilities and a shared driver/session
  abstraction. Keep the existing execution behavior for other providers.
- Pin the connection for one complete Run script, including cleanup. Release it
  after completion, error or Cancel. This first delivery does not persist temporary
  state or open transactions across separate Cmd/Ctrl+Enter executions.
- Support ordinary statements and explicit BEGIN / START TRANSACTION, COMMIT and
  ROLLBACK blocks. Define accepted aliases/options in parser tests before enabling
  them; reject unsupported transaction forms before any statement executes.
- Require each transaction block to close within the script. Support multiple
  successive blocks and ordinary autocommit statements between them. Reject
  nested BEGIN, unmatched transaction boundaries, transaction chaining, prepared
  transactions and savepoint controls in this first delivery.
- Keep procedural/client batch syntax (GO, DELIMITER, unquoted blocks) outside
  this change. Preserve existing dollar-quoted bodies and literal/comment rules.
- Stop at the first SQL error or cancellation. Roll back the currently open
  transaction and close the session; do not imply that earlier autocommit writes
  or already committed blocks have been undone. Never add an implicit transaction.
- Retain streaming into the paged/spilling result store and the latest 20 results;
  execute script statements as written without OFFSET replay.

A later delivery can add a persistent session per script tab, interactive
Begin/Commit/Rollback controls and savepoints. That is a separate lifecycle change.

## Existing code and implications

- `DatabaseDriver` exposes statement execution and driver-level cancellation but
  has no explicit leased session API. PostgreSQL currently caches an Arc<Client>
  shared by query, metadata and row-edit calls; reusing it is not isolation.
- `PostgresDriver::get_or_connect` reconnects closed clients. A pinned script must
  instead fail terminally if its physical connection disappears, never continue
  subsequent statements on a replacement connection.
- `split_script` currently rejects transaction-related word tokens. Extend it
  through a provider capability/policy while preserving the conservative path for
  existing callers; do not simply remove the forbidden-word list.
- `pending_statements` and per-statement operations currently sequence a script
  in the UI. A run must retain the same session and cancellation owner until
  rollback/close completes, rather than release ownership between statements.

## Implementation sequence

1. **Driver session boundary** (`crates/db-wire/src/traits.rs`). Add an optional
   pinned-script capability and a Send + Sync session handle for streaming one
   statement, out-of-band cancellation and explicit finish/abort cleanup. Defaults
   remain unsupported for other providers. Capture connection configuration once
   at run start; do not add PostgreSQL special cases to workspace orchestration.
2. **Dedicated PostgreSQL implementation** (`crates/db-wire/src/postgres/`). Reuse
   connection/TLS/socket setup and binary decoding/RowSink code, but create a
   dedicated client with its own cancellation token and serialized execution.
   Metadata and row-edit operations keep their existing driver connection. Avoid
   duplicate streaming implementations by extracting the shared client execution
   path. Do not reconnect, retry statements or retry COMMIT automatically.
3. **Script preflight** (`crates/sql-intel/src/script.rs`). Separate lexical
   splitting from capability-aware statement/control classification. Validate the
   entire transaction boundary sequence before execution. Classify leading tokens
   rather than matching keywords inside literals, comments or quoted identifiers.
   Preserve current rejection of ambiguous/session-dependent lexical forms.
4. **Run ownership** (`crates/app/src/workspace/{query,operations,tab_state}.rs`).
   Own the session by stable connection/tab/run identities, with statement
   generations inside that run. Wait for cancellation acknowledgement and cleanup
   before starting another statement/run. Route every execution and cancellation
   through the original session. Suppress stale completions after tab/reconnect
   changes and retain per-statement results/errors.
5. **Cleanup and UI** (`script_view.rs`, `script_saves.rs`, `tab_context_menu.rs`).
   Display dedicated-session run/cleanup status. Preserve autosave barriers before
   execution and existing close/quit flow; close/disconnect requests cancel the
   owning run and finish rollback/session close in the background. Bound cleanup
   waits, invalidate a failed session and surface cleanup failure separately from
   the original SQL error. Never reuse a possibly aborted session or report an
   uncertain COMMIT outcome as a successful rollback. No database work in render.
6. **Documentation and delivery**. Update README supported execution and remaining
   limitations, and PROVIDERS session/cancellation/TLS validation. Run the checks
   below and package the macOS app for the user's manual GUI test. Keep this plan
   in its own commit if the user requests committing; no implementation is
   authorized merely by this document.

## Verification

- Parser tests: quoted/commented keywords, embedded semicolons/dollar quoting,
  closed/multiple transaction blocks, unsupported aliases/options, unmatched or
  nested controls, unsupported providers and complete preflight before writes.
- Live PostgreSQL tests: identical backend PID within a script; temporary table
  and SET/SET LOCAL visibility; commit persistence; explicit rollback; SQL error
  and pg_sleep cancellation rolling back current work; earlier committed/autocommit
  work remains; no state leakage to another script or metadata/row-edit sessions.
- Lost connection tests: terminate the dedicated backend between statements;
  assert remaining statements do not run and no reconnect/replay occurs. Verify
  idle-driver queries and subsequent new script runs still work.
- TLS tests: dedicated session uses certificate/hostname validation and its own
  TLS cancellation path. Keep live tests gated by FASTDB_TEST_* and mandatory in
  configured CI via FASTDB_REQUIRE_DB_TESTS.
- Fake-session app tests: exactly-once sequencing, first-error stop, late events,
  tab switching/closing, disconnect, cancellation between statements, cleanup
  failure and a cancellation request that must not hit a subsequent statement/run.
- Regression checks: table queries, row edits, standalone script queries and
  non-PostgreSQL Run script retain their existing behavior.
- `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, live PostgreSQL/TLS tests where available, and
  `./bundle_macos.sh --no-install`.
- Manual GUI acceptance by the user: run the example, inspect individual results,
  test rollback/error/Cancel, switch tabs/connections during a run and close a
  running tab. Verify cleanup finishes and later scripts can execute normally.

## Status

Planning only. No runtime code, roadmap completion flags or build artifacts changed.
The user requested committing this plan separately. Implementation and push remain unrequested.
