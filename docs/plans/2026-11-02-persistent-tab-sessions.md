# Persistent PostgreSQL tab sessions

Keep one lazily opened PostgreSQL session per SQL tab for Execute and Run script.
Preserve temporary tables, settings and open transactions between runs. Add
Begin/Commit/Rollback controls and allow SQL SAVEPOINT/ROLLBACK TO/RELEASE.
Display transaction state derived from validated top-level controls. SQL errors
retain the session so users can recover an aborted transaction; cancellation
closes it after bounded rollback cleanup. Never reconnect or replay a lost session.
Closing a tab or disconnecting aborts its session, causing server rollback.

Implementation: add a tab-owned session holder with deterministic socket closure;
reuse the existing serialized runner and operation/cancellation barrier; add a
persistent-session parser policy and transaction state updates; route standalone
script execution and toolbar controls through that runner. Preserve other drivers.

Verify parser boundaries/savepoint forms, session reuse/isolation and error
recovery, cancellation and owner drop. Run formatting, workspace clippy/tests,
and launch the application for UI checks. Update README/provider documentation.
This plan and implementation remain uncommitted until the user requests commits.


## Validation

Implemented persistent tab ownership, Execute/Run script routing, toolbar controls,
SQL savepoints, acknowledged transaction-state updates and disconnect cleanup.
Workspace tests and clippy with warnings denied passed. All five PostgreSQL
session integration tests passed against a disposable PostgreSQL 16 server,
including cross-run savepoint recovery and isolation between two tab owners.
Formatting and diff whitespace checks passed. Live TLS tests retain their
environment-based skips. The app was built and launched with an isolated profile;
interactive GUI acceptance remains pending because Computer Use permissions are
not granted. No commits, installation or release were performed.
