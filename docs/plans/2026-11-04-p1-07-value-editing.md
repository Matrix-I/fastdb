# P1-07 value editing

Add a bounded value editor for text, JSON, XML and hexadecimal binary values.
Open it from the focused result cell. Preserve SQL NULL, empty text, empty
binary and unset insert defaults as distinct states. Apply stages one undoable
grid change and leaves the existing Save transaction responsible for persistence.
Query-result type and key restrictions continue to apply.

Read selected values and files on workers, revalidate tab, pager, edit revision
and driver identity, and report page/read/write errors explicitly. Editing
supports at most 256 KiB of raw value data. UTF-8 is the default text file
encoding. Explicit UTF-16 LE/BE options decode and encode strictly. Binary files
always carry exact bytes. Date/time display preferences affect only presentation.
UTC formatting applies only to timezone-aware timestamps.

Make binary DML type-aware for each supported dialect. Validate staged hex
before generating SQL and preserve original binary values in optimistic row
predicates. Cover SQLite round-trips through real writes and PostgreSQL native
binary handling, plus editor-state and file-format edge cases.

Keep validation/formatting off the render path. Reuse the existing SQL editor
view as a multiline text surface, existing value-format helpers and the app's
file dialogs. Format is an explicit edit. GUI verification belongs to the user.
Run format, Clippy, full tests and build, obtain independent review, commit the
completed step, then remove obsolete build/test artifacts.
