# Metadata capabilities and truthful result states

Implement the approved `DatabaseDriver::metadata_capabilities()` contract with
provider-owned declarations for columns, constraints, indexes, foreign keys,
table details and optional catalog sections. Distinguish available,
not implemented and provider unsupported. Capability declarations describe
implemented readers, not the current user's privileges or live provider validation.

Replace silent optional-metadata fallbacks during schema refresh with retained
per-section outcomes. A failed optional section must not prevent browsing tables,
but its error must remain visible instead of becoming an empty success. Classify
permission errors using native PostgreSQL/MySQL error codes where available;
retain full explicit failure feedback for other errors. Keep existing connection
identity/generation checks and timeouts. Do not add queries, dependencies, config
formats or schemas.

Gate Properties sections from these states before rendering empty grids. Keep
supported partial metadata visible with its limits, distinguish absent native
DDL/statistics from empty metadata, and disable the unimplemented Virtual category
with an explanation. Optional catalog loaders must consult capabilities and show
permission-denied, unavailable, loading, empty and failure states separately.

Tests cover provider declarations, native permission classification, available
empty vs unsupported/default readers, retained optional failure states and UI
message selection. Run full workspace formatting/clippy/tests, live existing PG
and MySQL fixtures and isolated application startup; interactive desktop
acceptance remains pending for the user. Get independent diff review and commit.
This is the metadata portion of P0-01; broader control/platform acceptance remains
open until audited and exercised.
