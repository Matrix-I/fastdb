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

## Verification

Implemented all seven provider catalog sections and asynchronous per-tab cache
loading with metadata-generation/connection identity checks. Real relation
navigation, source copy, retry and distinct unsupported/empty/error states are
wired. Virtual remains explicitly not implemented. Definitions/ACL lists are
read-only and scoped to direct catalog relationships/default/direct grants.

Formatting, workspace clippy with warnings denied and the full workspace suite
with PostgreSQL 16 passed. New live fixtures cover all seven sections, FK/view
navigation, parent/partition bounds, user trigger/rule source, RLS policy roles,
table/column ACLs, empty/missing relations and unsupported provider defaults.
A cache-generation unit test covers invalidating loaded and in-flight sections.
The app built successfully; isolated startup smoke is included. Manual GUI
acceptance is pending for the user, as requested.
