# PostgreSQL Properties catalog readers

Implement read-only Dependencies, References, Partitions, Triggers, Rules,
Policies and Permissions using a provider catalog-section API. Return unsupported
separately from empty results/errors. Preserve schema-qualified relation targets
and catalog definitions; display direct dependencies/grants without claiming
transitive dependencies or effective inherited privileges.

Load each selected section asynchronously on a dedicated read-only PostgreSQL
connection with bounded timeouts. Cache per table tab and metadata generation;
ignore results after refresh/reconnect/disconnect/tab close. Provide retry/refresh,
copy source and relation navigation only where a real relation target exists.
Virtual relations remain explicitly not implemented. Preserve both scrollbars.

Add isolated live fixtures for all sections, missing/empty targets and provider
unsupported defaults. Run formatting, clippy/workspace tests and startup smoke.
Update README/provider docs/roadmap and commit the implementation separately.
GUI acceptance remains pending for the user.

Sources: PostgreSQL 17 catalogs pg_depend/pg_policy and system-information
functions (pg_describe_object, aclexplode); use catalog identities, never parse DDL.
