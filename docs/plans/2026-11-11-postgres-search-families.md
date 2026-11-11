# PostgreSQL definition search families implementation plan

> **For agentic workers:** Use executing-plans-strict. Steps use `- [ ]`.

**Goal / Architecture / Tech Stack / Branch:** Expand P2-04 on the current
working tree. Extend the scoped native PostgreSQL query, its Rust result
mapping and GPUI titles. The working tree is on main.

## Confirmed Requirements

- P2-04 covers PostgreSQL, MySQL and SQLite. The user approved additional
  definition result kinds. Search is read-only, scoped, bounded and cancellable.

## Global Constraints

- Keep terms and scope bound. Apply schema filtering before deparsing.
- Preserve the existing result and source bounds, privileges and cancellation.
- Run format, Clippy, workspace tests and build. Verify with a disposable
  PostgreSQL server and inspect query plans. Desktop acceptance is manual.
- Keep the plan and implementation uncommitted.

## Technical Defaults

- Use PostgreSQL's native constraint and statistics deparsers. Include domain
  constraints under the domain name. For policies, search the USING and WITH
  CHECK expressions with their clause labels, following the catalog inspector.
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

- [ ] Extend the live definition fixture to create same-named constraints in
  different schemas, a domain constraint, a policy with both expressions and
  expression statistics. Search for their shared literal. Assert the names,
  owner labels, source clauses and exact schema. First run must fail because
  the existing query finds none of these candidates.
- [ ] Add namespace-filtered candidate branches for `pg_constraint`,
  `pg_policy` and `pg_statistic_ext`. Use `pg_get_constraintdef` and
  `pg_get_statisticsobjdef`. Build policy previews from `pg_get_expr` for
  each non-null expression. Map the SQL kinds to the enum and GPUI titles.
- [ ] Re-run `FASTDB_TEST_PG_PORT=55449 cargo test -p db-wire --test catalog_test --locked`.
  Ready to commit after verification: the five files above and guides —
  `feat(db-wire): expand PostgreSQL definition search`.

## Task 2 — Verification and documentation

- [ ] Compare before/after `EXPLAIN (ANALYZE, BUFFERS)` on scoped fixtures.
- [ ] Update the roadmap, provider and navigator guides, and not-built-yet.
- [ ] Read the final diff, run `cargo fmt --all`,
  `cargo clippy --workspace --all-targets --locked`,
  `cargo test --workspace --locked`, and `cargo build --workspace --locked`.
  Smoke-test startup and obtain independent review. Record manual desktop
  acceptance and any unavailable engine checks.
