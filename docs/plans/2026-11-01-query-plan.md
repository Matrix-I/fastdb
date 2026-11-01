# PostgreSQL query plan viewer

Implement the roadmap's visual query plan viewer for PostgreSQL first.

1. Add a provider-owned EXPLAIN query builder to db-wire. Use JSON output, with
   explicit estimated and ANALYZE modes; do not wrap or page the original SQL.
2. Add a parsed plan model and a scrollable, collapsible tree in the SQL script
   result area. Show operator/relation/index, costs, estimated/actual rows,
   timings, loops, conditions and buffer usage. Parse once after completion.
3. Add Explain and Analyze toolbar actions for PostgreSQL. Label Analyze as
   executing the statement; reuse the existing operation lifecycle/cancellation.
   Ordinary execution clears the plan. Preserve raw JSON in the result grid.
4. Test query generation, nested plans, optional metrics, malformed output and
   tab lifecycle. Update README and provider documentation with support limits.
5. Run formatting, workspace clippy/tests, and launch the application.

Other providers remain unsupported by the visual viewer.

## Validation

- `cargo fmt --all`, `cargo clippy --workspace --all-targets`, and
  `cargo test --workspace` passed. Unconfigured live provider tests skip normally.
- The new PostgreSQL estimated/analyzed JSON plan integration test passed against
  the local PostgreSQL container with authentication configured.
- The updated app launched with an isolated temporary profile. Interactive visual
  verification was unavailable because macOS computer-use permissions are not
  granted. The temporary profile was removed after the smoke test.
