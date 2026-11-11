# PostgreSQL definition search

Implement the first P2-04 slice. Search PostgreSQL view and routine definitions
within one selected schema. Table data and other providers are separate work.

1. Add a live PostgreSQL fixture with similarly named views and overloaded
   routines, an out-of-scope schema, literal wildcard characters, and more
   matches than the UI can display.
2. Query PostgreSQL catalogs on a dedicated read-only connection. Bind the
   exact schema and a case-insensitive literal substring. Limit results to 50
   plus one overflow match, and bound execution with a server deadline.
3. Open search from the navigator. Show the connection and schema, search
   progress, cancellation, empty/error states, and the result limit. Select a
   result to inspect its bounded source snapshot, associated with its
   PostgreSQL OID, kind and qualified identity. Reject results from replaced connection metadata.
4. Review the catalog query with EXPLAIN on a larger disposable fixture.
   Run the live regression tests, workspace tests, formatting, Clippy, app
   build and startup smoke. Record manual GUI acceptance as pending.

The plan stays uncommitted under the repository's commit-only-when-requested
rule.

## Query plan

PostgreSQL 16, local warm cache. The disposable fixture contained 4,000 views
and 4,000 functions, with 1,000 of each in the selected schema. The matching
query returned 51 rows in 10.462 ms (3,573 shared buffer hits); the no-match
query returned zero rows in 8.988 ms (3,385 hits). PostgreSQL chose sequential
scans of its two catalogs and applied the namespace filter before source
deparsing. The estimates for the matching branches were 313 and 325 versus 333
rows each. The bounded top-N sort used 26 KiB without spilling. No user tables
were scanned or indexed. The no-match case scans the catalogs because the
literal substring is evaluated against deparsed source.
