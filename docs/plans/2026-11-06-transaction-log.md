# Session-only SQL transaction log

## Requested behavior

Split the transaction toolbar into independent hit targets: the T icon and arrow
open the existing mode/isolation menu; the Auto/Manual label opens Transaction log.
Record every executed SQL statement, not only statements in manual transactions.
Keep the log for the current app process, including after tabs and connections close.
Store records in a private temporary file rather than retaining the session in RAM.
Closing the panel drops loaded records; normal application exit deletes the file.
A crash cannot guarantee exit cleanup, so stale files must be removed on a later
startup without touching another running process's files.

## Proposed implementation

Instrument database execution boundaries, including script statements, table reads
and writes, transaction controls, plans, imports and application-issued metadata SQL.
Record actual attempts, including failures and cancellations, without executing or
retrying SQL for logging. Do not record unexecuted queued statements. Preserve bound
parameter separation and never include connection credentials or result values.

Use an additive db-wire execution observer interface; do not change existing driver
or session method signatures. Attach connection/session identity to events. The app
owns a bounded writer worker and private temporary file. Logging failures surface
in the panel without changing a database operation's outcome. File reads/writes
stay off the GPUI render/UI path. Page and search records from disk with bounded
memory; release page data and cancel outstanding reads when the panel closes.

The panel presents Time (UTC), Type, Text, Duration, Rows and Result, with search.
Transaction grouping and the reference image's optional visibility filters require
explicit definitions before implementation if they are included.

## Validation

Test independent toolbar routing, successful/error/cancelled statements, scripts
stopping before later statements, concurrent append/read, bounded paging/search,
close/reopen without data loss, cleanup and I/O errors. Exercise real SQLite SQL
and normal application shutdown. Run formatting, full workspace tests, Clippy and
build, then launch the app. Obtain independent diff review. Manual GUI acceptance
belongs to the user; do not use Computer Use.

## Approval boundary

The repository requires asking before public API changes. Obtain approval for the
additive db-wire observer before implementation. Do not commit this plan unless the
user asks to commit; this follows the explicit no-commit rule.
