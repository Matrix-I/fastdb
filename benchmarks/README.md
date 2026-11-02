# Performance regression protocol v1

Requires only the repository Rust toolchain and the OS desktop tools (`ps`, plus
`caffeinate` on macOS). No Python runtime is needed.

This suite measures the release app on a real macOS/Linux desktop, not a synthetic
FPS label or a unit-test time. Run on an unlocked, visible desktop with FastDB in
the foreground, fixed display scale/refresh rate, AC power and no concurrent builds
or heavy applications. A display-less runner is not suitable. Linux needs the normal
GPUI/Vulkan dependencies and an active display session. Baselines are host-specific.

## Run and compare

From the repo root, build first and let the host cool/settle before measuring:

```sh
cargo build --locked -p xtask
cargo build --locked --release -p fastdb-app
cargo xtask benchmark --output benchmarks/results/baseline.json --display-id built-in-60hz-2x
# After implementing a feature, build the new revision with the same command:
cargo xtask benchmark --output benchmarks/results/candidate.json --display-id built-in-60hz-2x \
  --baseline benchmarks/results/baseline.json
```

The runner discards one warmup launch, then measures five independent processes.
It prints medians and writes raw per-run metrics and 300 frame intervals per run.
Exit codes: 0 pass, 1 regression/budget failure, 2 invalid/incomplete run. A report
is still written for threshold failures. Logs are saved alongside the report.
Use `--runs 3` for a shorter check (baseline must use the same run count).
`--timeout 180` caps each subprocess; tighter phase deadlines also apply (10 s
startup, 75 s query, 15 s scrolling). The app independently aborts scrolling after
8 s without progress. No metric can pass by being absent.
Do not run benchmarks in parallel. Build time is excluded. The alias builds the
runner before launching the measured app. For a dedicated host, compile everything
first and invoke `target/debug/xtask benchmark ...` directly after settling.

Existing protocol-v1 JSON reports from the previous runner remain compatible.
Check an existing report without launching the desktop app:

```sh
cargo xtask benchmark-check --report benchmarks/results/guarded-baseline.json
# Enforce relative limits too:
cargo xtask benchmark-check --report benchmarks/results/candidate.json \
  --baseline benchmarks/results/baseline.json
```

Report-only checking also verifies the current threshold policy hash. Exit codes
are the same as measurement mode. A changed runner does not silently refresh a
baseline or rewrite prior measurements.

The app's opt-in `--benchmark` flag requires an empty `FASTDB_CONFIG_DIR`; the
runner creates a fresh temporary working directory/profile each time. It avoids
saved connections, credential access, legacy config migration, update network
checks and cleanup of another app's spill files. On macOS it stages a minimal
`.app` containing a copy of the release executable and an isolated bundle identity
before starting the clock. AppKit needs that identity for reliable foreground
activation and display callbacks. The benchmark uses the normal workspace/window kind.
Keep it visible during scrolling: an independent watchdog terminates the process
if scrolling stops progressing for 8 seconds, including a stalled GPUI event loop.
Closing the benchmark window exits directly without the normal save barrier.
A temporary `caffeinate` assertion keeps the display and machine awake while each
subprocess runs; it ends with that process and does not change saved power settings
or unlock a locked session. Staging/copy time is excluded; executable bytes are
unchanged. The temporary SQLite database
is in memory and generates rows on demand; results use the production driver,
pager, default 1,024-page cache, LZ4 spill and actual workspace/grid. macOS spill
files use the temporary directory; Linux keeps production `/var/tmp/fastdb` disk
spill behavior. Normal spill-file destruction cleans up the run's files.

## Workloads and measurement boundaries

| Metric | Fixed measurement |
|---|---|
| `startup_ms` | Parent's process launch to receiving the ready event after the initial GPUI frame; includes process loading and IPC. Empty workspace at 1280 x 800 logical pixels. |
| `idle_rss_mib` | Median resident set after a 500 ms settling period during the 2 s empty-workspace idle window. |
| `query_ms` | SQLite stream to completion: recursive sequence, 1,000,000 rows x 20 columns; each non-NULL cell is a row-varying 36-byte string, NULL every 17 cells. Roughly 680 MB cell payload plus row/page overhead forces disk spill. No source table setup or full result collection. |
| `first_batch_ms` | Query dispatch to the first row-batch notification, **not** first-row latency; includes driver scheduling and notification throttling. |
| `peak_rss_mib` | Maximum process RSS sampled throughout launch, idle, query and scrolling. Includes GPUI/driver/runtime overhead, excludes OS filesystem cache and other processes. |
| `scroll_p95_ms` | Nearest-rank p95 of 300 GPUI callback intervals after 60 warmup frames. First 150 measured frames scroll 8 rows/frame; next 150 seek `(frame * 7919) % 999900`; horizontal offset cycles over 20 positions. |
| `scroll_over_50ms_pct` | Percentage of measured intervals exceeding 50 ms; catches stutters hidden by median FPS. |
| `binary_mib` | Raw release executable bytes / 1,048,576. Measures code growth; does not claim DMG/ZIP/AppImage size. |

