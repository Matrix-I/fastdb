# Reproducible performance regression suite

Implement a versioned benchmark protocol covering process launch to first frame,
idle RSS, SQLite streaming of 1,000,000 rows x 20 columns with disk spill, peak RSS,
actual GPUI scrolling frame intervals, and release binary bytes. Keep workloads
fixed and use isolated config/spill directories without saved credentials or network.

1. Add an opt-in app benchmark entry point: launch the normal empty workspace,
   signal first-frame readiness, wait for idle sampling, stream the deterministic
   SQLite query on a worker, install its pager in a real table tab and scroll it
   through sequential and distant rows. Emit structured measurements and exit.
2. Add a native Rust xtask runner to launch independent repetitions, sample
   RSS, aggregate medians and scrolling p95, record host/build/protocol provenance,
   enforce explicit absolute budgets and optional same-host baseline regressions.
   Fail closed on missing metrics, invalid measurements, timeout or workload mismatch.
3. Add unit tests for threshold and report validation. Add CI release measurements
   for desktop macOS, retain JSON reports, and document stable-machine comparisons,
   baseline review, limitations and a manual display procedure.
4. Update README roadmap (including the already delivered CSV/SQL UI export).
   Run formatting, workspace clippy/tests, runner tests and the real app benchmark.

The user subsequently requested committing the completed feature; plans are kept
in a separate documentation commit.

## Validation outcome

Delivered the fixed workload, regression policy, local runner and manual desktop
CI workflow. Final guarded release run: one discarded warmup plus three measured
processes passed every absolute budget; see `benchmarks/VALIDATION.md` and the
local `benchmarks/results/guarded-baseline.json` report. The original prototype
passed 12 checker/lifecycle tests and 228 workspace tests, plus formatting and
clippy with warnings denied. The Rust migration has its own native verification. Earlier scrolling stalls led to independent frame-progress termination,
phase deadlines and direct benchmark-window close; failed trials are not accepted
as baselines. No further desktop loops are launched automatically after failure.
