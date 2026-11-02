# Initial local validation — 2026-11-02

Validated the final guarded, uncommitted implementation with the release executable on an Apple
M1 Pro, macOS 27.2, rustc 1.99.1. The built-in display reported 1800 x 1169 at
120 Hz; GPUI's recorded viewport was 1280 x 801 logical pixels at scale 2. Frame
intervals include compositor scheduling and are not a claim of 120 FPS.

One warmup process was discarded, followed by three measured processes. The
medians below passed every absolute budget in `thresholds.json`:

| Metric | Local baseline |
|---|---:|
| Process startup to initial-frame event | 835.63 ms |
| Idle RSS | 99.03 MiB |
| Million-row x 20-column SQLite stream | 3,977.55 ms |
| First row-batch notification | 1.84 ms |
| Sampled peak RSS | 203.94 MiB |
| Scrolling p95 frame interval | 19.60 ms |
| Scrolling intervals over 50 ms | 0% |
| Release executable | 15.48 MiB |

Earlier trial runs stalled during scrolling and were rejected. The final harness
uses normal windows, direct close, phase deadlines and a watchdog independent of
GPUI that aborts after eight consecutive seconds without observed frame progress.
The watchdog converts a stall into an explicit failed run; the renderer's root
cause is not diagnosed by this suite. The complete guarded run above passed.

Full reports, raw frame intervals, host/build provenance and logs are retained
locally in the ignored `benchmarks/results/` directory. These are measurements of
one development machine and this workload, not portable product guarantees or
confirmation of the aspirational README targets.

```sh
cargo xtask benchmark --runs 3 --timeout 25 --display-id built-in-1800x1169-120hz-2x \
  --output benchmarks/results/guarded-baseline.json
cargo xtask benchmark --runs 3 --timeout 25 --display-id built-in-1800x1169-120hz-2x \
  --output benchmarks/results/guarded-candidate.json \
  --baseline benchmarks/results/guarded-baseline.json
```

The runner has been migrated to Rust (`cargo xtask`) without changing the app
workload or protocol-v1 report format. The existing real JSON report above passes
`cargo xtask benchmark-check`, including comparison against itself. Measurements
above are retained from the original successful desktop run, not a new UI launch.
All 17 native runner tests and 245 full-workspace tests passed. Native unit,
subprocess and CLI integration tests cover absolute/relative limits,
noise floors, incompatible baselines, invalid/missing metrics, debug rejection,
incomplete results, timeout cleanup and exit codes 0/1/2. Formatting and clippy with warnings denied also passed for the native
implementation.
Live external-provider and keychain tests retain their usual environment-variable
skip behavior.