RSS is sampled using `ps` at approximately 50 ms intervals; brief peaks can be
missed. It measures resident process memory, not GPU-only device allocations. Scrolling measures CPU/event-loop/display scheduling and spill reads;
it is not a GPU presentation fence or hardware input-to-photon measurement.
Frame intervals depend on refresh rate and compositor. Process launches use warm
filesystem/shader caches after the discarded run; they are **not** OS cold-cache
startup. The suite validates result shape and NULL/text values on old/spilled,
middle and final pages before accepting query measurements. It does not measure
remote provider latency or all interaction paths. Query timing uses the production
streaming driver with the empty workspace visible; it excludes per-batch workspace
query orchestration and incremental grid repainting during ingestion.

For true cold-cache startup, reboot, wait for background work to settle, run one
launch under an OS profiler and keep that result separate. For packaged size,
build with `./bundle_macos.sh --no-install` or the matching `ci/build-*.sh` and
record each artifact's bytes separately: compression, signing and architecture
change these sizes. Do not compare a package with the raw executable budget.

## Budgets and regression policy

`thresholds.json` is version controlled. Initial budgets are safety limits to
catch major regressions, **not measured baselines or the README's aspirational
targets**. Tighten them after collecting stable results on your designated host.

| Metric | Absolute maximum | Allowed increase vs baseline | Noise allowance |
|---|---:|---:|---:|
| Startup | 2,000 ms | 20% | 25 ms |
| Idle RSS | 256 MiB | 15% | 8 MiB |
| Million-row query | 60,000 ms | 15% | 250 ms |
| First batch | 1,000 ms | 20% | 10 ms |
| Peak RSS | 384 MiB | 15% | 16 MiB |
| Scroll p95 | 50 ms | 20% | 2 ms |
| Intervals >50 ms | 5% | 20% | 1 percentage point |
| Executable | 100 MiB | 5% | 1 MiB |

A run fails if any aggregate median exceeds the absolute maximum **or**
`baseline + max(baseline * allowed_increase, noise_allowance)`.
All metrics are lower-is-better. Each run's scrolling p95 is computed first,
then the median across runs is checked; raw intervals remain available for review.
Host ID, OS release, architecture, CPU, display ID, Rust version, release profile,
actual viewport/scale factor, workload/run count and threshold policy hash must match before comparison. Debug
builds, invalid/nonfinite values and incomplete workload events are rejected.
The runner records git revision, dirty state and binary SHA-256 for provenance;
build the binary immediately before running because the runner cannot establish
that a supplied executable matches the current source tree.

When a check fails, inspect individual runs and rerun both baseline and candidate
under identical conditions. Investigate a repeatable regression before adjusting
thresholds. Keep baseline reports outside disposable build output. Baseline
updates are explicit reviewed changes: attach old/new reports, explain the cost
of the feature and update budgets separately if necessary. Never silently promote
a failed candidate. A new workload requires a protocol version bump and new
baselines; changing a display or compiler requires measuring both revisions again.

See [initial local validation](VALIDATION.md) for the first measured results.

## CI and verification

Normal macOS CI tests the checker without performance assumptions. The manual
`Desktop performance` workflow uses a dedicated self-hosted macOS runner labeled
`fastdb-benchmark`, with its GUI login session active. It builds and measures an
optional baseline ref followed by the selected candidate, enforces the policy,
and uploads reports/logs even on failure. Both refs must implement protocol v1;
leave baseline empty for the first post-merge run. No dedicated host is provisioned
by this repository and the workflow does not run arbitrary PRs automatically.
Linux users run the same CLI on a desktop host and maintain a separate baseline.

```sh
cargo test -p xtask
```

Manual visual check: keep the benchmark window foreground, confirm the empty
workspace appears, then the million-row grid, vertical movement and horizontal
movement. Keep display/power settings unchanged between revisions. A minimized
or occluded window invalidates the performance comparison even if it exits cleanly.
