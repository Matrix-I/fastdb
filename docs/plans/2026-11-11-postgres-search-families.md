# PostgreSQL definition search families implementation plan

> **For agentic workers:** Use executing-plans-strict. Steps use `- [ ]`.

**Goal / Architecture / Tech Stack / Branch:** Expand P2-04 on the current
working tree. Extend the scoped native PostgreSQL query, its Rust result
mapping and GPUI titles. Continue on `feature/p2-04-search`.

## Confirmed Requirements

- P2-04 covers PostgreSQL, MySQL and SQLite. The user approved additional
  definition result kinds. Search is read-only, scoped, bounded and cancellable.

## Global Constraints

- Keep terms and scope bound. Apply schema filtering before deparsing.
- Preserve the existing result and source bounds, privileges and cancellation.
- Run format, Clippy, workspace tests and build. Verify with a disposable
  PostgreSQL server and inspect query plans. Desktop acceptance is manual.
- Keep follow-up changes uncommitted after the requested checkpoint.

## Technical Defaults

- Use PostgreSQL's native constraint and statistics deparsers. Include domain
  constraints under the domain name. Follow the existing DDL inspector for
  policy source, including command, mode, roles and optional predicates.
- Statistics belong to the statistics object's namespace, which can differ
  from the underlying table's namespace. The native source identifies its table.

## Risks & Rollback

- Catalog deparsing can cost time on large schemas. Keep the candidate scope
  fence and inspect the before/after plans. The added branches can be reverted
  with the result-kind mapping and labels. No persisted state changes.

## Task 1 — Constraint, policy and statistics definitions

Files: `crates/db-wire/tests/catalog_test.rs`,
`crates/db-wire/src/postgres/definition_search.sql`,
`crates/db-wire/src/postgres/objects.rs`,
`crates/db-wire/src/objects.rs`, `crates/app/src/workspace/definition_search.rs`.

Interface: existing `search_definitions` returns `DefinitionHit` with added
`Constraint`, `Policy` and `Statistics` kinds and the native object OID.

- [x] Extend the live definition fixture to create same-named constraints in
  different schemas, a domain constraint, a policy with both expressions and
  expression statistics. Search for their shared literal. Assert the names,
  owner labels, source clauses and exact schema. First run must fail because
  the existing query finds none of these candidates.
- [x] Add namespace-filtered candidate branches for `pg_constraint`,
  `pg_policy` and `pg_statistic_ext`. Use `pg_get_constraintdef` and
  `pg_get_statisticsobjdef`. Project CREATE POLICY source, using `pg_get_expr` for
  each non-null predicate. Map the SQL kinds to the enum and GPUI titles.
- [x] Re-run `FASTDB_TEST_PG_PORT=55449 cargo test -p db-wire --test catalog_test --locked`.
  Ready to commit after verification: the five files above and guides —
  `feat(db-wire): expand PostgreSQL definition search`.

## Task 2 — Verification and documentation

- [x] Compare before/after `EXPLAIN (ANALYZE, BUFFERS)` on scoped fixtures.
- [x] Update the roadmap, provider and navigator guides, and not-built-yet.
- [x] Read the final diff, run `cargo fmt --all`,
  `cargo clippy --workspace --all-targets --locked`,
  `cargo test --workspace --locked`, and `cargo build --workspace --locked`.
  Smoke-test startup and obtain independent review. Record manual desktop
  acceptance and any unavailable engine checks.

## Live evidence

The PostgreSQL 16 regression returned zero of four new definition hits
before the extension. The subsequent policy regression returned zero hits
for a predicate-free DELETE policy before adding CREATE POLICY projection.
The final catalog suite passed 12 tests, including both directions of
cross-schema statistics scope. Independent re-review found no remaining
correctness or security issue in the PostgreSQL extension.

Formatting, workspace Clippy, workspace tests and workspace build passed.
The test run required localhost socket access for the existing SSH test.
FastDB stayed running through the startup smoke check. The user confirmed desktop acceptance. The live MySQL suite passed all seven tests.

The plan fixture had 1,600 tables, each with one constraint, policy,
statistics object and view. One quarter of the objects belonged to the
selected schema. These are local timings from single executions.

| Search | Before expansion | After expansion |
| --- | --- | --- |
| View marker, 51 rows | 10.500 ms, 5,967 hits and 93 reads | 19.417 ms, 15,538 hits |
| New family marker | 9.506 ms, 5,732 hits, 0 rows | 19.347 ms, 15,436 hits, 51 rows |
| Absent marker, 0 rows | 9.316 ms, 5,732 hits | 18.902 ms, 15,232 hits |

The larger catalog scope increased deparsing work. The policy OID index was
probed 400 times for the scoped policies and only for matching policy rows
during preview projection. The result sort used at most 28 KiB and did not
spill. The no-match planner estimate reached the 51-row limit while actual
matches were zero.
