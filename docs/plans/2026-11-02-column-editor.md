# PostgreSQL column editor

Open a dedicated column tab from Properties or Navigator, deduplicated by
connection/database/table OID/attribute number after catalog resolution. Maintain
an original snapshot and a draft; expose name, native type declaration, optional
USING expression, nullable, explicit default set/remove, and multiline comment.
Identity/generated/collation/storage/origin/ordinal remain visible read-only;
restrict mutations to local ordinary, non-inherited, non-partitioned tables.

Provider API owns catalog reads and atomic schema changes. Use a dedicated client,
ACCESS EXCLUSIVE table lock, lock/statement timeouts and compare the original
snapshot under lock before mutation. Generate quoted DDL, drop/reapply defaults
around type changes, rename last, never guess conversions or DROP CASCADE. Commit
acknowledgement/read-back failure requires Refresh before another attempt. Stable
OID/attribute identity detects concurrent rename/drop/recreation. Refresh all
owning connection caches after success; flag old table result grids as stale.

UI: catalog-backed type suggestions and native declaration; Save produces a SQL
preview and Persist applies exactly that draft. Revert discards locally. Preserve
drafts/errors through failures; guard close/refresh/disconnect/Quit with
Save/Discard/Cancel. Busy operations cannot be closed mid-commit. Do not share SQL
tab sessions. Provider capability gates unsupported entries.

Tests cover quoting, fragment validation, default NULL vs remove, rename ordering,
conflicts, null/conversion failures, identity/generated/inherited restrictions,
transaction isolation and read-back. Run fmt/clippy/workspace/live PG tests, build
and launch/package the app for manual GUI review. Commit this plan separately,
then commit implementation after checks at the user's request.

Reference: https://www.postgresql.org/docs/17/sql-altertable.html (type USING does
not convert defaults; inheritance/partition restrictions and locking apply).


## Implementation and verification

Implemented PostgreSQL native snapshots, OID/attribute identity, catalog type
suggestions, typed drafts and quoted DDL preview/Persist. Applied edits run in a
dedicated locked transaction and preserve unrelated SQL-tab transactions.
Catalog conflicts reject the draft; uncertain commit/read-back outcomes require
Refresh. Specialized columns/properties remain explicitly read-only as scoped.

Added column tabs from Properties/Navigator double-click or selected-row Enter,
multiline field editing, default/comment enable/removal, Save/Revert/preview and
dirty-action guards for close/refresh/disconnect/delete/Quit. Connection generation
checks reject stale loads or Persist after reconnect. Refresh routes to the owning
connection; staged row edits are preserved but blocked from Save until reload.

Formatting, workspace clippy with warnings denied and the full workspace suite
with live PostgreSQL 16 passed. Live fixtures verify quoted rename, Unicode and
multiline/backslash comments, NULL/removal defaults, NOT NULL/type-error rollback,
catalog conflicts, dropped/recreated identity, inherited/generated/identity
restrictions and unrelated transaction isolation. Unit tests cover fragment
boundaries, quoted preview, dirty/preview state and multiline draft values.
The debug app built; isolated startup smoke testing is included. Manual GUI
acceptance remains pending for the user, as requested, and is not a blocker to
continuing subsequent implementation milestones.
