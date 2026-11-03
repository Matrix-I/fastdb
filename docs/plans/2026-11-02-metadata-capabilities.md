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


## Verification and review

Implemented provider-owned declarations, retained optional reader states, native
permission-code classification, Properties gating, real FK mappings on partial
providers and disabled Virtual navigation. Independently loaded native definitions
remain visible if a summary reader fails; that failure is shown as a notice.
No production catalog SQL, dependency, persisted schema or config format changed.

Reproductions on macOS/Rust pinned workspace, branch feature/roadmap-completion,
base `fad503b` plus the uncommitted feature under review:

- SQLite: create `VIEW broken AS SELECT id FROM missing_relation`, then call
  `list_all_columns()`. Expected error; formerly successful empty result. The
  regression failed before propagation and passes afterwards; table discovery
  remains usable with an explicit failed Columns state.
- PostgreSQL 16, isolated local test container: transaction-local catalog SELECT
  revocation and a temporary role make the actual column/details/catalog query
  fail. Expected permission-denied classification; formerly formatted strings
  lost SQLSTATE. The column-reader regression failed before source preservation;
  all three metadata boundaries now pass and each transaction rolls back ACL/role
  changes before assertions. A streaming-query 42501 fixture also failed before
  retaining native sources and passes afterwards. Event message formats remain
  unchanged.
- Mixed metadata: failed summary readers with successful TableDetails/native DDL.
  Expected detailed sections stay visible; test formerly returned blocking
  `summary failure` and now passes. Independent review identified this case and
  the native SQLSTATE loss; both were addressed. Follow-up review caught a
  duplicate fallback notice, which was removed.

Checks:

- `cargo fmt --all` / `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `FASTDB_TEST_PG_PORT=55439 FASTDB_TEST_MYSQL_PORT=55440 cargo test --workspace --offline`: passed, including 123 app tests, native metadata permission boundaries and existing live PG/MySQL suites. Unconfigured ODBC/TLS/keychain tests retain their documented skips.
- `cargo build -p fastdb-app --offline`: passed.
- Rebuilt app startup with an isolated `FASTDB_CONFIG_DIR`: remained running for
  four seconds; test instance terminated normally. Interactive GUI acceptance is
  deferred by explicit user instruction and is still unverified. This milestone
  does not close the full P0-01 control/platform audit.

Self-review:

1. Requirements: missing/unsupported/permission/failure/empty outcomes are distinct; disconnected and mixed-success paths retain accurate feedback.
2. Performance: optional batches run concurrently with five-second deadlines; unavailable readers are not invoked; metadata states share one Arc per refresh and no new production query is introduced. Existing catalog volume limits remain unchanged, not newly certified.
3. Security: no auth/privilege policy changes or credentials are added; native error classification is read-only. Test ACL/role changes are transaction-local in the isolated fixture and rolled back; independent review covered these changes.
4. Robustness: optional failures preserve table discovery, provider declarations remain separate from live permissions, connection identity/generation guards remain intact and no dependencies/config formats change.
5. Syntax/naming: pinned-toolchain build, formatting and warnings-denied clippy pass; English names match the surrounding modules.
6. Modern APIs: new code uses existing supported Rust/provider APIs and introduces no deprecated calls.
7. Business clarity: capabilities describe implemented readers only; unsupported features are explicitly listed, and GUI/provider validation is not inferred from declarations.
8. Coverage: before/after bug reproductions and empty/error/mixed-result tests pass, with real provider fixtures; interactive GUI and unconfigured ODBC/TLS cases remain unverified as documented.
9. Design: provider capabilities, UI states, load orchestration and section rendering remain separate; existing driver/Properties patterns are retained.
