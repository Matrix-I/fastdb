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

## P2-10 release checkpoint — 2026-11-17

P2-10 remains open. PostgreSQL backend measurements passed. The Linux desktop
suite failed before completing its first measured process. Cold startup and
same-host regression comparison remain pending.

The candidate is revision `6a02aac` with the uncommitted measurement example.
The host is Ubuntu 26.04.1, x86_64, Intel Core i5-8365U, 14 GiB RAM. PostgreSQL
18.7 runs in a disposable local container with loopback TCP and trust
authentication. Connection timing includes ping and excludes authentication
work that would be required with SCRAM, SSH or TLS. One warmup preceded five
independent connections. Fixture and commands are in the
[PostgreSQL protocol](postgres/README.md).

| Production-driver measurement | Median | Maximum |
|---|---:|---:|
| Connection and ping | 2.78 ms | 2.89 ms |
| All connection metadata readers, 2,000 tables / 10,000 columns | 1,528.06 ms | 1,603.99 ms |
| First mixed-result batch notification | 2.76 ms | 2.97 ms |
| Complete 20,000-row mixed-width stream | 350.71 ms | 390.85 ms |
| Sampled early/final value read-back | 0.12 ms | 0.13 ms |

Text and binary 300,000-byte values passed exact read-back in a separate capacity
probe. Empty pages can hold an oversized row. This probe measures the driver and
pager path. Viewer performance and large-tree rendering remain unverified.
No optimization is claimed. The mixed query has a bounded function scan with
20,000 actual rows. The large-LOB query uses a constant Result node. Full plans
and five samples are retained in
[PostgreSQL evidence](measurements/2026-11-17-postgres.json) and
[query plans](measurements/2026-11-17-postgres-plans.txt).

The desktop command was `target/release/xtask benchmark --runs 3 --timeout 120
--display-id linux-edp1-3072x1728-59.94 --output
benchmarks/results/v0.2.0-desktop.json`. It ran after builds and the PostgreSQL
measurement ended. The warmup completed the million-row workload in 7,774.96 ms.
The first measured process completed it in 8,516.65 ms, then exceeded the
15-second scrolling deadline. Its viewport also changed from 1280 × 800 to
1469 × 832 at scale 1.25. These partial measurements cannot establish a passing
baseline. Thresholds and deadlines retain their existing values. See the
[failed desktop checkpoint](measurements/2026-11-17-desktop.json) and raw
[warmup](measurements/2026-11-17-desktop-run-0.log) and
[first measured process](measurements/2026-11-17-desktop-run-1.log).

Verification completed with the existing temporary Linux build headers.
`cargo build --offline --locked --release -p fastdb-app -p xtask` and the
PostgreSQL example release build passed. `cargo clippy --offline --locked
--workspace --all-targets` passed with existing unused manifest dependency
warnings. `cargo test --offline --locked --workspace` passed 822 tests across
42 suites with spill-directory and loopback access. External tests without
fixture variables skipped their live checks. The first sandboxed test run had
17 failures from the read-only `/var/tmp/fastdb` directory. The unrestricted
rerun passed. The checker separately passed its 17 tests.

Independent review reproduced an unbounded partial-line read in the documented
cold-start probe and a mismatch between the setup command's default socket and
the example's loopback endpoint. The probe now uses nonblocking bounded reads
and setup names the loopback host and fixture user. A partial-line fixture
exceeded the external one-second deadline before the fix, then returned the
expected failure within its shortened deadline afterward.
`python3 benchmarks/postgres/test_runbook.py` passes three subprocess cases for
partial output, a valid ready event and debug-binary rejection. Opus is
unavailable in this session, so independent review uses the inherited model.
Release acceptance remains incomplete for the reasons above.

## Deadline correction and Linux backend checks

The runner's previous 15-second scrolling deadline could reject 360 callbacks
that each meet the 50 ms budget, since those callbacks need 18 seconds. A new
regression test failed against the original deadline and passed after changing
collection to 25 seconds. Performance thresholds are unchanged. The Wayland
rerun reached its third measured process, then the independent eight-second
no-progress watchdog aborted it.

X11 completed all three measured processes. At its default scale 2 the medians
were 272.47 ms startup, 90.85 MiB idle RSS, 184.86 MiB peak RSS, 7,307.53 ms
stream, 50.18 ms scroll p95 and 21.33% intervals above 50 ms. At explicitly fixed
scale 1, startup was 228.23 ms, idle RSS 90.96 MiB, peak RSS 185.02 MiB, stream
8,793.91 ms, scroll p95 50.17 ms and slow intervals 33%. Both runs failed the
existing scrolling budgets. Full protocol reports are retained as
[default-scale X11](measurements/2026-11-17-desktop-x11.json) and
[scale-1 X11](measurements/2026-11-17-desktop-x11-scale1.json).

The explicit scale-1 command prefixes the desktop runner with
`env -u WAYLAND_DISPLAY GPUI_X11_SCALE_FACTOR=1` and uses display ID
`linux-x11-edp1-3072x1728-59.94-scale1`. This host is on AC power in the
performance power profile. Vulkan exposes an Intel UHD Graphics 620 with
Mesa 26.0.8 and a software fallback. No dedicated benchmark runner is registered
in the repository. System cold-cache measurement is authorized by the user,
but sudo still requires interactive host authentication.

The deadline fix passed its failing-then-passing regression and independent review. Final workspace verification passed 823 Rust tests, Clippy and the release build, plus three cold-probe subprocess tests.

## Approved Linux policy and repeatability

The user approved Linux scroll p95 ≤75 ms and at most 40% of intervals above
50 ms on 2026-11-17. Other absolute budgets and all relative allowances remain
unchanged. The original macOS policy is retained. New captures explicitly use
`--thresholds benchmarks/thresholds-linux.json` and X11 scale 1 at a stable
1280 ×800 viewport. Each capture discards one warmup and measures three processes.

| Metric | Baseline median | Candidate median |
|---|---:|---:|
| Startup | 124.17 ms | 137.29 ms |
| Idle RSS | 88.14 MiB | 89.52 MiB |
| Peak RSS | 182.22 MiB | 184.18 MiB |
| Million-row stream | 6,017.70 ms | 6,077.83 ms |
| First batch | 1.30 ms | 1.39 ms |
| Scroll p95 | 33.47 ms | 33.47 ms |
| Intervals >50 ms | 0% | 0% |
| Binary | 32.97 MiB | 32.97 MiB |

Both captures pass absolute limits. The candidate passes same-host relative
comparison and report-only validation. Using the original policy to check the
new report correctly fails with a policy-hash mismatch. These two captures use
the same executable and establish repeatability for the initial Linux baseline.
They establish no speedup between product revisions. Keep the benchmark window
in the foreground throughout capture. Earlier Wayland and X11 failures remain
recorded above and are not promoted into this baseline.

Raw reports are [Linux baseline](measurements/2026-11-17-linux-baseline.json)
and [Linux candidate](measurements/2026-11-17-linux-candidate.json).
