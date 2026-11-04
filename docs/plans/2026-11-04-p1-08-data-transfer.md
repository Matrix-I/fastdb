# P1-08 data import, transfer and full-source export

## Contract

Implement the complete P1-08 roadmap item: CSV import with delimiter, encoding,
header, NULL and column/type mapping; bounded validation preview; batching and
explicit error policy; table/query-to-table transfer; full-source streaming
export; progress/cancellation and read-back evidence. Keep each operation tied
to its captured source and destination, rejecting stale UI state before writes.
Do not close this item after delivering only CSV parsing or loaded-row export.

The existing P1 completion plan records authorization for necessary additive
public APIs, backward-compatible configuration and dependency changes. This step
will reuse dependencies where possible. GUI acceptance belongs to the user;
Computer Use is excluded. Live vendor ODBC checks remain deferred to Linux.

Default policy is one transaction, rollback on any parse, validation, database or
cancellation error. Offer explicit commit-per-batch with durable row/batch counts
and a clear partial-commit outcome. Never retry writes automatically. Commit or
connection uncertainty must be reported as unknown, not as zero committed rows.
Reject nontransactional destinations where the selected contract cannot hold.

## Architecture and sequence

1. Build a streaming CSV reader with strict UTF-8/UTF-16 LE/BE decoding, RFC-style
   quoting, configurable single-byte delimiter and header handling. Default NULL
   token is unquoted `\N`; quoted tokens and quoted/unquoted empty text remain
   values. Bound a field at 256 KiB, a record at 1 MiB and width at 1,024 fields.
   Malformed quotes, encoding, width and oversized records produce explicit
   record-numbered errors. Test multiline values, CRLF, escaped quotes, Unicode,
   NULL/empty fidelity and one-byte-at-a-time reads before wiring persistence.
2. Add column/type mappings with explicit ignored/default columns and bounded
   preview. Preserve exact decimal text and binary hex; reject invalid values
   without silently substituting NULL or replacement characters. Preview is not
   a promise that later records or server constraints will pass.
3. Add parameterized native batch insertion on owned physical sessions. Reuse
   existing session lifecycle/cancel/cleanup patterns for PostgreSQL, MySQL and
   SQLite, and expose provider capability limits explicitly. Bound batch bytes,
   rows and parameter count; use set-based INSERT rather than one round trip per
   row. Validate metadata/transaction capability before beginning writes.
4. Run imports and table/query transfers off the UI thread. Own source/destination
   sessions through cancellation and rollback/close acknowledgement. Keep progress
   replaceable and bounded, report processed/committed/rejected counts, and make
   stop-on-error versus explicitly selected skip-invalid-record behavior clear.
   Syntax/encoding failures stop because record recovery may be ambiguous.
5. Add full-source CSV/SQL export, separate from existing loaded-row export. An
   explicit source run respects the actual query and its limits, or table filters
   without the grid's paging limit. Use an isolated read-only source where
   supported, existing paged streaming with bounded RAM and deterministic spill
   cleanup, and atomic destination-file replacement. Do not replay a mutation or
   use a SQL-tab transaction as the export source. Preserve NULL, empty and binary
   values in the new transfer format. Document temporary disk use honestly.
6. Wire a source/settings/preview/mapping/confirmation/run workflow into table
   and SQL-result toolbars. Destination writes require explicit selection and
   confirmation of the displayed transaction/error policy. Retain ownership in
   the workspace operation gate through cleanup and prevent duplicate starts.
7. Verify real SQLite, PostgreSQL and MySQL imports/transfers/export read-back,
   constraints and rollback, explicit batch partial commits, cancellation,
   encoding/mapping failures and bounded large-source behavior. Inspect plans on
   realistic data for new nontrivial SQL. Update guide/provider/roadmap status.

## Delivery gates

After the final code edit, read the complete diff including untracked files, run
workspace format/Clippy/tests/build, launch the binary for startup evidence and
obtain independent review against the entire requirement. Record all nine
repository self-review items and any remaining GUI/vendor acceptance gaps.
Commit the completed P1-08 implementation before P1-09. Clean obsolete debug,
incremental and temporary artifacts while retaining the current binary and
reusable development/test dependencies.
