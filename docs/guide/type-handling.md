# Type handling

- **PostgreSQL**: every column is decoded from the binary wire format. Covered types include integers, floats, `NUMERIC` (exact), `TEXT`, `UUID`, `JSON(B)`, `BYTEA`, dates and times (including `TIMETZ`, `INTERVAL` and ±infinity), `INET`, `CIDR`, `MACADDR`, `MONEY`, `OID`, `"char"`, enums, domains and N-dimensional arrays. A type FastDB does not recognize is shown as text or as a hex literal; it is never shown as `NULL`.
- **MySQL**: result columns are typed from the wire column type. **SQLite**: result columns are typed from the declared column type.

## Value viewer and editor

Select a loaded grid cell and click **Value editor**. Text, JSON and XML use a
multiline source editor; binary columns use a `\x`-prefixed hex editor. The side
viewer reads and formats values on one background worker. Both surfaces reject
values above 256 KiB instead of silently editing a truncated preview. Read and
encoding errors remain errors, distinct from SQL NULL.

**Apply to grid** stages one undoable change; use the table's **Save** to persist
it in the existing transaction. Closing the editor discards its draft. Apply
rejects changed results, connections, schemas or staged edits. Existing query
result restrictions still apply: a value editor does not make unsupported query
columns writable. Binary columns use this editor rather than the inline text
editor. Duplicate preserves binary bytes and accepts at most 1,024 cells within
a 256 KiB value budget.

**SQL NULL**, **Empty value** and **Use default** are distinct. Default is available
only for new rows and omits the column from INSERT. Empty binary is zero bytes,
not NULL. Text containing NUL and invalid JSON are rejected on Apply. JSON's
explicit **Format value** changes only whitespace, preserving number tokens and
duplicate keys. XML stays as source text because formatting can change significant
whitespace; schema-aware XML validation is not provided. The server validates
native column types and constraints on Save.

**Load file** and **Save file** run bounded I/O on workers. Text files use the
selected UTF-8, UTF-16 LE or UTF-16 BE encoding with strict decoding; binary files
always contain exact bytes. Invalid or oversized input leaves the draft intact.
NULL and unset defaults have no file representation. File output replaces the
destination only after a complete temporary file is ready.

For timezone-aware timestamps, **Time: source offset / UTC** changes the displayed
interpretation without changing editor text or stored values. Naive dates/times
are not assigned the host timezone. Encoding and time preferences currently last
for the open workspace; restart persistence belongs to workspace preferences.
Manual GUI acceptance is assigned to the user.
