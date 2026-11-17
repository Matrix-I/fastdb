# P2-10 release performance acceptance plan

> **For agentic workers:** Use `executing-plans-strict`. Steps track evidence with `- [ ]`.

**Goal / Architecture / Tech Stack / Branch:** Measure the v0.2.0 candidate with the existing Rust desktop runner and a disposable PostgreSQL fixture on `feature/p2-10-release-performance`. Record evidence for the candidate checkout without committing, tagging or publishing a release.

## Confirmed requirements

- Cover OS cold-cache startup, live-provider latency, a large catalog, mixed-width and LOB results, and desktop memory and scrolling regressions.
- Publish reproducible evidence separately from aspirational targets. The v0.2.0 acceptance provider is PostgreSQL.

## Global constraints

- Preserve the existing release profile, benchmark protocol and performance thresholds. Use an isolated app profile and a disposable database. Do not record credentials or use Computer Use.
- Keep measurements independent of builds and identify the metric boundary, revision, platform and provider. Record blocked checks and keep P2-10 open while required evidence is missing.

## Technical defaults

- Use the protocol-v1 runner for warm startup, RSS, the million-row workload and scroll intervals. Compare `--baseline` on the same host if a matching report exists. Otherwise use absolute budgets and record relative regression as unverified.
- Run `cargo run --offline --locked --release -p db-wire --example postgres_performance` on loopback. The fixture has 20 schemas with 100 tables and five columns each. `ping`, `list_tables` and the five parallel metadata readers measure the driver boundary. A 20,000-row query returns variable widths and NULLs with 20,000-byte text/binary cells. After one warmup, retain five samples and validate shape and values. Run a separate capacity probe for 300,000-byte text/binary cells. Exact setup, execution and cleanup are documented in `benchmarks/postgres/README.md`. This does not measure tree rendering or the value viewer.
- Measure cold startup separately after the user-approved Linux system cache eviction procedure. Stage the app and profile and record the binary hash before eviction. Use a bounded launch-to-`ready` probe from the performance runbook. Protocol v1 discards its first run and can warm the executable during staging.

## Risks & rollback

- Run generated catalog fixtures in an independent disposable database. Remove the fixture and temporary measurement files after use, retaining reports and reusable Cargo outputs.
- Timing from another host or a software-rendered desktop is useful only with its actual environment recorded. A failed benchmark remains a failed check. The acceptance docs can be reverted without changing app behavior.

## Release evidence

- [x] Build with `cargo build --offline --locked --release -p fastdb-app -p xtask` and run `target/release/xtask benchmark` with the current display ID and report path. Validate the report with `target/release/xtask benchmark-check` and retain report/logs. Run `cargo test --offline --locked -p xtask` for the checker.
- [x] Measure connection and first batch, catalog enumeration and mixed-width/LOB read-back using FastDB's PostgreSQL driver. Keep `benchmarks/results/v0.2.0-postgres.json` and capture query plans for the non-trivial workload queries in `benchmarks/results/v0.2.0-postgres-plans.txt`. Confirm the report contains five valid samples per case.
- [x] Capture a separate first-frame measurement after user-confirmed Linux system cache eviction. The user approved cache flush instead of a reboot. The prepared release binary and clean profile produced 322.53 ms startup with exact viewport and release-mode verification.
- [x] Review the complete diff, run formatting and diff checks, publish measurement commands/results and update the roadmap status to match the evidence. Code changes, if needed, require the workspace build, Clippy, tests and independent review before acceptance.

## Historical checkpoint before Linux policy approval and cache flush

Release app, runner and provider example builds passed. Workspace Clippy passed with unused manifest dependency warnings. The full suite passed 822 tests across 42 suites after granting spill-directory and loopback access. PostgreSQL measurements and plans are published in `benchmarks/measurements/`. The desktop gate failed during scrolling and its viewport changed between processes. Cold startup, same-host comparison and tree rendering remain open. The user subsequently authorized opening a PR. Keep this checkpoint as a draft PR because the release acceptance gate remains open.

Plan review ran twice using the available inherited model because Opus is unavailable. It identified missing cold-launch procedure, baseline comparability and fixture details. The protocol now records those boundaries and a bounded Linux probe. Test-first code-change steps were inapplicable to this measurement-only example, which asserts fixture shape and actual read-back on its normal entry point. The plan was initially uncommitted. The user subsequently authorized the PR, including its required commits. Independent review approved the measurement tooling after correcting the cold-probe deadline and setup endpoint.

At the pre-flush checkpoint, a follow-up reproduces and fixes the scrolling capture deadline conflicting with the existing frame budget. X11 now produces complete protocol reports but fails scrolling budgets. Wayland can stall. User authorized cache flush; interactive host authentication is still required.

## Approved Linux policy

On 2026-11-17 the user approved Linux p95 ≤75 ms and slow frames ≤40%, with
all other absolute and relative limits preserved. Add an explicit Linux policy
file through the existing `--thresholds` option, capture a new baseline and
candidate, and verify policy-bound comparisons. The deadline regression now
checks both policies. Its old 25-second capture limit failed the approved Linux
360 ×75 ms case and is corrected to 35 seconds. This is separate from the
unchanged eight-second no-progress watchdog.

Final automated Linux X11 / PostgreSQL acceptance passed after the user confirmed the authorized cache flush. Baseline, candidate and cold-start raw evidence are retained in `benchmarks/measurements/`. Navigator tree and value-viewer GUI acceptance remain deferred under the user's original scope. Earlier failed trials and unsupported host claims remain explicitly documented.
