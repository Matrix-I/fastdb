# Native Rust benchmark runner

Replace the Python orchestration with `cargo xtask benchmark`. Preserve protocol v1,
report/environment fields, raw samples, default workloads, absolute/relative
threshold checks and exit codes so existing JSON baselines remain compatible.

1. Add a lightweight workspace xtask binary and Cargo alias. Port CLI parsing,
   isolated macOS bundling, child lifecycle/awake guards, RSS sampling, phase
   deadlines, aggregation, hashing and report provenance to Rust.
2. Port policy validation and subprocess tests to native Rust fixtures. Add a
   report-only check command to validate existing reports without launching UI.
3. Replace Python commands in CI/docs and remove the old runner/tests. Build the
   runner before desktop measurements so Cargo compilation does not affect timing.
4. Run formatting, workspace clippy/tests, validate the existing real report and
   regression rejection. The app workload/UI stays unchanged; avoid unnecessary
   desktop launches after the earlier UI stall.

Commit the verified implementation as requested by the user; keep these plans in
a separate documentation commit.

## Verification

The native runner accepts the existing real protocol-v1 report via report-only
checking, including baseline comparison. Native fixtures validate process cleanup,
complete event ingestion, incomplete/error/debug rejection and timeout behavior.
CLI integration verifies exit codes 0 (pass), 1 (regression) and 2 (invalid report).
The measurement workload and app watchdog are unchanged; no new desktop stress
loop is needed for this orchestration migration.

Final checks passed: formatting, workspace clippy with warnings denied, all 245
workspace tests (including 17 native runner tests), and report-only validation of
the existing real baseline. The roadmap marks the benchmark suite complete.
