# FastDB full feature acceptance results

Original candidate: FastDB 0.3.0 at `359502a`. Linux x86_64, functional GUI on a private Xvfb display with software Vulkan. PostgreSQL 16.16, MySQL 8.4.13, MariaDB 11.4.13 and disposable SQLite files. See the [test plan](../plans/2026-11-22-full-feature-acceptance.md).

The continuation covers Linux as requested. The macOS matrix is deferred for this pass.

The grid fix landed at `4197887`. The branch's product sources and resources
match that commit through `9d224ed`. The retained x86_64 debug executable
matches the `227a16d` X11 overlap build
(`f962104e556768d4ed2866321cef0e0790dd12af1b31baedb6790d2f1ea3f2a2`).
The retained local release executable matches the packaging build
(`66e157d0b23bde6a267c787fe602e52f236f6aa35e0fbc4a691268934efca821`).
Public signed releases and the Ubuntu 24.04 package have separate provenance
below.

### Task 1 Linux fixture read-back

On `9d224ed`, fresh PostgreSQL 16.16 databases loaded the checked-in SQL
fixture. MySQL 8.4.13 schemas and SQLite 3.46.1 files used corresponding
portable tables with primary keys, a composite foreign key and an amount
check. Independent native reads returned `1000, 3, 1, 2, 0, 0, 0` for
each of the six fixtures. A and B used distinct database-specific strings
at ID 4. Sample reads confirmed NULL versus empty text and binary, Unicode,
embedded LF and the exact amount `1234567890123.45`. SQLite used
`123456789012345` integer minor units and passed `foreign_key_check`.
A restricted PostgreSQL role read all 1,000 rows and received a native
permission denial on UPDATE. A separate unrestricted read confirmed
the original value. The initial PostgreSQL check had an incorrect literal
length expectation. Correcting the assertion to 27 matched the fixture.
The databases and files were disposable. GUI profile checks have separate evidence.

| Area | Observed result | Status |
| --- | --- | --- |
| Workspace and enabled drivers | `FASTDB_REQUIRE_DB_TESTS=1 cargo test --workspace --locked --offline -- --nocapture` passed 834 harness cases across 42 suites. The real PostgreSQL, MySQL and file-backed SQLite workflow cases ran. Optional vendor ODBC and certificate fixtures skipped in this baseline. | Pass |
| Focused database integrations | `cargo test --locked --offline -p db-wire --test query_plan_test --test administration_test --test session_monitor_test --test catalog_test --test mysql_test -- --nocapture` passed 32 cases with required live fixtures. Both MariaDB cases ran. | Pass |
| TLS and SSH-over-TLS | `bash ci/test-tls.sh` with Podman as the container runner passed six TLS cases and two SSH-over-TLS cases against new loopback fixtures. | Pass |
| Linux credential store | `FASTDB_TEST_KEYCHAIN=1` enabled the native round trip. The test passed on a private D-Bus session with its own keyring data and runtime directories. | Pass |
| Linux build checks | Formatting, Clippy with `-D warnings`, workspace typecheck, and the all-targets development build passed. Existing unused dependency manifest warnings were emitted. | Pass |
| Linux X11 GUI setup | Automated pointer connected to PostgreSQL. SQL script bytes matched `select 42;` after close, and the reopened script produced a result and opened the native GTK export chooser. The export check below confirmed dismissal by Cancel. Window-manager close remains unconfirmed. | Partial |
| L03 PostgreSQL | Staged change on A survived keyboard Close and Quit. Independent reads returned the original value on A and B. Clicking Cancel restored the grid. A separate Save changed A to `qa-saved`, while B still returned `original`. | Pass for these subcases |
| L04 PostgreSQL | Immediate keyboard close and Quit persisted exact script bytes. Relaunch restored them. With write permission removed from only the isolated script directory, Quit produced a visible permission error, kept the draft and left disk bytes unchanged. Restoring permissions and pressing Quit saved the exact draft and exited. | Pass for these subcases |
| PostgreSQL, MySQL and SQLite grids | On the private X11 desktop, opening the seeded `Tables → items` grid exited with the GPUI panic `text argument should not contain newlines` at `text_system.rs:372` on all three providers. The PostgreSQL backtrace reaches [cell-width measurement](../../crates/ui-grid/src/grid_element/widths.rs) at its GPUI `shape_line` call. A build from the test worktree confirmed the shared artifact hash. | Fail |
| SQLite newline control | A copy of the SQLite fixture differed only in `items.note` for ID 3, whose LF was replaced by a space. The copied grid opened with row 3 visible. The original fixture retained its LF. | Pass for control, crash isolated to multiline input |
| SQLite filter control | The smaller `editable` grid opened. Filtering on `id = 2` displayed exactly `2, second`, confirmed by an independent read-only SQLite query. | Pass |
| PostgreSQL script and plans | A sequential script produced distinct results `42` and `43`, stopped on division by zero, and never ran its fourth statement. Closing the second result left the other results in place. Explain and Analyze showed distinct estimated and observed results for the same `editable_pkey` scan. Closing the script persisted its exact text. | Pass for these subcases |
| PostgreSQL loaded export | Canceling the GTK file chooser left both isolated output directories empty. CSV, JSON, HTML and Markdown each parsed to the same two expected rows. Replaying the exported SQL into a temporary PostgreSQL table produced exactly the same values. | Pass for these subcases |
| SQLite native restore | CLI 3.46.1 rejected `-ifexists` when restoring into an existing empty file. Scoped backup passed on 3.46.1. Both SQLite native cases passed with the verified upstream 3.54.1 CLI. | Compatibility failure on 3.46.1 |
| PostgreSQL native tools | Scope, restore, refusal and maintenance passed against PostgreSQL 16.16 with matching 16.16 CLI executables. A separate run with 18.6 executables failed against the 16.16 server because restore SQL included `transaction_timeout`. | Pass with matching clients |
| MySQL native tools | Native backup, scoped restore, broader-grant refusal, concurrent restore and maintenance passed with client and server 8.4.13. Initial fixture grants had wildcard semantics and failed before the expected refusal path. The successful run used exact schema grants and a separate account with access to both source and destination for the broader-grant refusal. | Pass |
| Updater | The candidate app test binary passed 19 updater and two update-controller cases. Native installation of a signed update requires a separate disposable installation and a supported signed target. | Pass at automated test level |
| Debian package structure | The development binary passed the package script's launcher, metadata, payload and validation assertions. The later [candidate release installation](#ubuntu-2404-desktop-launcher-and-removal) has separate evidence. | Pass for development artifact |
| PostgreSQL load fixture | The development example produced complete JSON for five workload samples and a passing 300,000-byte LOB check against PostgreSQL 18. The shell wrapper exited with signal status 143 after the report was written. The report is functional evidence. Release thresholds and cold startup require separate runs. | Partial |
| PostgreSQL Administration and Session Monitor | Five live integration tests passed on 16.16. In the original X11 GUI, Restart Sequence reported commit and a separate `psql` read returned `42\|f`. Session Monitor showed the blocking chain, reported cancellation/termination against PID 224 and refreshed the list. The independent waiter returned SQLSTATE 57014 and the downstream client returned 00000. Restricted-profile activity fields showed Details unavailable. | Pass for these subcases |

## Multiline grid regression retest (2026-11-23)

Original binary SHA-256 `cacc70b580e7e4c40ff6ddc95a8a7256ed56418f2d4c7230fc26031a527e2e44`.
The debug build from `359502a` with the grid fix has SHA-256
`392a93bc080bd8e77c1d7e7fa8698fe8e07ad957588315576f5061108565aba3`.

| Provider | Original read of the multiline grid | Fixed build |
| --- | --- | --- |
| PostgreSQL 16.16 | Exit 101, GPUI newline panic | Opened both rows, copied LF and CRLF exactly, and quit cleanly |
| MySQL 8.4.13 | Exit 101, GPUI newline panic | Opened both rows, copied LF and CRLF exactly, and quit cleanly |
| SQLite file | Exit 101, GPUI newline panic | Opened both rows, copied LF and CRLF exactly, and quit cleanly |

The isolated X11 test compared clipboard bytes to fixture values
`6265666f72650a7365636f6e64206c696e65` and `616c7068610d0a6f6d656761`.
The old width-sampling code included line breaks in GPUI's single-line shaping
call. The fix bounds the cell sample and measures a single line, while full
values remain available through the grid and clipboard.

After the fix, `cargo test --workspace --locked --offline` passed 836 harness
cases across 42 suites. The two new grid tests passed, and that workspace run
skipped opt-in live suites without fixture settings. Formatting, Clippy with
`-D warnings`, workspace typecheck and the all-targets debug build passed.
A second workspace run required live PostgreSQL 16.16 and MySQL 8.4.13.
It passed 836 harness cases across 42 suites, including all three workflow
tests. MariaDB, vendor ODBC and opt-in TLS used separate fixtures or were skipped.
An independent Codex review approved the grid patch. It verified the bounded
width inputs and full data paths. Multiline column aliases had unit coverage
and source review.

### Additional PostgreSQL X11 cases on the fixed build

| Case | Observed result | Status |
| --- | --- | --- |
| B01 / V01 · Browse and clipboard | The seeded 1,000-row `release_check.items` table opened. Filtering for ID 3 and copying its note returned the exact multiline fixture value. The second line of some multiline cells was clipped at the row boundary. | Pass for open, filter and copy. Visual clipping remains open. |
| M01 / M04 · Properties and DDL | The small `public.items` fixture displayed its integer primary key and non-null text column. Native DDL showed the corresponding columns and primary key. | Pass for these subcases |
| M05 · Relationships | The diagram for `release_check.items` rendered three qualified tables with four FK column links, including both composite-key columns, the self-reference and the incoming reference. Fit resized the view. Double-clicking `release_check.parents` opened the three expected rows. A private GTK portal exported a parseable SVG with the same three tables and four named foreign-key edges. | Pass for these subcases |
| W03 · Manual table changes | A saved change was visible in the tab while independent reads saw the old value. Commit changed the independent read. Rollback restored the committed value, and closing a tab with a later uncommitted edit preserved that value. The saved workspace confirmed the tab had closed. | Pass for these subcases |
| X03 · Full-source CSV | GTK export produced 1,000 unique IDs. A second export with `id <= 25` produced exactly those 25 IDs. Independent parsing confirmed Unicode, multiline text, and distinct NULL and empty text. | Pass for full-source CSV |
| X01 · CSV import | Closing the preview left the destination empty. UTF-8 comma import committed three rows and `psql` confirmed the exact Vietnamese, quotes and LF bytes. Semicolon import distinguished the default unquoted NULL token from quoted empty text. | Pass |
| X02 · Atomic duplicate rollback | The wizard reported a duplicate primary key and zero committed rows. An independent session confirmed that the preceding valid ID 4 was absent and that the original ID 1 remained intact. | Pass for atomic duplicate |
| X04 · Table transfer | The navigator refused a self-transfer. Same-provider Atomic Commit transferred all five rows. Separate reads returned five source rows, five target rows and zero differences, including NULL, empty text and LF. | Pass for these subcases |
| S07 / P2-05 · Plan snapshots | Saved an estimated primary-key lookup as valid PostgreSQL v1 JSON. Analyze reported one actual row and loop. Compare showed estimated and actual values separately. Open appended a result without executing SQL. Malformed and unsupported-version JSON produced a visible refusal and preserved all three results. | Pass for snapshot subcases |
| T02 · SQL-tab transaction | In Manual mode the tab saw the pending update while independent `psql` saw the original value. Rollback kept the original. A separate Commit changed the independent read, and a second commit restored the fixture. | Pass for Manual mode |
| T03 · Savepoint recovery | Division by zero stopped the script. The same tab recovered with Rollback to Savepoint, returned exactly one row from `SELECT 1`, and showed an active transaction. | Pass for these subcases |
| W04 / W05 · Log and result view | The transaction log showed UTC, operation type, duration, rows and result. Filters found the successful rollback and failed division. The script initially had only the editor, then displayed a result panel after execution. | Partial |

### Native Wayland smoke on the fixed build

On a private Sway 1.11 headless compositor at 1280×800 with Xwayland disabled,
the fixed build rendered through native Wayland. A virtual pointer connected to
the SQLite file and opened both multiline rows. Virtual keyboard input saved
the exact `select 42;` bytes with `Ctrl+W`. `Ctrl+Q` closed the process.
Relaunch restored the script and tab names. After reconnect and Refresh,
the grid copied both multiline values byte for byte via the Wayland clipboard.

## Linux packaging and updater

- Release binary `66e157d0b23bde6a267c787fe602e52f236f6aa35e0fbc4a691268934efca821`
  passed the Debian packaging script. A disposable Ubuntu 26.04.1 container
  passed apt install, launcher and executable verification, and apt remove.
- Minimal Ubuntu GUI launch failed at GPUI font fallback after apt install.
  The package dependency list lacks Fontconfig, which GPUI loads dynamically.
  Installing `fontconfig` enabled packaged GUI startup and exact script
  persistence across standalone restart. The host package requires
  `libc6 >= 2.43`. The separate Ubuntu 24.04 build and install are recorded below.
- Updater tests passed 21 cases. The public v0.3.0 Minisign manifest and Debian
  payload verified against the production public key. A live upgrade remains
  blocked because the newest signed public release is the same version.
- The original Linux build produced Debian and standalone artifacts.
  The separate [AppImage retest](#linux-appimage-packaging-and-startup)
  exercised the conditional AppImage path.

### Linux AppImage packaging and startup

The AppImage portion of `ci/build-linux.sh` packaged the retained
Ubuntu 24.04 release executable (SHA-256
`00af96e67fa90681bda0e4cd2ab38fb7493c87f5f2cc77a095d2b5a63c6f38fb`).
A disposable Ubuntu 24.04.5 container used the verified x86_64
appimagetool 1.9.1 asset and supplied the verified x86_64 type-2 runtime
with `--runtime-file`. The resulting AppImage SHA-256 was
`da9c6131fc97428dc405753b10ff35a8f5c45130819ada07f0379d4491bda344`.
Extraction returned an executable `AppRun`, the icon and a desktop entry
accepted by `desktop-file-validate`. The embedded FastDB executable
matched the original byte for byte. [AppImage tools](https://github.com/AppImage/appimagetool/releases/tag/1.9.1),
[type-2 runtime](https://github.com/AppImage/type2-runtime/releases/tag/continuous)

With `APPIMAGE_EXTRACT_AND_RUN=1`, the AppImage opened on private
Openbox/Xvfb. Its Navigator connected to a disposable SQLite file.
The table grid and a SQL editor result both showed ID 76031 and
`appimage-fixture`, matching an independent read-only SQLite query.
Ctrl+Q closed the window and saved the SQL text byte for byte. Relaunch
restored the tab and text without rerunning the query.

The extract-and-run smoke passed. Direct launch exited 127 because the
environment lacked both `/dev/fuse` and a suitable `fusermount`. The mounted
launch remains unverified and requires a FUSE-enabled environment.
Evidence is in `.sdd/appimage-evidence/`.

### Ubuntu 24.04 desktop launcher and removal

The retained amd64 0.3.0 DEB installed in disposable Ubuntu 24.04.5.
`desktop-file-validate` accepted its desktop entry, and the scalable
`fastdb.svg` application icon was installed. With Fontconfig explicitly
installed, `gtk-launch fastdb` resolved that entry and started
`/usr/bin/fastdb`. Private Xvfb showed an empty workspace at 1280 × 800
with no startup banner. Ctrl+Q closed the window. Apt removal removed
the binary, launcher and icon and preserved a seeded profile file
byte-for-byte.

The packaged Linux workspace exposed no About or manual Check for Updates
entry. F10 and F1 left the workspace unchanged. The package manager and
workspace version both read 0.3.0. The plan's About version check still
fails at the missing entry. This launcher run used a fresh app profile
on a warm host. The earlier minimal-install Fontconfig failure stands.

### Minimal Ubuntu 24.04 package startup

The retained 0.3.0 amd64 DEB (SHA-256
`23e7a07a81a58dd45f638ec0bbe57dfc5bb384ffbe69cfec6a0829a207823a77`)
installed on fresh Ubuntu 24.04.5 with `--no-install-recommends`.
FastDB and the Vulkan software renderer ran in one disposable container.
A separately provisioned Xvfb container provided the private X11 socket
at 1280 × 800 so its packages could not satisfy application dependencies.

Neither `libfontconfig1` nor the checked DejaVu and Noto font packages was
installed in the application container, and the loader cache contained
no Fontconfig library. FastDB aborted with exit 134 when GPUI could not
resolve `.SystemUIFont` or any fallback. Installing `libfontconfig1`
with its required configuration and fonts in the same container let
the same DEB open a viewable FastDB window with a fresh profile. It
stayed running until the 12-second harness timeout. This reproduces
the minimal-install failure on Ubuntu 24.04 and confirms that the
library, configuration and font packages suffice without the Fontconfig
command-line utilities. Evidence is in `.sdd/noble-font-evidence/`.

### Published Debian package upgrade from RC to final

On `9d224ed`, a fresh Ubuntu 24.04 amd64 Podman container installed the
public RC package, then apt explicitly unpacked the final package over it.
Both [RC](https://github.com/Matrix-I/fastdb/releases/tag/v0.3.0-rc.1) and
[final](https://github.com/Matrix-I/fastdb/releases/tag/v0.3.0) manifests
verified with the repository production Minisign public key. Each package
matched its exact entry in the signed manifest.

| Package version | DEB SHA-256 | Installed executable SHA-256 |
| --- | --- | --- |
| `0.3.0~rc.1` | `704612477170f5fe9ab30353a6e67d43e6f4755889a5647bcee2a9946cd119d6` | `607dd12c02707fc66e5233438f30b84088668eefd500310a4418d1b9b21b9d05` |
| `0.3.0` | `430348fa8eb9a117b8275d0055382954c5e85dcf5dbe3802f60198e20999759c` | `ecc427e0add62c379206ec5adde8d81a6243c65b1ad10873609d53353650b017` |

The upgraded version, desktop launcher and linked libraries passed checks.
A separately seeded SQL file retained its SHA-256 after upgrade and removal.
Removal deleted the executable and launcher. The first test attempt expected
the upstream version string with a hyphen. The signed package correctly
uses Debian's tilde for prerelease ordering, and the corrected check passed.

On rewritten head `c27ca70`, a repeat on clean Ubuntu 24.04 also ran both
installed executables with their built-in startup probe, before and after
the apt upgrade. Both emitted the protocol-1 first-frame event for a
1280 × 800 viewport at scale 1. A bare Xvfb run reached renderer initialization
and did not emit the event in 20 seconds. The successful retry used
Openbox, Fontconfig and software Vulkan on private Xvfb.

Both tags predate the P3-05 verification commit `c8768a4`.

### Interactive restart after the signed Debian package upgrade

On Debian 13 in private Openbox/Xvfb, both released DEBs again matched
production-signed manifests. The RC created Script-1 through the GUI,
ran it against a disposable SQLite profile and displayed ID 731 and
`signed-upgrade-fixture`. Ctrl+Q saved its exact SQL bytes. Apt unpacked
the final DEB over the RC and reported version 0.3.0. The final build
reopened the same profile and script without running it. Execute opened
a connection dialog because the profile was disconnected. After
explicit reconnect and Execute, the same row appeared. Independent
SQLite and SHA-256 checks proved the database and script bytes survived
the upgrade and package removal. Screenshots and hashes are in
`.sdd/interactive-signed-deb-upgrade-evidence/`.

GitHub's latest-release API still returned v0.3.0 on 2026-11-28, with
signed checksums and the Linux DEB. The tested v0.3.0 candidate requires
a newer compatible production-signed target to exercise the in-app
download, verification and installer handoff. UPD-04 remains Blocked.

## Linux release performance

`cargo test --locked -p xtask` passed all 18 checks on the current
worktree, including environment mismatch, policy budgets, incomplete
workloads and subprocess cleanup.

The release example ran on disposable PostgreSQL 18.7 and completed five
measured samples. Median catalog loading for 2,000 tables was 2,213 ms,
first batch 3.57 ms, and the 20,000-row stream 599 ms. The 300,000-byte text
and binary capacity probe passed exact read-back. The mixed query plan
showed a function scan over exactly 20,000 rows. The large LOB plan returned
one row. On Xvfb the release GUI loaded that catalog, filtered Navigator to
`table_100`, and displayed matches under multiple schemas. The grid rendered
a 300,000-byte text result and the Value panel reported the 256 KiB editing
limit. Retained evidence is under `.sdd/postgres-release-evidence/`.

The first isolated Xvfb/Lavapipe benchmark timed out during first-frame warmup.
The focused retry completed warmup, including the 1,000,000 × 20 workload
in 6.97 seconds and all 300 scroll intervals. The first measured run timed
out during launch, so no measured result or policy verdict was produced.
Desktop policy comparison requires a dedicated display in the approved
configuration. Cold-start acceptance requires a first launch on that host
after a coordinated reboot.

The current `xtask benchmark-check` accepted the archived
[Linux baseline](../../benchmarks/measurements/2026-11-17-linux-baseline.json)
and [candidate](../../benchmarks/measurements/2026-11-17-linux-candidate.json)
under `thresholds-linux.json`. They describe revision `def097c` and require
the recorded `linux-x11-edp1-3072x1728-59.94-scale1` display configuration.
The current session is Wayland, its X11 display is inaccessible from the
test shell, and its kernel is `7.0.0-44-generic` instead of `7.0.0-43-generic`.
The comparison verifies that the historical reports are usable. The current
release binary requires a new comparable capture and cold-start observation.
On 2026-11-26 the same release build completed with
`cargo build --locked --release -p fastdb-app` using the retained target
directory. A new isolated Xvfb 1440 × 900 run under Lavapipe used a
separate diagnostic envelope. The first measured process streamed
1,000,000 × 20 in 8.47 seconds, reached the first batch in 2.46 ms and
emitted all 300 frame intervals. Its scroll p95 was 95.99 ms. Process
two reached the scroll stage and timed out before emitting frame
intervals. The runner exited 2 and produced no aggregate report. A retry
with four Lavapipe threads and an isolated cache timed out during warmup
scrolling with the same phase deadline. The diagnostic envelope SHA-256
is `c169ccd271783e48dedaa4bfe646b0c050012fae06282fa09efea2433f837ec9`.
Raw logs are retained in `.sdd/benchmark-xvfb-diagnostic/`. An approved
physical-display comparison still requires the matching X11 host.

The local `xtask benchmark-check` also rejected disposable copies of the
archived baseline when one field at a time was changed to an alternate
display ID or the current kernel release. Both runs returned
`baseline mismatch: environment`. These checks exercised the comparison
gate without launching FastDB or changing the archived reports.

The [Linux X11 lifecycle retest](../WORKFLOW_ACCEPTANCE.md#postgresql-desktop-smoke-follow-up-2026-11-23)
also verified window-manager close, Refresh and Criteria with a staged change,
and reconnect after Cancel. The stale Save or Cancel banner remained visible
after the successful reconnect, with clean grid state and unchanged database values.

### Linux Wayland benchmark smoke

The retained Ubuntu 24.04 candidate DEB ran on a private KWin 6.6.7
virtual Wayland output (1600 × 900) using Lavapipe and disposable
configuration and spill paths. No X11 display was available to the
process. A startup trace showed FastDB receiving its toplevel configure
and submitting buffers that the compositor released.

`fastdb --benchmark` exited 0. The 1,000,000 × 20 workload streamed in
10.37 seconds, with 2.14 ms to the first batch. The app emitted all
300 scroll intervals. Nearest-rank p95 was 91.89 ms, the largest
interval was 171.79 ms, and seven intervals exceeded 100 ms. The
scroll watchdog did not trigger. This was one software-rendered
Wayland smoke run. Raw evidence is in `.sdd/wayland-perf-evidence/`.

## Signed-checkpoint retest (2026-11-24)

The retest uses the product tree from signed commit `4197887`, with checkpoint
`7c248df` recording the earlier run. With `FASTDB_REQUIRE_MARIADB_TESTS=1`,
`cargo test --locked --offline -p db-wire --test query_plan_test mariadb -- --nocapture`
passed both MariaDB 11.4.13 cases. Native estimated and actual plan metrics and
ordinary and generated column metadata were exercised on the disposable server.

| Case | Observed result | Status |
| --- | --- | --- |
| C01 / W07 · Connection picker and form | PostgreSQL, MySQL and SQLite were enabled. Db2, MariaDB, Oracle and SQL Server showed `Feature is coming` without switching the selected provider. Back/Next and Main/SSH/TLS navigation retained the connection draft. | Pass for these subcases |
| C02 · PostgreSQL connection feedback | Invalid credentials and a closed loopback port produced distinct actionable errors. The credentials stayed masked and out of the error. Valid credentials passed ping and connected to A. | Pass |
| C03 / C04 · Connection profiles | B connected and the profile kept group, color, timeout and application name. Timeout 121 was rejected. A seeded older disabled MariaDB profile retained its values and could not start a driver. | Pass for these subcases |
| R03 · PostgreSQL read-only profile | SELECT returned the original row. UPDATE and Rename were refused, editing and Save remained disabled, and the transfer wizard had no writable destination. Independent PostgreSQL reads confirmed unchanged data and catalog. | Pass for these subcases |
| R02 · Quit with a pending transaction | The SQL tab saw the pending value while independent PostgreSQL still read the original. Quit released the database sessions and persisted the second, unexecuted INSERT script byte for byte. Relaunch restored both tabs and the script without a result. The saved profile reconnected, the original row remained, and the INSERT's ID was absent. | Pass |
| L01 · Connection ownership | Same-name table tabs showed distinct database-specific values, verified by independent PostgreSQL reads. Switching tabs and reopening A retained the draft without writing it or changing B's grid. | Pass for these subcases |
| L02 · Asynchronous disconnect and close | Disconnecting A during a server-confirmed sleep left zero A sessions while B continued on the same script backend. Closing and reopening a second busy A tab restored the script with an empty result area. PostgreSQL confirmed the sleep ended. | Pass for these subcases |

`cargo test --locked --offline -p xtask -- --nocapture` passed 17 unit tests
and one CLI integration test.

After draft PR #30 opened at `4d2fc89`, a repeat of `bash ci/test-tls.sh`
with Podman passed all six live TLS cases and both SSH-over-TLS cases on
disposable PostgreSQL and MySQL fixtures.

The `7c248df` release artifact was built inside Ubuntu 24.04 from the same
product source as the signed PR head. Its Debian package declared
`libc6 >= 2.39`. In a second clean Ubuntu 24.04 container, apt install
and remove passed, the installed binary matched the release artifact,
the desktop launcher existed, and `ldd` found every linked library.
Package SHA-256 was
`23e7a07a81a58dd45f638ec0bbe57dfc5bb384ffbe69cfec6a0829a207823a77`.
The packaging assertions passed with the release workflow's build libraries.
The initial check on a tools-only image failed because `dpkg-shlibdeps`
could not find the build libraries.

The installed package reproduced the Fontconfig startup failure on Ubuntu 24.04
under Xvfb, exiting 134 at GPUI fallback font resolution. After installing
`fontconfig`, it remained running until the ten-second deadline with no panic.
This was a startup probe. A later [interactive Ubuntu 24.04 launcher test](#ubuntu-2404-desktop-launcher-and-removal) verified the desktop entry, window startup, Quit and removal.

The [draft PR](https://github.com/Matrix-I/fastdb/pull/30) hosted checks passed
for live engine and drivers, Linux build and macOS build/test at `9d224ed`.
The rewritten head `c27ca70` has the same tracked tree. GitHub reports success
for live engine and drivers, Linux build and macOS build/test on that head.
PR #30 was returned to Draft while acceptance remains open. The manual macOS
matrix remains deferred.

### L01 overlap retest after push

The debug build of `227a16d` ran against disposable PostgreSQL A/B fixtures on
private X11. Independent PostgreSQL reads confirmed that each A operation was
waiting on a test lock while B was active.

| Operation | Observed result | Status |
| --- | --- | --- |
| Save | After the lock was released, only A's intended ID 4 changed. B kept its database-specific value. | Pass |
| Refresh | Only A received the refreshed row and new result column. B kept its original rows and columns. PostgreSQL confirmed the new column existed only in A. | Pass |
| Failing Save | A displayed the check-constraint error and retained the staged value. B showed no error or row change. Independent PostgreSQL reads confirmed that both stored amounts remained 0.00. | Pass |
| Navigator catalog Refresh | On `23014e4`, PostgreSQL confirmed A's catalog backend was waiting on a lock while B's selected grid fetched a newly committed value. After release, the new table appeared only in A's expanded Navigator and B remained selected. Independent reads confirmed that the table existed only in A. | Pass |

## MariaDB session compatibility retest

On `17ad68a`, a fresh MariaDB 11.4.13 loopback fixture passed both
`query_plan_test` MariaDB cases with fixture enforcement enabled. The cases
verified estimated and actual plan metrics, temporary table visibility across
plan requests, stable physical session identity, and ordinary and generated
column metadata.

The same server passed six `mysql_session_test` cases with the MySQL
provider configured. They covered savepoints, cancellation and rollback,
read-only behavior, implicit commit and connection loss, caller runtime
lifetime, and closing the tab's server session. The MySQL-specific Explain
case was excluded from this compatibility run.

### MariaDB-configured sessions and direct TLS

On `9d224ed`, a temporary probe linked the existing `db-wire` artifact
and exercised `DbDriverKind::MariaDb` against MariaDB 11.4.13. It used the
production driver factory and tab-session runner.

| Case | Result |
| --- | --- |
| SQL tab isolation | Distinct physical sessions kept their own variables and temporary tables. Savepoint rollback preserved earlier work. Closing A rolled back its pending row and removed only its backend. |
| Cancellation | The native server confirmed A was running the sleep before cancellation. FastDB rolled back the pending write, skipped the next statement, preserved B's backend, and recovered A on a fresh session. |
| Native transaction state | A duplicate-key error left the transaction active. DDL implicitly committed the intended write. Independent reads returned exactly the expected two committed IDs. |
| TLS and client identity | Pooled and pinned connections negotiated TLS 1.3 with the trusted DNS certificate. No CA, unrelated CA and IP hostname mismatch were refused. The server refused plaintext and required an identity for the X.509 account. The correct PKCS#12 identity worked on both paths. |

An initial harness assertion read the first pager from a multi-statement
result. A separate read on the same tab resolved the assertion and the
session probe passed.

### MariaDB-configured SSH and TLS

An isolated OpenSSH fixture exposed only the SSH listener to the host and
forwarded to MariaDB 11.4.13 on its private loopback. The pool and pinned
session negotiated TLS 1.3 through the tunnel. The pool could not see a
pinned session's pending UPDATE. A native client inside the database
container read the expected value after COMMIT. Both FastDB connection paths
refused the wrong SSH fingerprint, a certificate hostname mismatch, an
absent private CA and SSH without the TLS required by the database.

On `c27ca70`, a separate probe used independent SSH sidecars for A and B
against the same MariaDB 11.4.13 server. With A's tunnel intact, the server
confirmed its running sleep and one uncommitted write before Cancel.
FastDB reported cancellation, skipped the later INSERT and COMMIT, and
closed A's backend. With a distinct pinned A session, stopping only A's
sidecar during the same kind of workload returned `connection closed`,
skipped later statements and left only the original committed row. The
A backend disappeared about five seconds after tunnel interruption. B
remained usable on the same backend throughout. Restarting A's sidecar
at the same endpoint and pinned key allowed fresh pooled and pinned TLS
connections. The desktop provider remains gated.

## Linux definition search, SSH and credential store retest

The required live workspace run after the grid fix passed all five target
cases across PostgreSQL, MySQL and SQLite. The tests covered literal
wildcards, PostgreSQL overload identity, scoped SQLite source, and
read-only file and private-memory SQLite URIs. The driver and storage
sources are unchanged from that run through `17ad68a`.

On `17ad68a`, fresh PostgreSQL 16.16 and MySQL 8.4.13 loopback fixtures and
SQLite test databases also passed all five definition search cases.
An earlier SQLite attempt failed at spill-file creation under a read-only
`/var/tmp/fastdb` mount. The successful retry used the normal writable spill
directory.

The same PostgreSQL and MySQL fixtures passed SSH session routing tests with
the database fixtures required. A separate SSH loopback test verified
authentication and closure of owned connections. Five offline cases verified
pinned host keys, MySQL TLS host naming, profile and credential references,
and stale connection test rejection. The native Linux credential round trip
passed on an isolated D-Bus session with a private keyring.

The four vendor script directive parser cases passed. The current Linux
test environment has no configured vendor ports, registered vendor drivers
or local server images for Oracle, SQL Server or Db2. The
[historical provider runs](../PROVIDERS.md#local-validation-2026-11-01)
covered local Oracle and SQL Server plaintext connections. They recorded a
Db2 failure before the connection-string correction. Candidate read/write,
paging, cancellation, transport and batch execution still need live fixtures.

| Current Linux ODBC scope | Status | Fixture needed for retest |
| --- | --- | --- |
| SQL Server | Blocked | Registered Microsoft ODBC Driver 18 and a disposable SQL Server database with a SQL login. |
| Oracle | Blocked | Registered Oracle Instant Client Basic/ODBC and an isolated Oracle service with a fixture user. |
| Db2 | Blocked | Registered IBM Data Server ODBC/CLI driver and a disposable Db2 LUW database. Retest the corrected connection string before running the write and session cases. |

## Linux native backup retest

On `9d224ed`, all 22 `fastdb-app` tests under `native_backup` passed in
four filtered runs of the existing test binary. The provider fixtures were
disposable, with loopback server listeners and fresh restore targets.

| Test filter | Result | Observed assertions |
| --- | --- | --- |
| `native_backup::sqlite` | 2 passed with the checksum-verified SQLite 3.54.1 CLI | Scoped backup preserved exact values, indexes and sequences. Whole-file restore, maintenance and unsafe-target refusal passed. |
| `native_backup::postgres` | 3 passed with PostgreSQL 16.16 client and server, repeated with direct TLS and with SSH + TLS | Full, scoped and partitioned restores passed native read-back. Populated and busy targets were refused. Exactly one concurrent restore succeeded. TLS was independently confirmed with `pg_stat_ssl`. |
| `native_backup::mysql` | 8 passed with MySQL 8.4.13 client and server, repeated with direct TLS | Restores preserved NULL, empty text and binary bytes. Full restore preserved view, routine, trigger and event. Scoped grants refused cross-schema writes. The native client negotiated TLS and refused a certificate hostname mismatch. |
| Remaining `native_backup` cases | 9 passed | Cancel removed partial output and closed the wrapper descendant's input. Existing destinations survived, successful output was published, and diagnostics stayed bounded. |

The initial PostgreSQL fixture needed a wrapper to load its extracted
client libraries after FastDB cleared the child environment. The successful
MySQL fixture used exact schema grants and disabled binary logging so
ordinary fixture accounts could create and restore stored programs. With
binary logging enabled, setup failed at trigger creation with MySQL error
1419. In the first PostgreSQL SSH + TLS fixture, OpenSSH found the disposable
client key and host PAM denied the account. Running OpenSSH in a disposable
Ubuntu 24.04 container alongside PostgreSQL resolved the setup blocker.
The SSH fixture used a pinned disposable host key and restricted forwarding
to the isolated database. The native suite passed its TLS assertion, restore
read-back and maintenance cases through the tunnel. A separate negative fixture
supplied the fingerprint of a different key. The test returned the expected
`SSH host key does not match the trusted fingerprint` before starting backup.
Desktop native tools remain pending.

The required live workspace run also exercised CSV import commit policy
on PostgreSQL and MySQL. Both providers reported atomic rollback and retained
only completed batches in per-batch mode after a constraint failure. Separate
SQLite integration cases verified cancellation after progress rolled back
atomic import, and cancellation during full export preserved the prior file.
The desktop controls for these cancellation and per-batch paths remain pending.

## Linux cross-provider transfer retest

A temporary Rust probe linked the existing `db-wire` artifact and called the
app's table-transfer runner on `9d224ed`. It used disposable PostgreSQL 16.16,
MySQL 8.4.13 and file-backed SQLite targets. Native read-back verified the
destination independently of the transfer result.

| Case | Status | Observed |
| --- | --- | --- |
| PostgreSQL → MySQL, MySQL → SQLite, SQLite → PostgreSQL | Pass for these inputs | Three rows committed per direction. Exact NULL, empty text, Unicode with LF and non-UTF-8 binary matched. The unmapped column kept its default. |
| PostgreSQL → MySQL, duplicate ID 257 | Pass | Atomic mode committed 0 rows. Per-batch mode committed the first 256 rows. Independent reads compared every target row. |
| SQLite → PostgreSQL, cancel at row 257 | Pass | A trigger blocked the second batch on a test-only advisory lock. PostgreSQL confirmed the wait before the caller sent Cancel. Atomic mode committed 0 rows and per-batch mode committed 256. Independent reads checked every row, including the unchanged pre-existing ID 999. |
| MySQL ↔ SQLite, ASCII-shaped BLOB | Fail | A raw BLOB with hex `414243` failed conversion with `Binary edits require a \\x-prefixed hex value`. The destination remained empty. |
| MySQL ↔ SQLite, BLOB containing the bytes of a hex literal | Fail | A raw BLOB with hex `5c7834313432` reported one committed row and no error. Native read-back returned `4142` instead of the original six bytes. |

The runner treats any valid UTF-8 cell as text before mapping. Binary
mapping parses the resulting text as a hex literal, which rejects some
valid BLOBs and silently changes others. The acceptance run records both
failures. A retest needs to verify exact bytes and committed counts for
empty, ASCII, valid UTF-8 and non-UTF-8 BLOBs
across each applicable direction. The probe exercised the transfer runner.

### Cross-provider transfer in the packaged GUI

The Ubuntu 24.04.5 DEB ran on private Xvfb against PostgreSQL 16.16,
MySQL 8.4.13 and file-based SQLite.

| Source → destination | Observed with independent native read-back |
| --- | --- |
| PostgreSQL → MySQL, scalar rows | Atomic Commit reported three committed and zero rejected. All row IDs, signed integers, UTF-8 bytes, empty strings and NULLs matched the source. The existing destination row was unchanged. |
| PostgreSQL → MySQL, duplicate 257 | Atomic Commit reported zero commits. The destination contained only its original ID 257 row. Retrying with Per-Batch Commit reported 256 committed. All IDs 1–256 matched the source, the seed stayed unchanged, and IDs 258–260 were absent. |
| SQLite → PostgreSQL, scalar rows | Atomic Commit reported three committed and zero rejected. The separate SQLite and PostgreSQL reads matched Unicode bytes, empty text, NULLs and integers. The existing seed row stayed unchanged. |
| SQLite → PostgreSQL, FK error | The second row had no matching code in the parent relation. The dialog showed PostgreSQL's constraint error and zero commits. The destination contained only its original seed row. |
| SQLite → PostgreSQL, Cancel in batch two | A disposable trigger blocked ID 257 on an advisory lock. Independent PostgreSQL confirmed both waits before Escape. The GUI reported zero commits under Atomic and 256 under Per-Batch. Both waiters exited before the fixture lock was released. Ordered source and destination reads verified the exact committed prefixes and the unchanged seed. |
| MySQL → SQLite, BLOB rejection | A raw BLOB with bytes `414243` was rejected as requiring a hex prefix. SQLite showed zero committed rows. |
| MySQL → SQLite, silent BLOB change | A raw BLOB with bytes `5c7834313432` produced a success message and one committed row. Independent SQLite read-back showed two-byte BLOB `4142` in place of the six source bytes. |

Screenshots and native reads are retained in
`.sdd/cross-provider-transfer-gui/`.

## PostgreSQL catalog drop retest

On `c27ca70`, the app's `postgres_live_drop_schema_and_database` test passed
with `FASTDB_TEST_NAV_DROP_PORT` set to a disposable PostgreSQL 16.16 server.
The schema drop refused dependent objects without CASCADE and succeeded with
CASCADE. The database drop refused an active session and succeeded after its
release. An independent native client confirmed the target database was
absent and a marker in a sibling database still read `42`. Desktop preview,
Cancel and confirmed-action controls remain pending.

### PostgreSQL desktop catalog deletion

The packaged candidate ran in private Openbox/Xvfb with a disposable
PostgreSQL 16 instance. The Navigator preview quoted `qa_drop` and
defaulted to RESTRICT. Cancel kept its row. Delete with RESTRICT
displayed PostgreSQL's dependency error. Explicit CASCADE changed the
preview and removed the selected schema and its test table.
Independent psql still read 732 in the sibling schema.

A separate app profile connected only to the disposable `qa_drop_db`.
Navigator previewed the quoted database name. Typing `qa_keep_db`
left Delete disabled, and Cancel left both databases present.
Entering the exact name enabled Delete. Independent psql verified
that only `qa_drop_db` was removed, and Navigator returned that profile
to its disconnected state. Evidence is in
`.sdd/postgresql-desktop-catalog-drop-evidence/`.

### PostgreSQL native-tool dialogs

Private Xvfb, Ubuntu 24.04 DEB, PostgreSQL 16.16 server and Ubuntu 16.15
clients. The first Save Backup As attempt surfaced a missing desktop
portal service. Installing the GTK portal in the isolated image and
reloading its private session D-Bus enabled the chooser.

- Backup Database produced a custom archive with a `PGDMP` header. Native
  `pg_restore -l` listed the items and leaf-partition data entries. Repeating
  Run refused the existing file and preserved its SHA-256
  `b9cfcd7f21476ff6c91a402d93dad3a7dbefdfaf235b2d56eb489d7d953608a2`.
- Restore Database chose that file and restored successfully into the
  independently created empty database. Native psql returned 1,000 items
  and zero rows in the initially empty partition in both databases.
  Ordered full items rows matched byte-for-byte, and the view definition
  matched. Reconfirming against the now populated target was refused.
- Backup Table wrote an archive whose table and table-data entries
  named only `release_check.items`. Backup View wrote an archive with the
  `release_check.items_view` entry and its native SELECT definition.
- Client Directory selected a controlled wait-only stand-in for `pg_dump`.
  Cancel Job showed Canceled, reaped the observed child before its
  45-second deadline, and left no destination or staging file. The
  standalone run used the real client for the full and scoped backups.
- Database Maintenance completed Analyze, Vacuum and Reindex.
  Independent PostgreSQL saw both counters increase from 0 to 1,
  timestamps set for both, and the valid items primary-key index move
  from filenode 16403 to 16624.

### PostgreSQL backup cancellation in a short window

The packaged candidate ran in native Wayland under private Sway on Xvfb.
A controlled pg_dump stand-in emitted a progress line and waited in a
live child process. The compositor resized FastDB to 1280 × 350 while
the job was active. The native output and Cancel Job button stayed
visible. Clicking Cancel Job reported Canceled and reaped the child
before its deadline. No final backup or staging file remained in the
isolated destination. Escape then dismissed the modal.
Evidence is in `.sdd/postgresql-short-backup-evidence/`.

### PostgreSQL scoped restore and invalid archive retest

The same DEB ran in private Sway nested in Xvfb, with a disposable
PostgreSQL 16.16 server and its native clients selected through GTK.
Changing the target after confirmation removed the check and disabled
Run. Choosing a non-archive file and reconfirming displayed
`Select a PostgreSQL custom archive file`. Independent psql found zero
user tables in that target.

A separate empty target received a custom archive built by native
pg_dump for only `scoped_qa.sample`. With `scoped_qa` entered in
Source schema for scoped archive, FastDB displayed Completed and
native output included CREATE SCHEMA. Independent psql matched both
source rows, including SQL NULL, empty text, zero-byte binary and
`00 01 7f ff`. The unrelated source table was absent from the target.
Evidence is in `.sdd/postgresql-scoped-restore-evidence/`.

### PostgreSQL partitioned table backup and restore

On the Ubuntu 24.04 candidate in private Openbox/Xvfb with a GTK
portal, PostgreSQL 16.16 held two nonempty partitions under
`restore_qa.events`. **Backup Table…** on the parent produced a
custom archive with the parent, both children, their attach entries and
data for both children. The Ubuntu 16.15 `pg_restore -l` confirmed
the archive scope. FastDB restored that archive into the independently
verified empty `qa_dst` database using the source schema field.

Independent PostgreSQL reads matched both partition bounds and all
four rows byte for byte, including the partition that owned each row,
SQL NULL, empty text, an empty binary value, Vietnamese and Japanese
text, and binary values containing zero bytes. The three primary keys
were present. The unrelated source table was absent from the target.

A separate backup with **Client Directory…** pointed to a directory
without `pg_dump` displayed the missing-executable error. The new
destination and staging files were absent, and the completed archive's
SHA-256 stayed unchanged. Archive listings, native read-back and
screenshots are in `.sdd/postgresql-partitioned-restore-evidence/`.

### PostgreSQL view restore and connected target

On the same packaged candidate in private Wayland, a native pg_dump custom
archive included a view and its base table in `scoped_qa`. It excluded an
unrelated source table. With a separate FastDB profile still connected to
the empty target, Restore Database reported that other target sessions
must disconnect. Independent psql confirmed the idle target session
was active and no source schema had been created.

After disconnecting the target profile, FastDB restored the same archive
with the exact source schema and displayed Completed. Native psql matched
the view definition and both result rows, including a SQL NULL, and
confirmed the unrelated table was absent. Reconnecting the target profile
populated Navigator with the restored table and view. Opening the view
displayed IDs 731 and 732 with values PRESERVED and NULL. Refresh of
metadata in an already-connected PostgreSQL restore target is blocked by
the restore guard. Evidence is in `.sdd/postgresql-view-restore-evidence/`.

## Linux table-data search on the packaged GUI

Retained Ubuntu 24.04 candidate DEB on private Xvfb, scale 1. At the default
1280-pixel window width with Navigator open, the footer ended partway
through Import CSV. **Search table data…** was visible after widening the
window to 1700 pixels. The provider runs used that width.

| Provider | Observed on a disposable fixture |
| --- | --- |
| SQLite 3.45.1 CLI for independent read-back | The literal `%_` matched only ID 1. After an independent update, opening that hit showed the original captured label and numeric value. ASCII case folding matched `alpha plain`. BLOB-only and NUL-containing text probes each returned zero. The 55 matching rows produced 50 hits. A token after character 600 was searchable, while the row preview was labeled at most 512 characters and showed a truncated value. |
| PostgreSQL 16.16 | The quoted name returned the one exact result that independent psql found. A subsequent native UPDATE caused the old hit to report that the row changed or was no longer visible. With a fixture lock active, psql showed dedicated waiter 94 blocked by 93 on the relation. Cancel reported cancellation, removed waiter 94 and left the original lock holder asleep. |
| MySQL 8.4 | The literal `%_` and a lowercase search for an uppercase JSON value each returned one row. The BLOB-only token returned zero. The 56 native matches for the repeated term yielded 50 hits. Opening the JSON hit after an independent update preserved the original captured row. |

A second private Xvfb run exercised cancellation against active lock waits.
On MySQL 8.4, an independent process list showed FastDB session 14 waiting
for a table metadata lock while probing the table, and fixture session 26
holding the lock. FastDB showed Cancelled with zero tables and rows.
The next process-list read showed session 14 gone and session 26 still
holding the lock. After release, the same dialog returned the single match.

On SQLite, an independent reader confirmed the fixture's exclusive lock.
The FastDB file descriptor count for the database rose from two to three
during the search and returned to two after Cancel while the fixture still
held the lock. The dialog showed Cancelled and also reported
`database is locked` as a failed table request. After release a repeat
returned the single match, which independently read back as ID 731
on both providers. Raw evidence is in
`.sdd/table-search-cancellation-evidence/`.

The SQLite and MySQL menu entry checks on the same DEB failed. The
connected, writable, file-backed SQLite profile showed its full path
and loaded its table. Its menu omitted Backup Database, Restore Database
and Database Maintenance. The connected MySQL profile also loaded its
table but omitted Backup Database and Restore Database. Both table menus
omitted Backup Table and the MySQL menu omitted Table Maintenance.
PostgreSQL exposed the applicable entries in the same app instance.
The connection success path stores a session configuration only for
PostgreSQL, while the native-tool menu requires that configuration for
each provider. This blocks the corresponding MySQL and SQLite desktop
operations. The separate native-client tests above cover those
operations below the GUI.

## Linux MySQL and SQLite connection profiles

The Ubuntu 24.04.5 candidate DEB ran on private Xvfb at scale 1.
The server was MySQL 8.4.13 and the SQLite profiles used isolated files
and in-memory databases. CLI reads inspected the stored data independently.

| Case | Observed |
| --- | --- |
| CON-01 | The MySQL connection dialog returned native access denied (28000/1045) for a wrong password and connection refused (111) on the closed TCP port. The valid profile passed Test and Connect. After Ctrl+Q, the old app process exited. A new process reopened the table tab and the saved profile reconnected without re-entering the password. Native MySQL and the reopened grid agreed on both fixture rows. |
| CON-02 | The existing SQLite file opened and displayed its seeded row. Connect created a new file and a write from its SQL tab stored marker 751, verified by sqlite3. A missing read-only file returned an open failure and was not created. Memory A stored its marker and Memory B independently returned zero matches for the table name. |
| CON-07 | Read-only MySQL and SQLite profiles showed their mode, allowed reading and rejected SQL UPDATE. Double-click and F2 left the selected cells outside edit mode. Both providers opened a schema preview, then refused Add Column on Execute. Native reads confirmed unchanged rows and columns. Transfer listed only the writable MySQL and SQLite tables, omitting both live read-only profiles. Earlier R03 supplies PostgreSQL coverage. |
| CON-08 | Save rejected the non-hex color. The valid MySQL profile displayed a green color marker under group QA after app restart. The saved settings held `#37a768` and group QA. |
| CON-09 | Test rejected 0 and 121 seconds, and a live Test accepted 120. On a local socket that accepted but sent no MySQL greeting, FastDB reported a connection timeout. The independent listener measured peer closure at 1,001.1 and 2,001.4 ms for the configured 1 and 2 seconds. The network-path retest below covers the unroutable host. |
| CON-14 | SQLite Main Tab and Shift+Tab moved between visible name and database fields. Advanced Tab moved from group to color. Switching back retained the draft. The 860 × 620 window exposed the body and footer. MySQL Copy details returned the complete clipped error without the supplied substitute password. |

### Cancel and scalar transfer across providers

- On the writable MySQL profile, the independent process list saw backend
  41 executing the 25-second read before the GUI Cancel click. The query
  disappeared from the server before its deadline. The same editor tab
  then returned `mysql-a-only`, matching the native read.
- SQLite began a recursive 100-million-step read. Cancel closed its
  executing session before the aggregate returned. A fresh read in the
  same editor tab returned `sqlite-a-only`, matching sqlite3.
- The GUI refused the original MySQL table as its own transfer target.
  A transfer to the SQLite file's new `transfer_items` table mapped the
  integer ID and text note. Atomic Commit reported two processed, two
  committed and zero rejected, matching native read-back in ID order.
  Repeating the transfer hit SQLite primary-key error 1555, reported
  zero committed rows and preserved both destination rows.

Screenshots and listener read-back are in `.sdd/connection-gui-evidence/`.

## Linux MySQL and SQLite SQL editor

Capture used the Ubuntu 24.04 candidate DEB on private Xvfb with MySQL
8.4.13 and SQLite. Native reads used a separate client or database handle.

| Case | Observed |
| --- | --- |
| SQL-02 | Execute on MySQL and SQLite ran the selected second statement (202) and, separately, the first statement at the caret (101). Run script appended both results. CTE returned 311, UNION ALL returned 312 and 313, and INSERT … SELECT wrote ID 4, confirmed through native reads. Separate caret executions of adjacent SELECT lines without semicolons returned 404 and 405. |
| SQL-03 | MySQL Run script accepted `DELIMITER $$`, created a procedure with two internal statements and returned 701 from its CALL. Independent MySQL returned the same value. SQLite Run script created a trigger with two internal INSERTs and executed the outer INSERT once. SQLite read-back showed exactly two audit rows for ID 2. PostgreSQL's dollar-quoted DO block is covered in S05. |
| SQL-04 | MySQL tab A returned its temporary table marker and variable 617 on backend 18 across separate executions. Tab B used backend 24, returned SQL NULL for the variable and received native error 1146 for the temporary table. A retained both values after switching back. A pending UPDATE was invisible to the native client. DDL changed the tab from In transaction to Idle and made the exact change visible. A later ROLLBACK left the committed value and new table intact. |
| SQL-04 · SQLite | Tab A retained its TEMP table across separate actions. Tab B found zero matching TEMP tables and received a missing-table error on SELECT. Returning to A read the same marker. A separate `BEGIN IMMEDIATE` change was invisible to the independent reader, and ROLLBACK restored the original value. The per-preset in-memory isolation was exercised in CON-02. |
| SQL-07 | MySQL `SHOW WARNINGS` on the same tab returned code 1292 for the synthetic invalid integer. SQLite INSERT … SELECT reported a single affected row, confirmed independently. The separate Execution log route still fails as recorded under S05. |
| SQL-09 | MySQL Explain showed the estimated plan and the expensive-node action selected its query-block node. Analyze showed one row and loop with actual timing, matching the native CLI count. SQLite Explain matched the native primary-key lookup and marked expensive-node navigation unavailable without metrics. With the GTK portal in the private session, both providers saved v1 JSON with the expected provider and native rows. Comparing showed estimated versus actual on MySQL and scan versus lookup on SQLite. Opening both files restored their plan results without executing SQL. The PostgreSQL snapshot and invalid-file checks were covered earlier. |
| SQL-12 | MySQL returned length 67,283 from a 67,327-byte statement. After Quit, its history file retained the short marker 934 and omitted the oversized statement. SQLite's history file retained marker 933 after Quit. See the retest below for the SQLite oversized result and missing History action. |
| SQL-14 | MySQL and SQLite each ran 23 statements returning markers 1–23. The newest retained result returned 23 and the first retained result returned 4. Closing the first selected its adjacent result, which returned 5. PostgreSQL split bounds are covered in W05. See the retest below for PostgreSQL retention. |
| APP-10 · unavailable portal | Saving the SQLite plan when the desktop portal was absent produced a visible service error and left the active plan visible. |

### Linux PostgreSQL Query Plan node navigation

The Ubuntu 24.04 candidate ran in private Openbox/Xvfb against a
disposable PostgreSQL 16 server. The grouped read-only SELECT produced a
Sort, Aggregate and Seq Scan over 30,000 fixture rows. FastDB showed the
same estimated costs of 583.40, 583.13 and 433.00 as independent
PostgreSQL JSON output. Next selected those operators in that order and
wrapped to Sort. Previous from Sort wrapped back to Seq Scan. With
Sort collapsed, Next selected Aggregate and reopened its ancestor.

Analyze appended a separate plan and labeled the ranking as inclusive
time multiplied by loops. On that GUI execution all three operators
had one loop. Sort took 7.592 ms, Aggregate 7.520 ms and Seq Scan
2.190 ms. The independent read-only Analyze returned the same ordering,
13 output groups and 30,000 scanned rows. Next selected Sort, Aggregate
and Seq Scan. Previous from Seq Scan returned to Aggregate.
Switching to the estimate restored its previously selected Seq Scan.
Native JSON, screenshot selections and pixel assertions are in
`.sdd/postgresql-plan-ranking-evidence/`.

### Linux Query Plan file limits and provider checks

On the Ubuntu 24.04 candidate in private Openbox/Xvfb with the GTK portal,
SQLite returned `plan-fixture` in Result 1 and showed the native
`qa_plan` primary-key lookup in Result 2. Both matched independent
SQLite reads.

| Input | GUI result |
| --- | --- |
| 16,777,217-byte JSON snapshot | Open rejected the file with the 16 MiB limit. |
| Malformed JSON | Open reported invalid saved-plan JSON. |
| Row with three values for four columns | Open reported invalid saved-plan row width. |
| Snapshot with 4,097 rows | Open reported too many saved-plan rows. |
| PostgreSQL snapshot compared to the SQLite plan | Compare rejected the different provider and explained that engine costs cannot be compared. |
| 16,777,216-byte JSON snapshot | Open appended Result 3 with the same lookup and reported that no SQL was executed. |

The two size fixtures contained identical plan data and differed by one
trailing whitespace byte. Every rejected request left the two original
result tabs in place. After the valid open, the original rows and native
plan remained readable, and an independent SQLite read still returned
the same fixture row. Input hashes and screenshots are in
`.sdd/query-plan-file-limits-evidence/`.

## Linux SQL history and result retention retest

The Ubuntu 24.04 candidate package (SHA-256
`23e7a07a81a58dd45f638ec0bbe57dfc5bb384ffbe69cfec6a0829a207823a77`)
ran on Debian 13 with private Xvfb and the Openbox window manager. A missing
Ubuntu mirror package prevented setting up the new Ubuntu fixture.

| Case | Observed |
| --- | --- |
| SQL-01 | `+ SQL` created a script per profile. The PostgreSQL and SQLite autosave files under `scripts/qa-pg` and `scripts/qa-sq` matched the input byte for byte (635 and 67,113 bytes). The SQL rail has no Rename or Delete action. See the MySQL and SQLite chooser retest below. |
| SQL-08 | In a fresh workspace, SQLite returned 951 and the Transaction log search for `qa_clean_quit` returned its single successful entry. Closing the modal restored the result. The matching mode-0600 log file disappeared after GUI Quit terminated FastDB, while the container and log bind mount stayed live. The previous app session was absent from the new process's modal. Earlier W04 covers PostgreSQL paging. The controlled close retest below verified release of the pending snapshot descriptor. |
| SQL-12 | PostgreSQL and SQLite returned length 67,067 from statements of 67,109 and 67,113 bytes. Their separate history files retained the short queries and omitted the oversized statements. Together with the preceding MySQL run, file-level omission is verified for all three providers. The visible editor rails lacked History, and the history handler has no registered invocation. Recall into the editor therefore fails the documented GUI workflow. |
| SQL-14 | PostgreSQL joined MySQL and SQLite in retaining the newest 20 results. After submitting markers 1–23, the newest result was 23, the first retained result was 4, and closing that result selected 5. The SQLite splitter attempt after the large statement yielded no reliable bound measurement. The short-statement split retest below covers both providers. |

In the initial session, after running the 67,113-byte SQLite statement and
attempting to drag its result splitter, the Transaction log opened but
search and dismissal did not respond during the test. FastDB logged a
Quit action and the process and log file still existed after the bounded
wait. The retest used the same binary in a fresh workspace with a short
statement and completed successfully. The cause of the first session's
failure to quit is unconfirmed.

### MySQL and SQLite split bounds and script reopening

The same Debian 13 Xvfb fixture and candidate package were used with a
disposable MySQL 8.4 server and SQLite file. Both editors clamped the
divider to the same screenshot-relative offsets, 201 and 693 pixels,
after dragging past the upper and lower limits of the split area.
The split starts 96 pixels below the window top and spans about 704 pixels,
so these offsets verify the 15% and 85% bounds at scale 1. The MySQL query
returned 351 and the SQLite fixture query returned 352.

The SQLite chooser initially listed only its own `Script-1`. Turning on
**Show scripts for all connections** showed both identically named scripts
with their connection names. Choosing the closed MySQL script reopened it
on MySQL, and Execute returned 351. Navigator **Files** search for
`Script-1` also returned both qualified entries and reopened the closed
MySQL tab with the correct SQL. The MySQL and SQLite autosave files held
the exact short statements (30 and 44 bytes). The SQL tab context menu
offered only tab close actions. Rename collision and Delete to `.trash`
cannot be exercised through the candidate GUI.

The Transaction log search for `qa_mysql_split` returned exactly three
rows, one syntax error and two successful one-row results. Normal GUI Quit
removed its mode-0600 log file while the container stayed live.

### Linux SQL session control read-back

The same candidate ran on Debian 13 private Xvfb with MySQL 8.4.13 and
file-backed SQLite. The PostgreSQL Database, Schema and mode paths are
recorded in T04.

| Provider | Observed |
| --- | --- |
| MySQL | The Database prompt opened `fastdb_alt` in a temporary tab. Its first query returned database `fastdb_alt`, backend 20 and value 703. The native client confirmed 703. Returning to the original tab still reported `fastdb_qa` and backend 18. The transaction menu applied Read committed and Manual directly. Native read-back on backend 18 reported `READ-COMMITTED` and `@@session.autocommit = 0`. Schema returned the provider's unsupported message and preserved the result. |
| SQLite | The Database and Schema buttons both returned the provider's unsupported message and preserved the result of 701. The transaction menu offered Serializable and read back `PRAGMA read_uncommitted = 0`. Manual mode left the row update pending. The tab read 711, while an independent SQLite reader still read 701. Toolbar Rollback restored Idle and left the independent value at 701. Choosing Auto and running a fresh query returned 701 on the same tab. |

The transaction menu applies Isolation and Auto/Manual selections directly.
The separate documented preview dialogs have no reachable GUI entry on
this build.

### Linux SQL parameter prompts

- PostgreSQL prompted for the named Integer and returned 812.
- MySQL prompted only for `:qa_parameter`. The SQL preview preserved
  `@qa_native` and `@@session.autocommit`. The result returned native
  values 813 and 0 alongside the bound Integer 814.
- SQLite prompted for positional `?`, doubled the apostrophe in the
  String preview, and returned `O'Brien` as one value. A separate prompt
  returned Float 1.25, Boolean 1, Date/Time `2026-11-26`, SQL NULL and
  Raw SQL expression result 815. The Integer field blocked Execute for
  `nope` with a validation error. Correcting it returned 816.
- PostgreSQL and MySQL independently returned the same String, Float,
  Date/Time, NULL and Raw SQL values. PostgreSQL rendered Boolean as
  `true` and MySQL rendered it as 1.

### Linux Transaction log pagination and timeout controls

The same candidate ran on Debian 13 with private Xvfb. A saved script
executed 130 distinct one-row statements each on MySQL 8.4.13 and SQLite.
The Transaction log filter and scroll reached marker 130 for both.
An independent read of the session JSONL confirmed exactly 130 matching
one-row successes in order per provider, with no duplicates. PostgreSQL
paging is covered in W04.

The large SQLite script from the previous retest returned 67,067 again.
After a pointer drag near the editor/result divider, the Transaction log
opened, but the search field and Close button did not respond during the
capture. A subsequent Ctrl+Q left the process and its mode-0600 session
file present after a 25-second wait. The app logged the Quit action
about 50 seconds after it was sent, and the process and exact log file
were later absent while the container remained live. This confirms a
responsiveness failure after the large script interaction and eventual
log cleanup. The trigger and cause remain unconfirmed. Normal
short-statement exits removed the file in the preceding runs.

The candidate has no GUI Timeout preset selector. MySQL
`SELECT SLEEP(6)` ran with the default settings, returned 0, and logged
6,000 ms. The SQLite recursive CTE was active before Cancel. Cancel
reported a closed session, and the next query in the same tab returned
the independently seeded row 42. PostgreSQL `pg_sleep` and cancellation
are covered in S06. These runs exercise long statements and recovery.
The documented preset configuration and firing remain unverified.

### Linux Transaction log resource release on close

The packaged candidate ran with a SQLite fixture on private Xvfb.
A test-only interposer delayed a positional read of the disposable SQL
session log. It did not change the bytes returned by the read. The
transaction log opened with an empty page while the writer and pending
snapshot each held a descriptor for the same file. Escape dismissed the
modal before the delay was released. After release, the snapshot
descriptor disappeared and only the session writer remained.

The same script tab returned 953 on its next query. Reopening the modal
and searching for the query marker displayed exactly one result, matching
the independent JSONL read. Ctrl+Q removed the mode-0600 session log while
the fixture container stayed live. Relaunch without the interposer created
a different session file, and the old marker was absent in both the modal
and that file. Evidence is in `.sdd/transaction-log-close-evidence/`.

## Linux Navigator search cap and layout retest

The Debian 13 private Xvfb fixture used the same FastDB candidate with
MySQL 8.4.13 and SQLite. Each native catalog contained 120 tables named
`qa_find_001` through `qa_find_120`. The Tables and objects filter
`qa_find_` started at 001 and stopped at 100 on each provider after
scrolling through the tree. Searching for the full name `qa_find_120`
returned that table. Clearing the query restored the expanded Tables
folder and the other tables. A search for `child` retained separate
Tables and Views folders and displayed the matching table and view.

Both child diagrams displayed three qualified tables and four column
links, consistent with independent MySQL catalog and SQLite PRAGMA
reads. Clicking the composite edge highlighted both source and target
column pairs. Dragging each parent card redrew the links and saved distinct
positions under the corresponding connection and table. SQLite Ctrl+wheel
changed zoom from 100% to 200%. After a normal Quit, both diagram tabs
reopened and restored their moved cards after reconnecting. MySQL Export
SVG showed a portal service error with the graph still visible when the
portal was absent. The enabled-portal retest follows below.

The Connections divider moved from 280 to 390 pixels at scale 1.
`workspace-state.json` saved a width of 390.00168. After FastDB exited
and restarted with the same profile, the settled window again showed a
390-pixel Navigator with both provider tabs restored.

### Linux SVG export and PostgreSQL search cap

The Debian 13 fixture used GTK portal 1.15.3 over xdg-desktop-portal
1.20.3 in the application's private D-Bus session. Canceling an SVG save
left the isolated destination directory empty. Saving exported 2,629
bytes for MySQL, 2,585 for SQLite and 2,486 for PostgreSQL. XML checking
identified all three qualified tables and mapped each of the four
arrowed polylines back to its exact source and target column. The
exported MySQL and SQLite parent-card positions agreed with their saved
layout coordinates.

Firefox ESR 153.8.0 opened and rendered each local SVG at 900×500.
In the MySQL export, solid heading pixels for `fastdb_qa.incoming` and
`fastdb_qa.parent` extended two pixels beyond their right card borders.
The shorter PostgreSQL and SQLite headings were inside their cards.
The MySQL export's text fit therefore fails even though the file and
all its routes render.

PostgreSQL completed the Tables and objects cap check. Its native
catalog contained 120 numbered tables. The filtered tree stopped at
100, the exact query for table 120 found it, and clearing the query
restored the expanded tree. The three-provider table cap is verified.

### Linux Navigator caps in Connections, Files and Catalogs

In the Debian 13 private Xvfb fixture, the active PostgreSQL database
`qa_cap_database` contained 120 schemas named `qa_cap_001` through
`qa_cap_120`. The config contained 120 disposable profiles and 120
SQL files. The native catalog and on-disk config and SQL bytes were
checked independently.

| Mode | Observed |
| --- | --- |
| Connections | The shared prefix `qa_profile_` stopped at profile 100. The exact query found profile 120. Clicking it updated the saved active connection ID to `qa-profile-120` while the open script retained its PostgreSQL connection. |
| Catalogs and schemas | The shared prefix `qa_cap_` displayed the database row and schema rows 001–100, totaling 101 against the expected maximum of 100. Searching for `qa_cap_120` returned only that schema. |
| Files | The shared prefix `qa_file_` stopped at file 100 when scrolled to the end. The exact query located file 120. Clicking it opened the PostgreSQL script, whose execution returned the fixture value 120. |

The Catalogs and schemas renderer adds the active database independently
of its 100-schema bound. The result cap therefore fails when both the
database and at least 100 schemas match. The Tables and objects cap
passed on all three providers in the preceding runs.

### Linux Navigator creation and refresh

The Debian 13 private Xvfb fixture ran the packaged candidate against
MySQL 8.4.13 and file-backed SQLite. Earlier PostgreSQL D03 verified
creation of a table and a view, and the A/B retest verified connection
ownership during catalog Refresh.

- MySQL Create Table previewed a qualified identifier, a non-null
  `bigint` primary key and a nullable `varchar(255)`. The native catalog
  confirmed both columns. Canceling a separately named table left that
  name absent. Create View produced a view that independently returned
  601. Schema Refresh discovered a table created by a native client.
- SQLite Create Table previewed an `INTEGER` primary key and nullable
  `TEXT` column. The native schema matched both. Create View put the view
  in the Views folder, and a separate SQLite reader returned 601.
- On SQLite, Refresh from the connection, Databases folder, database,
  Schemas folder, schema, Tables folder and table each discovered a new
  marker table created independently. Refresh was absent from the Views
  folder and view menus. The MySQL Views folder likewise offered Create
  View only.

A follow-up on the same packaged candidate used PostgreSQL 16.16 and
MySQL 8.4.13 on private Openbox/Xvfb. For each provider, a native client
created and confirmed a distinct marker table before each of seven
Refresh actions. The open tree lacked the marker before the click and
showed it after Refresh from the connection, Databases folder, database,
Schemas folder, schema, Tables folder and an existing table. All 14
routes passed. Both providers' Views folder and individual view menus
lacked Refresh, so NAV-05 remains Fail for the missing per-node actions.
Screenshots and native read-back are in `.sdd/navigator-refresh-evidence/`.

### Linux MySQL and SQLite table mutations

The packaged Ubuntu 24.04 candidate ran in private Openbox/Xvfb with
MySQL 8.4.13 and a file-backed SQLite database. On each provider the
initial `qa_ddl` and unrelated `qa_keep` tables each held one sentinel row.

| Action | SQLite | MySQL |
| --- | --- | --- |
| Add Column | The native `sqlite_schema` showed nullable `qa_probe text DEFAULT 17`. The original row 731 remained. | Preview and native catalogs showed nullable `qa_probe17 varchar(40)` with default `marker`. Independent SQL returned row 831 with that value. The dialog warned that MySQL DDL commits automatically. |
| Rename Table | Empty input disabled Execute. Renaming to `qa_keep` returned the native name collision and preserved both tables. Renaming to `qa_renamed` updated the tree and catalog and preserved row 731. | Empty input disabled Execute. Renaming to `qa_keep` returned native error 1050 and preserved both tables. Renaming to `qa_renamed` updated the tree and catalog and preserved row 831. |
| Truncate Table | Preview used `DELETE FROM "qa_renamed";`. Cancel kept row 731. Confirm left the table with zero rows. | Preview used qualified `TRUNCATE TABLE`. Cancel kept row 831. Confirm left the table with zero rows. |
| Drop Table | Cancel kept `qa_renamed`; confirm removed it from the tree and native schema. The unrelated row 732 in `qa_keep` remained. | Cancel kept `qa_renamed`; confirm removed its catalog entries and tree row. The unrelated row 832 in `qa_keep` remained. |

The Truncate and Drop dialogs on both providers offered an enabled
**CASCADE dependent objects** checkbox. Toggling it did not alter either
SQL preview. Those probes were canceled. The confirmed operations used
the unchecked option. The supported Cancel and Execute flows passed.
The Cascade control remains a UI defect on these providers. Screenshots
and independent catalog and row reads are in `.sdd/object-ddl-evidence/`.

### Linux Reveal active object across connections

The packaged candidate ran on Debian 13 private Xvfb with MySQL 8.4.13
and two separate SQLite files. Each contained a `qa_items` table and
`qa_view` view with the same identifiers and a different row value.

| Target | Observed |
| --- | --- |
| MySQL table and view | Reveal reopened the collapsed branch and selected the correct Tables or Views row. The grids both showed 703. With the view active and SQLite A selected in Navigator, Reveal switched Navigator back to MySQL and selected its view. |
| MySQL column tab | Reveal selected the owning `qa_items` row from the `qa_items.value` tab. The column tab showed its PostgreSQL-only editing message. |
| SQLite table and view | Reveal selected SQLite A's table and SQLite B's view in their respective branches. The grids showed 701 and 702, matching independent SQLite reads. |
| SQLite column tab | With the SQLite A column tab active and SQLite B connected, Reveal selected the owning table in SQLite A. |
| Target hidden by filter | With the MySQL `qa_items.value` tab active, searching for `qa_view` hid the owning table. Clicking Reveal left the filter and visible tree unchanged, so the target remained hidden. |

PostgreSQL view evidence is recorded under M04. A follow-up on the
packaged candidate in private Openbox/Xvfb used PostgreSQL 16.16 and
MySQL 8.4.13. Both had a `qa_anchor` table. Independent reads returned
611 on PostgreSQL and 711 on MySQL. FastDB displayed 611 in the
PostgreSQL table tab. From that tab, and again from its `qa_anchor.id`
column tab, Reveal switched the Navigator from the MySQL tree to
PostgreSQL and selected the owning `qa_anchor` row. With `qa_view`
entered as a filter, Reveal left the PostgreSQL target hidden.
Clearing the filter and clicking Reveal selected it again. Evidence is
in `.sdd/navigator-reveal-postgres-evidence/`.

### Linux Catalog Browser keyboard route

On the packaged candidate in Debian 13 private Xvfb, focusing the
empty Tables and objects search field and pressing Enter opened
Catalog objects. The documented **Browse catalog objects…** row was
absent from the empty search on a connected PostgreSQL profile.

PostgreSQL listed `public.qa_catalog_seq` and showed its native start
value 739. Toggle favorite persisted the sequence identity with owner
`qa-pg`. After an independent psql session renamed it, Refresh listed
`public.qa_catalog_renamed`. Selecting the old favorite displayed
`Favorite no longer exists; refresh the catalog`. Native psql confirmed
only the new name with the same start value. M04 previously verified
favorite persistence across a complete restart.

The keyboard route on connected MySQL and SQLite opened Catalog objects
and displayed `Object discovery is not supported by this provider`.
Neither profile displayed the PostgreSQL favorite.

### Linux Navigator range selection and clicked-item actions

On the packaged candidate in private Openbox/Xvfb with SQLite,
clicking beta and then Shift-clicking omega highlighted beta through
omega inclusively. Shift-clicking alpha then highlighted only alpha
and beta, retaining beta as the anchor. With the Tables folder collapsed,
Shift-clicking the visible view replaced the hidden anchor. Reopening
Tables showed only the view selected.

With beta through kappa selected, right-clicking the unselected omega
and choosing View Table Data opened only omega. It returned ID 735.
Escape cleared the highlighted relations. A separate double-click
opened only alpha from the same Navigator, returning ID 731 with
omega still open. Both IDs matched independent SQLite reads.

Filtering for `group` hid the selected alpha. Shift-clicking group_b
selected only group_b, and clearing the filter preserved that selection.
Shift-clicking omega then selected group_b, kappa, and omega, showing
that group_b became the anchor.

A second SQLite connection with the same relation names initially
showed no selection. Selecting alpha through group_a there and
switching back to the first connection restored its group_b through
omega selection. Returning to the second connection restored its
alpha through group_a selection. Evidence is in
`.sdd/navigator-range-evidence/`.

### Linux Navigator folder and key metadata

On the packaged candidate under Debian 13 private Xvfb, each fixture
contained `qa_child`, `qa_parent`, and `qa_view`. The tree showed both
tables under Tables and the view under Views. PostgreSQL also showed
`qa_mat` under Materialized Views and the foreign table `qa_other`
under Other Relations, matching `pg_class`. MySQL and SQLite showed
empty states in those last two folders.

| Provider | Expanded `qa_child` tree and independent catalog comparison |
| --- | --- |
| PostgreSQL | Three columns with the expected nullability. Constraints listed the primary key, unique key, foreign key and the two generated NOT NULL checks in `information_schema`. Foreign Keys listed `qa_parent_fk`. Indexes listed the PK, unique index and `qa_parent_idx`. |
| MySQL | Three columns with the expected nullability. Constraints listed PRIMARY, `qa_label_unique` and `qa_parent_fk`, with the FK also in Foreign Keys. Indexes listed PRIMARY, the unique index and `qa_parent_idx`. Native metadata linked `parent_id` to `qa_parent.id`. |
| SQLite | Three columns with the expected nullability, with the `VARCHAR(32)` column displayed as text. Constraints listed the primary key, `fk_0` and the unique autoindex. Foreign Keys listed `fk_0`. Both indexes matched `PRAGMA index_list`, and `PRAGMA foreign_key_list` confirmed the same target column. |

### Linux plain-text result limits

The packaged candidate ran on Debian 13 private Xvfb with MySQL 8.4.13
and SQLite. Independent reads confirmed 2,001 rows, IDs 1 through
2,001, and one SQL NULL. Row 1 contained 266 characters, including
a marker after character 255. Each saved SQL tab executed an ordered
query with an explicit limit of 2,001. The Text header reported all
2,001 loaded rows.

Copy in each Text view produced 524,523 bytes. Reading the Xvfb
clipboard showed exactly IDs 1–2,000 in order, the first 255
characters of row 1 without the trailing marker, and `[NULL]` for row
2. The two clipboard files were byte-identical. The SQLite retest
replaced the clipboard with a sentinel before Copy to verify that the
data came from the active SQLite tab. PostgreSQL W06 previously
verified the same limits.

### Linux Record view on wide tables

The packaged candidate ran on Debian 13 private Xvfb with a file-backed
SQLite table containing 66 columns and two rows. A native SQLite read
returned `101|164|165|166` and `201|264|265|266` for columns 1, 64,
65 and 66.

- From the selected Grid cell on row 2, Tab opened the compact record.
  Scrolling reached field `c66` with value 266. A second Tab restored
  the Grid and the selected row 2 cell.
- The Record button opened the read-only Grid at row 2, fields 1–64.
  Next fields showed exactly `c65=265` and `c66=266`. Switching to
  Text retained both row and field page. Previous fields restored
  fields 1–64, and Previous row showed row 1 with `c01=101`. Escape
  restored the two-row Text presentation.

PostgreSQL and MySQL repeated the same 66-column fixture. Native
read-back returned exactly the same boundary values. Tab opened row 2
in compact Record and a second Tab restored the selected cell. The
Record button showed fields 1–64, then 65–66 with values 265 and 266.
Switching to Text kept the same row and field page. Escape restored
the two-row Text presentation on both providers. PostgreSQL W06 also
covers ordinary Record navigation.

### PostgreSQL Record mode in a SQL result

On the packaged candidate in private Xvfb, a PostgreSQL SQL result
contained three rows with exact decimals, timezone-aware timestamps
and a SQL NULL. Selecting ID 732 opened Record at row 2. Grid and
Text kept that row, Next moved to ID 733, and Previous returned to
ID 732. Double-click and F2 left Record read-only. Escape and All rows
returned to the three-row Grid with the amount cell still selected.

An independent X11 clipboard read matched the native psql decimal
`9007199254740993.0100` and the same UTC instant, with `(SQL NULL)`
for the empty note field. Grid displayed both timestamp inputs with
different source offsets at the same UTC instant. Evidence is in
`.sdd/postgres-sql-record-evidence/`.

### Linux Find in loaded results

The packaged candidate ran in private Debian 13 Xvfb. The table footer
exposed Find at 1,600 pixels with the Navigator hidden. The search
`needle` matched rows 1 and 3 in order. Next wrapped to 1, Previous
wrapped back to 3, and the upper-case value in row 4 was skipped.

Editing row 2 staged `staged_probe`. Find selected it at row 2,
column 2 without saving. An independent SQLite read still returned
`original`, and Grid Cancel restored the displayed value. The dialog
accepted a 4,096-byte paste and ran the search. It refused 4,097 bytes
with the UTF-8 limit message.

A separate SQL result loaded 150,000 rows. Cancel search kept the
dialog open and reported that pending matches were discarded. A
subsequent search selected row 1, column 2 from the same result.
PostgreSQL 16 and MySQL 8.4 repeated the same fixtures and GUI checks.
Both found rows 1, 3, 1 and 3 during forward and reverse wrap, found
the uncommitted value at row 2, column 2, and accepted 4,096 bytes
while refusing 4,097. Independent reads after discarding the edit
returned all four original values. Both 150,000-row result footers
confirmed the loaded count. Cancel left the dialog open, and a new
search selected row 1, column 2.

### Linux multi-key sorting and Criteria history

MySQL 8.4.13 and SQLite ran the same five-row fixture in the packaged
candidate under Debian 13 private Xvfb. In the column menu, Sort
ascending on `bucket` followed by Add descending sort key on `score`
produced IDs `4, 5, 2, 3, 1` in both grids. The primary key resolved
the tie between IDs 4 and 5. Independent MySQL and SQLite queries
returned the same order.

In Criteria, equality on the text `O'Brien` selected IDs 2 and 3.
The MySQL preview used `CONVERT(X'4f27427269656e' USING utf8mb4)`.
SQLite doubled the apostrophe in the quoted literal. Native queries
confirmed the exact result on both providers. Back restored the
five-row result with both sort keys. Forward restored the Criteria
expression and its two rows. PostgreSQL ordering, literal filters
and history are covered in B02 and B03.

### Linux TSV paste limits and empty-value actions

PostgreSQL 16, MySQL 8.4 and SQLite used the same 36-row table with
33 nullable text columns and a primary key on Debian 13 private Xvfb.
Each accepted a 32×32 TSV block and removed the staged block with one
Undo. A second block at exactly 262,144 bytes also staged on each
provider and was undone. The 1,056-cell block and 262,145-byte input
were rejected with the respective cell and byte limit messages.

Pasting four bytes of literal `NULL` and saving stored text on each
provider. An explicit **Set SQL NULL** saved a database NULL. In a
new row with ID 101, **Use database default** changed the previously
pasted text to DEFAULT, and Save stored `native_default`. Independent
reads confirmed the distinct text, NULL and default states and found
no persisted rows from the staged boundary block.

### Linux composite key navigation and picker

PostgreSQL G06 covered both directions, staged both key columns and
verified Cancel with independent read-back. On Debian 13 private Xvfb,
MySQL 8.4 and SQLite used child rows `1 → (A,01)`, `2 → (B,01)` and
`3 → (A,02)`. **Open referenced row** applied both predicates and
returned only `(A,01,A-one)`. **Open referencing rows** applied both
predicates and returned only child ID 1.

**Choose key** opened the parent picker on each provider. Selecting
`(B,02)` staged both columns on child ID 1. Cancel restored `(A,01)`,
and independent reads confirmed all three child rows still had their
original keys. A separate MySQL numeric FK with value 9 showed
`MySQL lookup requires text keys to avoid implicit numeric coercion`
when opening its referenced row. The source tab remained active.

### Linux clipboard formats and output limit

PostgreSQL 16, MySQL 8.4 and SQLite each copied a three-row, two-column
selection as TSV, CSV and SQL INSERT on Debian 13 private Xvfb. All nine
pastes were saved from GNOME Text Editor. An independent X11 clipboard
read matched each file after accounting for the editor's final LF.
TSV matched the expected bytes, and parsed CSV retained Unicode, quotes
and embedded LF, with NULL and empty text both represented as empty
fields. Replaying each saved SQL file in a scratch table restored the
three rows, including distinct SQL NULL and empty text.

Each provider also supplied 18 cells with 1,048,576 bytes each. Copy TSV
reported that the selection exceeded the clipboard size limit. The
previous SQL payload remained in the clipboard and pasted into the editor
again unchanged. At 1800 pixels, the error was readable after hiding the
Navigator, and the footer still clipped the trailing guidance.

### Linux native file dialogs with the portal stopped

The Ubuntu 24.04 candidate DEB ran as a native Wayland client under
Sway nested inside private Xvfb. The SQLite profile, downloads directory,
D-Bus session, GTK portal and data were disposable.

| Control | Portal running and chooser canceled | Portal stopped |
| --- | --- | --- |
| Export CSV and Export SVG | Native Save File opened. Cancel left the downloads directory empty. | Both displayed an export-dialog error. |
| Open, Save and Compare plan snapshots | Native choosers opened. Cancel wrote no file and left the current plan result intact. | All three displayed an error opening the chooser. |
| Import CSV Browse | Native Open File opened. Cancel returned to the wizard with no CSV selected. | No chooser opened. The wizard stayed responsive and left the source empty without displaying any error. |
| PostgreSQL Client Directory and Save Backup As | Both native choosers opened. Cancel preserved executable auto-detection, left the output unset and produced no file. | Both displayed a portal request error in the backup dialog. Run stayed disabled, and Close dismissed it. |

The test stopped both portal processes and confirmed that
`org.freedesktop.portal.Desktop` had no owner on the private bus. A
subsequent FastDB query returned the SQLite fixture value `recovery`.
Independent SQLite read-back matched both original rows, and the
downloads directory remained empty. Relaunching FastDB with the portal
restored allowed the plan chooser to open again.

A separate PostgreSQL 16 fixture confirmed that both backup chooser
attempts left the test row intact. The candidate still has no reachable
SQL copy or Value Editor file controls. Raw evidence is in
`.sdd/native-portal-failure-evidence/`.

### Linux shortcut matrix on private GNOME

GNOME Shell and Mutter 50.1 ran headless with a private D-Bus, native
Wayland and a 1700 × 1000 virtual output. Mutter's test-session remote
input sent the Linux key chords to the packaged FastDB candidate. The
settings backend used defaults and did not access the desktop user's
settings. No GNOME intercept was observed for the exercised chords,
and the installed default window-manager, Shell and media-key lists
had no exact conflicting binding for the documented matrix.

| Shortcut group | Observed |
| --- | --- |
| Run and format | Ctrl+Enter ran the caret statement and later only the selection. The latter displayed 733 while 734 remained unexecuted. Ctrl+Shift+Enter appended both script statements. Ctrl+Shift+F split FROM and WHERE onto separate lines. |
| Movement and selection | Arrows, Home/End and the word, line and script modifiers moved the caret in the two-line fixture. Inserted markers confirmed Alt+Left/Right and Ctrl+arrows in the saved bytes. Shift+Left and Shift+click highlighted text. Select all, Copy, Cut and Paste restored the complete script text. |
| Completion and filter | Tab and Enter accepted the SQLite table completion in separate trials. Escape dismissed the list and retained the typed prefix. Filter Enter reduced IDs 731 and 732 to 731. Escape removed filter focus and preserved its contents and result. |
| Workspace | Ctrl+B hid and restored Navigator. Ctrl+Shift+T changed the theme. Ctrl+=, Ctrl++, Ctrl+- and Ctrl+0 changed editor size and restored its default. Ctrl+W closed the active script and Ctrl+] reopened it with the saved text. Ctrl+Q destroyed the Wayland toplevel and the FastDB process exited 0. |

Screenshots, saved navigation bytes and the default-binding export are
in `.sdd/gnome-shortcuts-evidence/`.

### Linux IBus composition on private Wayland

IBus 1.5.34-rc2 with the Unikey engine ran in a private D-Bus session on
Sway nested in Xvfb. Typing Telex into native GNOME Text Editor produced
`Tiếng Việt ` with exact UTF-8 clipboard bytes. In the FastDB candidate,
the SQL editor and table filter stayed blank, and the staged cell editor
and connection-name field retained their original text. The FastDB client
trace received `zwp_text_input_v3.commit_string` for all four inputs,
including `Tiếng ` and `Việt `. Independent SQLite and the saved
connection metadata retained their initial values.

The CJK and emoji follow-up used the same candidate on a fresh private
Sway display. IBus CangJie 5 composed `日` from A then Space. IBus Unicode
input composed `🎉` from U+1F389. Both committed to native GNOME Text Editor
and its clipboard matched the expected UTF-8 bytes. FastDB received
Wayland commit events for both characters in the SQL editor, table filter,
grid cell editor and connection-name field. SQL and filter stayed empty.
The cell and connection name retained their original text, also confirmed
with independent SQLite and connection-file reads. The separate clipboard
run verified Vietnamese, CJK and emoji paste in SQL and grid text.

Screenshots and commit-event excerpts are in `.sdd/wayland-ime-evidence/`
and `.sdd/wayland-ime-cjk-emoji-evidence/`.

### Linux IBus Unikey composition on private X11

A separate Openbox/Xvfb session used the packaged candidate, an isolated
IBus daemon, and the Unikey engine. The X11 root advertised `@server=ibus`.
Typing the Telex sequence in GNOME Text Editor produced `Tiếng Việt`.
With the FastDB SQL editor focused, the same input terminated FastDB.
The main thread panicked in `zed-xim` 0.4.0-zed while decoding UTF-8.
The X11 fixture covered Vietnamese in the SQL editor. The other fields,
CJK and emoji were exercised separately on native Wayland. Screenshots
and the panic excerpt are in `.sdd/x11-ibus-composition-evidence/`.

### Linux clipboard round trips on private X11 and Wayland

FastDB and GNOME Text Editor ran as native Wayland clients on a private
Sway compositor nested in Xvfb, with Xwayland disabled. SQL text and a
SQLite grid cell pasted into the external editor and copied back with
identical UTF-8 bytes, including decomposed Vietnamese marks, CJK and
emoji. Independent SQLite supplied the cell's expected bytes.

Two pointer-only clicks on Copy DDL left the previous grid text on the
clipboard. Copy record likewise left the preceding DDL text. The Wayland
trace showed both click handlers submitting the latest keyboard serial
instead of the newer pointer serial. Pressing Right in FastDB before
clicking allowed both actions to update the clipboard. The DDL pasted
into the editor and replayed into a scratch SQLite database with identical
schema. The record pasted with the expected headers, ID and Unicode label.
Raw evidence is in `.sdd/wayland-clipboard-evidence/`.

On a separate Openbox/Xvfb display, the 49-byte SQL statement with
decomposed Vietnamese marks, CJK and emoji went from GNOME Text Editor
to FastDB and back unchanged. Pointer-only Copy DDL and Copy record
both updated the clipboard. GNOME Text Editor pasted and copied back
the exact 66-byte DDL and 118-byte record. An independent SQLite session
ran the copied SQL, matched the replayed DDL against the original schema
and read the ID and Unicode label shown in the record. The prior X11
run covered nine additional grid-format pastes. Raw X11 evidence is
in `.sdd/x11-clipboard-roundtrip-evidence/`.

### Linux table transactions and failed-batch recovery

MySQL 8.4 and SQLite used a three-row fixture with a nonnegative value
constraint on Debian 13 private Xvfb. Independent clients read values
in primary-key order. Auto Save changed `(10,20,30)` to `(11,20,30)`
immediately. Manual Save displayed `(12,20,30)` in the table while the
independent client still read `(11,20,30)`. A later batch staged
`(12,21,-1)` and hit the constraint. Both failed drafts were still
visible. Cancel and Commit persisted only the earlier Manual Save
as `(12,20,30)`.

In Manual mode, Rollback removed a saved update and a separate draft,
restoring `(12,20,30)`. Commit also applied an edit that had only been
staged, yielding `(12,23,30)`. Auto rejected a two-row batch with an
invalid value and preserved `(12,23,30)`. After Cancel, a valid Auto
Save committed `(19,23,30)` independently on each provider.

On MySQL, choosing Auto-Commit after a successful toolbar Rollback
twice showed `Commit or Rollback before enabling autocommit` and left
the table in Manual mode. Closing and reopening the table returned it
to Auto. SQLite switched to Auto in the same tab. PostgreSQL is covered
by W03.

### Linux export formats, cancellation and write failure

On private Xvfb with a GTK file portal, PostgreSQL 16, MySQL 8.4 and
SQLite each exported the same three rows through loaded CSV, SQL,
JSON, HTML and Markdown and through Export All CSV and SQL. Independent
parsers validated the 21 files, including Unicode and quotes in the
fixture. Loaded CSV represented SQL NULL and empty text with an empty
field. Full-source CSV used the unquoted `\N` marker for SQL NULL.
Replaying each of the six SQL files with its native engine restored the
three rows and preserved the distinction between NULL and empty text.

Escape dismissed the GTK Export All chooser on each provider. With a
pre-existing destination accepted in GTK, **Cancel export** stopped
MySQL Export All SQL while a separate session held a table write lock
and SQLite Export All CSV while a separate client held an exclusive
database lock. Both footers reported that the destination was unchanged.
The destination bytes matched, and neither run left a temporary output.
PostgreSQL's running Cancel and byte comparison are recorded in X05.

A temporary 64 KiB per-process file limit provoked `File too large
(os error 27)` after accepting replacement of an existing file. The
GUI reported the error on PostgreSQL Export All CSV, MySQL Export All
SQL and SQLite loaded CSV. All three original files matched byte for
byte, with no temporary outputs left in the destination directory.

### Linux CSV import formats, commit policies and cancellation

On private Xvfb with the GTK file chooser, PostgreSQL 16, MySQL 8.4
and SQLite imported UTF-16 BE with semicolon delimiters, UTF-16 LE
with tabs, UTF-8 with pipes, and headerless UTF-8 with commas. The
headerless file required manually mapping Col 1 and Col 2. Independent
reads matched the exact Unicode, quoted punctuation, embedded LF, SQL
NULL and quoted empty text. Existing PostgreSQL X01 and X06 also cover
UTF-8 with headers, a cleared NULL token and the default for a skipped
column.

On each provider, explicit Integer conversion rejected the invalid ID
with Skip off. With Skip on, the wizard reported two processed, one
rejected and one committed. A duplicate ID in the second batch left
zero newly committed rows under Atomic Commit and exactly IDs
1001–1256 under Per-Batch Commit. The read-back matched the accepted
conversion row, the surviving batch and the original fixture rows.

Escape cancelled an Atomic import waiting on a separate database lock
on each provider. The wizard reported zero committed and independent
clients confirmed zero new rows. This covers lock-wait cancellation.
The PostgreSQL X02 run still records a cleanup timeout for both
policies when cancellation occurs during a delayed insert in batch
two, with the Per-Batch statement still running after the GUI outcome.
The follow-up used a 350-row CSV on the packaged candidate under private
Sway nested in Xvfb. Both destinations had an insert trigger at ID 257.
On MySQL, independent process-list output showed the trigger sleeping
and a separate read saw exactly the first 256 rows before Escape. FastDB
reported 256 committed. The sleeping session disappeared, and a later
read still found exactly IDs 1–256 and the expected labels.

On SQLite, an independent WAL reader saw IDs 1–256 committed while a
second writer probe was refused by the active writer lock. FastDB then
received Escape and reported 256 committed. An independent write probe
could acquire the lock after the outcome, and read-back confirmed that
only IDs 1–256 and their exact labels survived. Neither dialog showed a
cleanup timeout. Raw evidence is in
`.sdd/grid-import-batch-cancellation-evidence/`.

### Linux large grid streaming and resident memory

The retained 0.3.0 candidate opened a fresh FastDB process on private
Xvfb for each workload. The PostgreSQL 250,000-row table used the
FX2 `lab.large_results` definition and seed. A separate million-row
table used the same seven columns and value generator. SQLite used a
seven-column counterpart with notes up to 272 bytes. Independent
queries matched the grid counts and confirmed IDs 1–250,000 and
1–1,000,000, respectively. Each grid reached Idle.

RSS comes from FastDB's `/proc/<pid>/status`, sampled every quarter
second for 80 samples around each query. Figures below are KiB,
except for spill file size in bytes.

| Provider | Rows | Initial RSS | Peak sampled RSS | Increase | Spill bytes |
| --- | ---: | ---: | ---: | ---: | ---: |
| PostgreSQL | 250,000 | 175,900 | 242,904 | 67,004 | 0 |
| PostgreSQL | 1,000,000 | 154,076 | 269,908 | 115,832 | 44,854,948 |
| SQLite | 250,000 | 160,184 | 237,112 | 76,928 | 1,721,419 |
| SQLite | 1,000,000 | 154,268 | 234,912 | 80,644 | 29,058,172 |

Four times the rows increased query RSS growth by about 1.7× on
PostgreSQL and 1.05× on SQLite, with cold pages spilling to disk in
both million-row runs. Closing the PostgreSQL million-row tab removed
its spill file. Quitting SQLite with its million-row tab open removed
that spill file. This check measures the stated grid workloads under
Xvfb.

### Linux spill directory permissions for a second user

The GRID-01 run observed nonempty files under `/var/tmp/fastdb` during
million-row results and verified their removal on tab close and Quit.
In a separate Debian 13 container with private Xvfb and no pre-existing
spill directory, FastDB running as UID 1000 created it with owner and
group 1000 and mode 0755. That FastDB process then quit. UID 1001
launched the same binary with its own profile and runtime directory.
FastDB exited before opening a window, reporting permission denied
while creating `fastdb-sql-session-*.jsonl` in the shared spill directory.

### Linux crash restart and temporary file recovery

With an autosaved SQLite script showing a million-row result in
private Xvfb, the test sent SIGKILL to FastDB only. The terminated
process left a 4,930-byte transaction log and a 29,058,172-byte
spill file, both unchanged from before the kill. Relaunching with
the same profile removed both abandoned files and created a fresh
transaction log. All script hashes matched and the original SQL
reappeared in the restored Script-1 tab. A normal Quit removed
the new log.

### Linux empty-profile startup

The retained 0.3.0 candidate launched with an empty profile on Debian 13
in private 2000×1000 Xvfb. The Openbox frame was centered and FastDB's
client area measured exactly 1280×800. The workspace had no tabs or
connections, showed Add Connection and displayed no error banner.

The first launch in this disposable environment mapped the client after
410 ms. A pixel in the workspace first became nonblack after 2,874 ms,
above the 2,000 ms budget. The probe used a monotonic clock and polled
the mapped window from the same container. This is a software-rendered
Xvfb observation. The physical-display cold-start gate remains open.

A final diagnostic ran three fresh profiles on private Openbox/Xvfb.
An Xlib probe polled the screen center directly on a monotonic clock.
The first nonblack workspace sample appeared after 1,256 ms, 176 ms
and 209 ms. All client areas measured 1280 × 800 and the captures
showed an empty workspace without a banner. These samples are from
one container without a host reboot. They do not resolve the prior
2,874 ms observation or the dedicated cold-start gate.
Evidence is in `.sdd/final-linux-shell-evidence/`.

### Linux theme, editor zoom and sidebar persistence

In the SQLite script editor on private Xvfb, Zoom In twice changed
13 to 15 px, Zoom Out changed it to 14 px and Reset restored 13 px.
The editor rendered each size change. A second Zoom In and the Light
theme survived Quit and relaunch. Theme toggled in both directions.

Ctrl+B hid the Navigator and saved its width as 0. On relaunch, the
Navigator was visible at 160 px with the same profile, even though
the saved width was still 0. The startup restore path clamps the
stored width to the 160–800 px resize range.

### Linux pane, tab and column state after restart

The packaged candidate ran on private Xvfb with two SQLite profiles
whose tables shared a name. Dragging the pane dividers saved a 390-pixel
Navigator and a 488-pixel Value panel. Both widths and the open Value
panel restored. The table tab and two tabs named Script-1 kept their
profile IDs and order. Both scripts retained exact bytes. Manually rerunning
them returned 731 for Layout A and 732 for Layout B, matching independent
SQLite reads.

Moving Note left and hiding Label left visible columns ID and Note.
After Quit, relaunch, reconnect and Refresh, the table showed its original
ID, Label, Note order with Label visible. Column order and visibility
therefore did not persist. The separate timeout test found no reachable
preset, so preset lifetime could not be exercised. Evidence is in
`.sdd/layout-restart-evidence/`.

### Linux title bar and tab clicks

In private Openbox X11, double-clicking the in-app title and the empty
part of the same bar alternated between 1280×800 and the maximized
2000×981 client area. Selecting Script-1 and closing Script-2 kept the
window maximized and left Script-1 open.

In private Sway 1.10.1 nested inside Xvfb, Xwayland was disabled and
Sway reported FastDB as an `xdg_shell` client. On a floating 800×550
FastDB window, double-clicking the in-app title or the blank area
left both the dimensions and position unchanged. Single clicks
selected the correct tab and closed Script-2 without affecting
geometry, and a Navigator double-click opened the SQLite connection.
UPD-03 verified the update badge under X11 with the release override.
A follow-up with the same simulated v0.9.0 on native Wayland dismissed
the initial release notes, clicked the badge twice with Escape between
clicks, and reopened the notes both times. The private Sway tree reported
the same `xdg_shell` rectangle at every step, position (360, 100) and
size 1280 × 800. Evidence is in `.sdd/wayland-updater-badge-evidence/`.

### Linux HiDPI grid and diagram

The release candidate ran on a private 3000×1800 Xvfb with the GPUI
X11 scale set to 1 and then 2. The same nominal window measured
1280×800 and 2560×1600 device pixels, respectively. Text, including
Vietnamese accents, was readable in both captures. The grid's 26-pixel
row spacing became 52 pixels and the column lines stayed aligned.

The PostgreSQL, MySQL and SQLite provider logos rendered in their
picker cards at both scales. The SQLite diagram displayed the same
two tables and one foreign-key arrow from `child.parent_id` to
`parent.id` at both scales. Card headings and fields stayed within
their borders.

### Linux plain-text font fallback

With the default DejaVu Sans Mono on Debian 13, all five `END` markers
in the SQLite plain-text result began at the same device-pixel column.
The preceding cells included equal-length strings with different glyph
widths, accented Vietnamese and SQL NULL.

After removing `fonts-dejavu-core` and `fonts-dejavu-mono` in the
disposable container, Fontconfig reported Nimbus Mono PS as the
generic monospace match. FastDB exited at startup because GPUI could
not resolve a UI font. Adding Noto Sans let the same binary open and
render the result while the DejaVu packages remained absent. The first
visible pixel of `END` ranged from column 455 to 487 across the five
rows, confirming that FastDB rendered the result with proportional
spacing despite the available monospace family.

### Linux Quit guards and save recovery

On private Openbox Xvfb, Ctrl+Q and the window manager's Close both
kept FastDB open with a staged SQLite grid edit and displayed the
Save or Cancel message. A separate SQLite connection still read
`original`. Grid Cancel cleared the edit. The earlier L03 result
also records that the guard banner can remain after Cancel.

A fixture directory at the expected `connections.json` path forced
a connection name save to fail. The error and **Retry saving
connections** were visible, and both departure routes kept FastDB
open. Restoring the original file and clicking Retry saved the new
name, cleared the save error and let the window close.

For a dirty PostgreSQL column rename, both departure routes showed
Save, Discard and Cancel. Cancel kept the draft, then Discard allowed
the window to close. Independent psql confirmed the original column
name and value. In a separate run, closing the window immediately
after typing a script preserved its exact SQL for relaunch, matching
the shortcut behavior recorded in L04.

The tested Linux window exposed no application menu for Quit.
F10 did not open an application menu, consistent with APP-04.

In the final empty-profile smoke, the window manager close action exited
with code 0 once and Ctrl+Q exited with code 0 twice. Each run removed
its own mode-0600 session log while Xvfb stayed available. The app-menu
Quit failure remains recorded. Evidence is in
`.sdd/final-linux-shell-evidence/`.

### Linux Secret Service restart, lock and save retry

FastDB ran on private Sway nested in Xvfb, with its own D-Bus and
GNOME Keyring data. The disposable PostgreSQL server required SCRAM
over TCP. With no Secret Service owner and activation disabled, saving
a password displayed the credential-storage error and Retry control.
Ctrl+Q left the window open and the connection metadata stayed
byte-identical. Starting GNOME Keyring on that bus and clicking Retry
cleared the banner and allowed Quit. D-Bus reported one FastDB item.
The connection file recorded the keyring reference and no password.

After a FastDB restart, opening the saved profile connected without
re-entry. An independent PostgreSQL session verified the fixture user,
database and TCP connection. After another Quit, the private bus
confirmed that the login collection and item were locked. On relaunch,
FastDB logged that the unlock prompt was dismissed. Opening the profile
showed an empty password field, PostgreSQL reported zero matching TCP
sessions, and the locked item and its reference remained. Evidence is
in `.sdd/native-credential-evidence/`.

A follow-up on the packaged Ubuntu 24.04 candidate used a separate
Openbox/Xvfb session, private D-Bus, GNOME Keyring 46.1 and Gcr 3.41.2.
FastDB migrated the disposable profile's password to the login keyring.
After Quit, a Secret Service Lock call marked the collection and its
one item locked. Relaunch raised **Unlock Login Keyring**. A wrong unlock
password displayed an error, the item stayed locked, and PostgreSQL
showed no FastDB session. The correct unlock password closed the prompt
and cleared the locked flags without replacing the keyring item.

Opening the saved profile connected without re-entering the database
password. Independent PostgreSQL 16.16 queries saw one non-loopback TCP
session for the expected role, database and application name, covered
by the server's SCRAM rule. The saved profile still held only the
keyring reference. After Quit the session count returned to zero.
Evidence is in `.sdd/native-keyring-unlock-evidence/`.
[Secret Service prompt specification](https://specifications.freedesktop.org/secret-service/latest/prompts.html)

### Linux local and native credential isolation

The release candidate connected to a disposable PostgreSQL 16 server
requiring SCRAM over TCP. With `FASTDB_LOCAL_CREDENTIALS=1`, FastDB
stored one credential under the profile's `local-credentials` tree.
Both directory levels had mode 0700, the file had mode 0600, and
`connections.json` retained only the credential reference. After
Quit and relaunch, the saved profile reconnected without re-entry
and returned the fixture user over TCP.

With the same profile opened in native mode, the password field was
empty and an independent D-Bus search found no FastDB item in the
isolated GNOME Keyring. Saving a different fixture credential
explicitly in native mode created one Secret Service entry and kept
the local file byte-identical. Returning to local mode again
authenticated to the original server without re-entry. The local
file still contained the original fixture credential and neither
value was present in the connection metadata.

### Linux legacy plaintext credential migration

Two independent fixture profiles contained a plaintext PostgreSQL
password. One used the managed `connections.json` format and the
other used a working-directory `.fastdb_config.json`. With native
storage selected in a private D-Bus session without Secret Service,
FastDB logged the migration failure and kept both original source
files byte-identical. It did not create a managed config for the
working-directory case.

After relaunch with an available isolated local store, both cases
created a credential file matching the fixture and left only the
reference in the managed config. The obsolete working-directory
file was removed. Both migrated profiles authenticated over TCP to
the disposable PostgreSQL server from the Navigator without
password re-entry.

### Linux connection deletion and diagram cleanup

Two saved PostgreSQL profiles connected over SCRAM in private Xvfb.
Each had a local database credential and its own saved position for
the same parent card. The two positions were created by dragging
the card in each connection's diagram and verified to differ.

Deleting the first profile through the Navigator removed its metadata,
credential file and entire diagram-position directory. The other
profile's credential and position file matched their pre-delete bytes.
After relaunch, only the surviving profile appeared. It authenticated
without password re-entry and restored its moved card and foreign-key
arrow. The deletion test used database credentials in local mode.

### Linux MySQL connection deadlines without a responsive peer

The GUI tested MySQL at the reserved address `192.0.2.250` in two
isolated network namespaces. With no route, an independent socket
returned `ENETUNREACH` immediately and FastDB displayed the same
error within 440 ms of clicking Test Connection. The kernel error
ended the attempt before the configured 2-second deadline.

On an internal network with an on-link route and no neighbor at the
target address, an independent socket waited rather than returning
a routing error. FastDB displayed `MySQL connection timed out` at
1,469 ms and 2,374 ms from the X11 click for the configured 1- and
2-second deadlines, respectively. Those observations include GUI
dispatch and paint. The earlier stalled-handshake run measured
peer closure independently at 1,001.1 and 2,001.4 ms.

### Linux Unix socket discovery and TLS override

In private Xvfb, the candidate discovered PostgreSQL's socket at
`/var/run/postgresql/.s.PGSQL.5432` but returned `Not a directory`
when connecting. An independent psql session through that directory
confirmed Unix transport and no TLS. The candidate passes the full
socket filename to tokio-postgres, which appends its own
`.s.PGSQL.<port>` component.

MySQL connected through `/var/run/mysqld/mysqld.sock` and its
server-side session reported `Socket`. Both fixture sockets passed
the native-client checks. After enabling TLS in the connection
dialogs with the fixture CA, both FastDB profiles kept socket
discovery enabled in saved settings and connected over TLS instead.
PostgreSQL reported a non-NULL client address and `ssl=true`.
MySQL reported `SSL/TLS`. Both sessions returned an active cipher.

### Linux TLS trust and refusal matrix

The release candidate's Test Connection action ran against PostgreSQL
16 and MySQL 8.4 on private Xvfb. Both accepted the same fixture CA
as an explicit PEM file and as an explicit DER file. With the CA
field empty, both rejected the private issuer until the CA was
installed in the disposable Debian system trust store. After a fresh
launch, the same profiles with empty CA fields passed.

A separate hostname resolved to the same loopback address. With an
explicit trusted CA, both drivers rejected that name because it was
outside the server certificate's SANs. Both drivers also refused their
plaintext-only fixture ports when TLS was enabled. Independent clients
connected to those ports with TLS disabled and confirmed there was no
active cipher. CON-03 records the active TLS ciphers through the GUI.

### Linux startup update check interval

On private Xvfb with no network, the release candidate's first
start showed simulated v0.9.0 and persisted the check timestamp.
A second start with a different simulated version left the
timestamp unchanged and showed neither an update dialog nor a badge.
The same suppression held when the saved timestamp was seeded at
23 hours and 58 minutes 50 seconds before the probe.

With the saved timestamp seeded 24 hours and 5 seconds in the past,
startup replaced it and displayed simulated v0.8.0. Each comparison
used the same isolated profile across normal Quit and relaunch.

### Linux simulated update badge and notes

With `FASTDB_OVERRIDE_LATEST_VERSION=0.9.0` in a fresh private Xvfb
profile, FastDB 0.3.0 displayed the v0.9.0 update modal and release
notes. Closing the modal left the **Update (v0.9.0)** badge in the
tab row. Clicking the badge reopened the same notes, and the Openbox
window retained its maximized state and 2000×881 client area.

The simulation offered `FastDB-0.9.0-arm64.dmg` (40.5 MB) and
enabled Upgrade on Linux. This reflects the override's test asset.
The test stopped at the modal without downloading or installing it.

### Linux updater API fallback

The packaged FastDB ran without the release override on private
Xvfb in a network-isolated container. A trusted HTTPS fixture on
loopback returned HTTP 403 to the releases API request. The fixture
log recorded the next request as HEAD on the latest release page,
followed by HEAD on its v0.9.0 tag after the HTTP 302 redirect.

FastDB displayed v0.9.0 with a notice that the API did not supply
release details. The modal offered **View on GitHub** and **Close**,
and reported no matching prebuilt installer. No request left the
isolated fixture.

### Linux manual update check entry

The private HTTPS release fixture identified v0.3.0 as current,
matching the candidate. Startup reached the v0.3.0 endpoint and
left the workspace without a badge or modal. No Check for Updates
menu item was reachable in the Linux window. F10 and F1 left the
workspace and fixture request log unchanged. The manual up-to-date
confirmation remains unverified because the entry point is absent.

### Linux PKCS#12 client identity

On private Xvfb, PostgreSQL 16 required a valid client certificate
for the fixture role and MySQL 8.4 required X509 for the fixture account.
Both rejected a connection without a client identity. Both accepted
a password-protected `.p12` archive and an empty-password `.pfx`
archive. A wrong archive password and a certificate-only archive
produced the same local error asking to check the PKCS#12 archive,
private key and password.

After saving the protected archive and relaunching, both profiles
reconnected without re-entry. PostgreSQL returned SSL enabled and
`/CN=fastdb_tls_client` as the session client DN. MySQL returned a
nonempty session cipher on its X509-required account. In both
profiles, clearing the archive path and password made Test Connection
fail again. Save removed the identity metadata and the corresponding
local credential file. The plaintext archive password never appeared
in the saved connection metadata.

### Linux SSH authentication and database TLS

On private Xvfb, PostgreSQL 16 and MySQL 8.4 each passed Test
Connection over the dedicated OpenSSH fixture using a password and
again using an encrypted ED25519 key with its passphrase. OpenSSH
logged two accepted password authentications and eight accepted
public-key authentications across the Test, Connect and SQL paths.
The SSH listener was loopback-only, with forwarding limited to the two
fixture databases. No host port was published.

On both providers an unrelated host key fingerprint was refused
with the explicit fingerprint message. After restoring the valid
pin, changing only the database host from `localhost` to
`127.0.0.1` failed certificate verification because the certificate
had only the localhost DNS SAN. Restoring the database hostname
allowed Connect and a SQL query. PostgreSQL reported TLS 1.3 and
both providers reported the `TLS_AES_256_GCM_SHA384` session cipher.

### Linux editable PostgreSQL results and atomic conflict

On private Xvfb, **Load editable** accepted a direct SELECT of a
permanent PostgreSQL 16 table with a primary key. The grid allowed
edits to integer, text, varchar, Boolean and UUID fields. A single
Save committed all five changes and psql read the exact values.
The primary-key and numeric columns independently showed the
read-only message when opened for editing.

After staging edits to IDs 1 and 2, a separate psql session changed
ID 1. Save reported that no edits were saved and retained both
drafts. Independent psql still read the external value on ID 1 and
the original value on ID 2. Cancel and Load editable displayed that
same database state. The script status still showed the previous
successful Save while the current result showed the conflict banner.

The editor rejected a view, join, expression projection, WHERE
clause, RLS table, partitioned table, keyless table and table with
a user trigger. The direct-query restrictions and table eligibility
restrictions appeared as distinct error messages.

### Linux table Properties capability states

Private Xvfb opened table Properties on PostgreSQL 16, MySQL 8.4
and SQLite. All three showed the fixture's two columns while
connected and a separate Disconnected message when the owner
session was absent. MySQL and SQLite showed a Not implemented
message for Triggers and an Unsupported message for Policies.
SQLite also marked Partitions unsupported. The Virtual category
remained marked pending.

A PostgreSQL role with schema USAGE and no table privileges initially
had no visible schema in Navigator. A temporary SELECT grant exposed
the table and its Policies section showed that the reader had loaded
zero objects. With the tab still open, the test revoked table SELECT
and access to the policy catalog in the disposable database. Reload
then displayed Permission denied. Independent psql under the same
role confirmed SQLSTATE `42501` from `pg_policy`. Disconnect
replaced that prior error with the Disconnected state.

### Linux value viewer and editor entry

The PostgreSQL V01 replay on the retained candidate showed SQL NULL, exact
JSON including duplicate keys and a large integer, unchanged XML, and hex
output distinguishing nonempty bytes, empty bytes and SQL NULL. Two source
timestamp offsets represented the same instant in PostgreSQL and in the viewer.

V02 reached a grid message directing binary edits to Value Editor. The grid
context menu, footer and Value panel exposed no way to open it. A call-site
search on the acceptance branch found only the shared `open_value_editor`
definition. This is a failed GUI entry for the provider-independent editor.
The 256 KiB editor boundary, UTF and binary file round trips, invalid encoding,
UTC toggle and stale Apply remain unverified through the editor.

The follow-up used MySQL 8.4 and SQLite on the same packaged candidate in
private Xvfb. Both viewers showed XML source unchanged, displayed
`00 01 7f ff`, and distinguished a blank zero-byte BLOB from `[NULL]`.
The formatted SQLite JSON retained the exact 20-digit integer and both
duplicate keys. MySQL exposed a distinct 16-digit integer and the one key
retained in its native JSON value. The X11 clipboard values for JSON,
XML and hex matched independent provider read-back. At 262,144 bytes,
Copy Value returned the exact fixture text from both viewers. At 262,145
bytes, both showed the 256 KiB limit. Raw evidence is in
`.sdd/value-panel-provider-evidence/`.

## Linux Administration and Session Monitor retest

On `9d224ed`, the existing driver test binaries passed all five
`administration_test` and `session_monitor_test` cases with a disposable
PostgreSQL 16.16 fixture required. The monitor verified the blocking chain,
selected-backend signaling and native privilege restrictions. Administration
verified catalog mutations, rollback, and enforced read-only policy. Ten
app-level checks also exercised review, owner replacement, departure,
target identity, bounded blocking edges and refresh selection. The app
logging case initially skipped its live part. It passed in a separate run
with PostgreSQL required, verifying that Administration query text stayed
out of debug output.

On the Ubuntu 24.04 DEB under private Xvfb, both Session Monitor review
panels were dismissed with Go back. Independent PostgreSQL still reported
`PgSleep` for PID 229 after dismissing Cancel and for PID 231 after
dismissing Terminate. Confirming Cancel sent a signal to PID 229 and the
one-shot client exited. The monitor refreshed automatically, and PID 231
continued its original statement. Confirming Terminate sent the signal
to PID 231 and the refreshed view dropped that row. Independent PostgreSQL
confirmed the backend was gone and retained `2|second`, the committed value
before the terminated session's pending update. The read-only B profile
inspected PID 260 and its running statement. Cancel and Terminate were
disabled, attempts to click them opened no confirmation, and independent
PostgreSQL still reported the same PID in `PgSleep`.

### Administration catalog, reviewed changes and rollback

The Ubuntu 24.04 DEB on private Xvfb used PostgreSQL 16.16 and the
disposable `qa_admin` schema. A second native client supplied the read-back.

| Catalog family | Reviewed GUI operation and independent result |
| --- | --- |
| Routines | Read `alpha()` source and reviewed Rename. Go back left the original function returning 7. Confirm changed the catalog name to `alpha_renamed()`, which returned the same value. Run with arguments generated `NULL::text` for `echo`. The edited invocation returned `qa-admin-typed`, matching native psql. |
| Sequences | Page 1 began with the fixture sequence and `counter`. With 66 total sequences, page 2 began at `zpage_049`. Refresh returned to page 1. Native details showed the start value of 7. Reviewed Restart with 87 produced `87\|f`. |
| Triggers | Inspected the owning table and trigger source. Reviewed Disable changed `qa_alpha` to `tgenabled = D`. |
| Rules | Inspected rule source. Reviewed Disable changed `qa_alpha_rule` to `ev_enabled = D`. |
| Policies | Inspected the policy expression. Reviewed Alter changed the native predicate to `(id >= 0)`. |
| Partitions | Inspected the parent and initial bound. Reviewed Detach cleared `relispartition`. Attach restored the bound `[0,10)`. Create added a second partition at `[10,20)`. |
| Roles | Browsed role characteristics and membership. Reviewed Grant changed `pg_has_role` for the disposable member and role from false to true. |
| Privileges | Inspected the relation owner and explicit ACL. Reviewed Grant changed the fixture role's INSERT privilege from false to true. |

The catalog supplied family selection, paging and Refresh. The plan's
catalog filtering step failed because the dialog had no object filter.
An invalid statement surfaced PostgreSQL's syntax error and disabled
Review until Refresh. After refreshing, reviewed SQL returned 902.
A restricted account received `must be owner of table small`, and psql
confirmed the trigger remained disabled. Both errors displayed
`Cancellation request sent` from failure cleanup even though the user
did not click Cancel. The read-only profile browsed the catalog without
a Review action and the sequence value remained 87.

For active cancellation, independent PostgreSQL showed backend 134
holding a granted RowExclusiveLock on the target while the reviewed
routine slept after INSERT. Cancel operation closed that backend and
the inserted row remained absent. Reopening Administration after
connecting profile B to a separate database returned `fastdb_admin_b|2206`
and matched native read-back. The A fixture independently returned
`fastdb_accept_a|2205`. The existing application test separately exercises
the requirement to reject review execution after the driver is replaced.

### Session Monitor advisory-lock chain

On the same packaged candidate, Escape closed the monitor opened with
Navigator search focused. After reopen and Refresh, selecting C showed
193 → 188 → 183, its active lock SQL and the advisory wait, matching
independent PostgreSQL. Go back preserved B's original wait and C's
dependency. Confirm Cancel returned SQLSTATE `57014` to B, while backend
188 remained connected and C still waited for it. Go back from Terminate
also preserved B. Confirm Terminate returned `57P01` to B, removed only
188 from the refreshed snapshot, and let C acquire the lock. Backend
183 stayed connected. The read-only monitor refreshed and disabled
both actions. The restricted profile showed `Details unavailable` for
a selected session owned by a different role. At scale 1 the restricted
profile's longest row label crossed into the wait detail column.

## PostgreSQL browse, editor and preferences on private Xvfb

The retained Ubuntu 24.04 candidate DEB (SHA-256
`23e7a07a81a58dd45f638ec0bbe57dfc5bb384ffbe69cfec6a0829a207823a77`)
ran with Openbox and software Vulkan in disposable containers. PostgreSQL
16.16 used a separate fixture for each app profile. The displays were
container-local. Both initial delegated runs stopped at the usage limit
after setup. The following observations came from continuing on their
isolated displays and a separate preferences fixture.

| Case | Observed |
| --- | --- |
| C01 | At Xvfb scales 1 and 2, PostgreSQL, MySQL and SQLite were selectable. Db2, MariaDB, Oracle and SQL Server displayed `Feature is coming` and left the selected provider unchanged. The PostgreSQL connection name `c01-restart-proof` remained after Back and Next on scale 2. |
| C03 | In the connection dialog, timeout 0 and 121 produced the 1–120 validation error. Nonnumeric keystrokes were ignored, with the empty field displaying the default 10. A saved and reconnected profile retained its edited name, 17-second timeout and application name. The A script and independent `pg_stat_activity` reported `fastdb-c03-app-a` on `fastdb_accept_a`. |
| C04 | After Ctrl+Q and relaunch with the same settings directory, both PostgreSQL profiles and their script tabs reappeared. The scripts returned `fastdb_accept_a` on backend 159 and `fastdb_accept_b` on backend 162, and independent `pg_stat_activity` confirmed both. The isolated run used `FASTDB_LOCAL_CREDENTIALS=1` and showed no credential prompts. The legacy disabled MariaDB profile was exercised in the earlier run. |
| L01 | While A's script was sleeping on backend 79, the B script retained its existing result and connection. Cancel from B produced an error on A alone. B ran a later query on the same backend 82 with its own database and result marker. A ran again on backend 88 with its own database and marker. |
| L02 | PostgreSQL confirmed A was sleeping before explicit Cancel. The old A backend closed, and B kept backend 82. A second query was independently observed waiting before Disconnect from B. A had zero sessions immediately after disconnect and beyond the original 35-second deadline. B still owned backend 82. Previous runs covered close and reopen of a loading tab. |
| L03 | Disconnect with a staged grid edit showed Save or Cancel on A. Cancel removed the draft, and the retry disconnected and reconnected to A. The old guard banner stayed visible with a clean grid after reconnect until Refresh cleared it. PostgreSQL still showed IDs 1 and 2 with their original values. |
| M04 | Navigator grouped `items`, `items_view` and `items_materialized` into their respective folders. Native DDL was opened for the table, view and materialized view. The catalog browser showed sequence settings, native function and trigger definitions, and enum labels. The sequence favorite survived a clean quit and opened its settings after relaunch. An external rename was verified by PostgreSQL. Refresh listed the new name and opening the old favorite displayed `Favorite no longer exists; refresh the catalog`. Search found the table, six `note` columns and the qualified view. With the unrelated `note` filter cleared, Reveal selected `items_view` in the owning PostgreSQL A tree. |
| R03 | After the read-only profile checks, the normal A profile accepted an UPDATE and returned its new value. Independent PostgreSQL confirmed the write. |
| B02 | Criteria generated `id ASC`. The ordered, NULL, empty, quote, AND and OR cases displayed IDs `1,2,3`, `1`, `2`, `3`, `3` and `1,3` respectively. Independent psql returned the same ID lists. |
| B01 | With explicit `ORDER BY id`, fetch size 20 showed IDs 1–20. Fetch Next loaded 40 and scrolling loaded 60. A subsequent 1,000-row fetch scrolled to ID 1000 and cleared the more-rows indicator. Drag changed the Name column width and double-click fit it again. Grid → Text → Grid worked. At 1280 pixels and scale 1, the multiline note for ID 3 was cut at the row bounds. The entire LF-containing cell copied correctly, with UTF-8 hex `636f6d6d612c2071756f7465202220616e640a6e6577206c696e65` in both clipboard and PostgreSQL. Independent psql read 1,000 rows with IDs 1–1000. |
| B03 | Back and Forward restored SQL, order and rows. A follow-up at fetch sizes 10 and 20 restored IDs `1,2,3` ascending and `10,9,8` descending, respectively, with matching footer sizes and independent psql results. Changing size had created an intermediate history entry, so the two saved endpoints were two steps apart. Criteria Cancel preserved the active filter. Branching to `id = 2` left Forward disabled. |
| B04 | Expand hid Navigator and Value on both items and editable. Escape restored both panels and the active `id = 3` filter on items. Restore workspace restored both panel widths and the selected value on editable. The items filter was cleared before the editable check. |
| M01 | At scale 2, Columns, Constraints, Foreign Keys, Indexes, Statistics and native DDL showed the fixture types, defaults, nullability and key definitions. Statistics explicitly labeled the 1,000 rows as estimated. Copy column details preserved the exact Unicode and LF in the comment. Copy DDL omitted line numbers and replayed through the last owner statement in a disposable schema. PostgreSQL compared all 12 catalog columns exactly and reported five constraints including two FKs and three indexes. Horizontal scroll exposed the comment field at 860 logical pixels, where the note row used two lines and its first line was clipped. The full comment was available through Copy. |
| M02 | Enter and double-click both opened `items.note`. Data for `empty_items` had zero rows, with psql count 0. Virtual explained that keys and relationships were unimplemented. A disposable table's identity and generated-expression editors showed read-only status with no Save action. PostgreSQL retained `id` as identity and `doubled` as generated after an edit attempt. The generated column's Refresh reported a dropped-column or identity error after the fixture table was dropped in psql. |
| M03 | Items showed the composite parent dependency and incoming reference. Open relation opened `release_other.incoming` and displayed ID 1 linked to item 1. The view showed its dependency on items and its internal rewrite rule as a reference. Reload preserved the selected view. The parent and child partition panels showed both directions and the exact 2026 bound. Guarded showed the fixture trigger, rule, policy and owner permissions. Empty items reported triggers loaded with no objects. Psql confirmed the bound, trigger, rule, policy and empty trigger list. |
| M05 | The composite connector highlighted both mapped column pairs and the arrows pointed toward the referenced columns. The self and incoming links remained distinct. Cards displayed names without type or PK/FK badges. Middle drag panned the enlarged graph, zoom reached 184%, Reset returned to 100%, and Focus and Fit repositioned it. The earlier SVG export verified the same graph. |
| M06 | Dragging the parents header and an items column row moved both cards. The adjusted offsets survived closing and reopening the items tab and a zoom round trip back to 100%. The independent B database retained default positions. Double-click opened different parents tabs on A and B with the respective labels `Parent one` and `B-only`. The narrow window retained horizontal scrolling. In the scale-2 Xvfb session, all four connectors remained distinct at full size and after Fit in an 860-logical-pixel window. |
| S01 | Caret execution returned only `fastdb_accept_a, 271`. Selection returned only `314`. Whole script appended `314` then `fastdb_accept_a, 271` in separate results and the toolbar showed connection A. |
| S02 | Format via Ctrl+Shift+F split the input at FROM and WHERE and preserved the Unicode literal. One Undo and Redo restored byte-identical input and formatted output. Select-all, cut, paste and clipboard round-tripped identical bytes. Shift-click selected `pha\nbeta` from two lines as expected. A 150-line paste scrolled down and right, but the last line's `END_S02` marker and caret were outside the viewport, including after Ctrl+End. Comment, Indent, Outdent and Find/Replace controls were absent from the SQL rail. |
| S03 | Typing `it` after FROM suggested `items`, `items_materialized` and `items_view`. Typing a period after alias `i` suggested columns from `items`. An unterminated string showed syntax coloring only while typing. Execute displayed `Unterminated SQL literal or identifier`, and the corrected script returned `corrected`. Snippets, Import SQL copy and Save SQL copy had no entry point on the SQL rail. |
| S04 | `SELECT :value AS value;` preview escaped the apostrophe by doubling it. Copying the result returned the exact UTF-8 `O'Reilly 日本語` despite missing Japanese glyphs in the test image. Integer `-42`, Boolean `true` and SQL NULL returned their expected values. Integer `42x` left Execute disabled and displayed a validation error. Cancel closed a binding with a distinctive value without adding a result, and the script backend's last query remained `SELECT NULL AS value;`. |
| S05 | The exact three-statement script retained `11`, displayed the native missing-table error and produced no result for the third statement. A separate run showed zero affected rows for UPDATE and one result row for SELECT, with matching counts and elapsed times in the transaction log. The DO block succeeded. A clean repeat raised `s05-gui-notice`. The visible result showed zero affected rows, and Transaction log recorded the original SQL once. The separate Execution log control was absent, so the emitted server notice was never shown as a separate message. |
| S06 | PostgreSQL independently confirmed `pg_sleep(30)` was active before Cancel and no longer active after it. Tab 2's Execute showed a busy refusal and produced no result after cleanup. The same tab that ran the sleep then returned `1`. The log marked the sleep cancelled after about four seconds. The documented five-second Timeout control was absent from the script rail; the candidate source initializes the tab timeout to off and has no action that changes it. |
| T01 | Separate Create, Insert and Select runs in Script-1 returned ID `7` on backend 69. Script-2 used backend 71, reported the table invisible, and got a native missing-relation error when selecting it. Closing Script-1 removed backend 69. Reopening the saved script still returned missing-relation on its fresh session. Script-2 kept backend 71. |
| T02 | The transaction toolbar showed Commit, Rollback and a mode/isolation menu, with no explicit Begin action. Selecting Manual Commit left backend 76 idle. The first UPDATE began a transaction implicitly. The tab saw its pending change while independent psql read the original. A later toolbar Rollback left backend 76 idle with ROLLBACK as its last command. Toolbar Commit made a separate update visible to psql, and a final committed update restored `original`. |
| T03 | SQL `BEGIN` and `SAVEPOINT accept_sp` preceded a division-by-zero result with failed-transaction feedback. `ROLLBACK TO SAVEPOINT` recovered the tab and its next SELECT returned `1` on backend 76. The toolbar Rollback click in Auto mode left the backend in its transaction. Executing `ROLLBACK` in the editor returned the same backend to idle, confirmed independently by `pg_stat_activity`. |
| T04 | Manual Commit left the scratch UPDATE pending. Enabling Auto-Commit was refused with `Commit or Rollback before enabling autocommit`. After toolbar Rollback, Auto-Commit took effect and psql still read `second`. Repeatable read was selected while idle and `SHOW transaction_isolation` returned `repeatable read`. Selecting the database opened a new temporary PostgreSQL tab that returned `postgres`. Script-1 retained `fastdb_accept_a`, and choosing `release_other` there changed its `SHOW search_path` to `release_other`. |
| T05 | Load editable on a direct SELECT enabled scalar edits. Save on ID 1 persisted `t05-first`, confirmed in psql. An unsorted reload changed the row order, so the stale-write attempt was repeated after checking the ID shown in Grid. After a separate psql session wrote `t05-external-final` to ID 1, Save reported a conflict, retained the staged `t05-stale` value, and psql confirmed the external value survived. A fresh fixture reproduced the conflict. Cancel cleared the draft, the next query showed `t05-external-final`, and Load editable refused both the view and the join. |
| R01 | Light theme and zoom 15 survived restart. Zoom in, out and reset saved 14, 13 and 13 as expected. The hidden Navigator saved width 0 but reopened visible. The first shortcut after restart hid it again and the second opened it at width 280. |
| G01 | On separate A/B PostgreSQL 16.16 databases, A still read `alpha` through psql while its grid edit was staged. Undo restored `alpha`, Redo restored the draft, and Cancel removed it. Repeating and saving changed A to `alpha-edited`. A separate psql read confirmed B still stored `alpha`. |
| G02 | Inserted ID 1001 with parent key `(1,1)` and left Note at DEFAULT. After Save, psql read `default note`. Duplicated the row, changed its ID to 1002 and its name to `qa-g02-copy`, then saved. Undo restored ID 1001 after a staged Delete and psql still read both rows. Shift-click selected both scratch rows. Delete and Save restored the fixture to exactly 1,000 rows with neither scratch ID present. |
| G03 | One Save contained A's staged name change on ID 1 and amount `-1` on ID 2. PostgreSQL reported `items_amount_check`. Both drafts remained visible. Separate reads confirmed A retained the committed `alpha-edited` and ID 2's `0.00`. B retained `alpha` and `0.00`. Cancel discarded both drafts and the prior error banner stayed visible. |
| G04 | Drag and Shift-click selected the three-row Name/Note rectangle. TSV matched the expected bytes, CSV parsed to all three rows and SQL replay into a temporary target preserved NULL, empty text, Unicode, quotes and LF. A 2×2 paste staged four cells and one Undo restored the entire rectangle. A paste with a valid ID followed by invalid `tenant_id` left Save disabled and psql confirmed the replacement ID was absent. Valid paste and Save stored literal `NULL` as four bytes and empty text as zero. Set SQL NULL changed the former to SQL NULL and left the latter empty. |
| G05 | Header sort by parent then ID displayed `3,2,1`, matching an independent PostgreSQL read. Moving Name to the right of Note and hiding Previous ID made Note/Name the visible paste order. A two-cell TSV paste staged the values in those fields and copying returned them in the same order. Restore all brought back Previous ID and the original column order with both drafts attached to their fields. Find matched `Tiếng Việt` and the staged values, wrapped Next/Previous, and reported no match for `NULL`. Record went from loaded row 3 to 2 and back to 3, then returned to Grid with row 3 selected. Cancel removed both drafts and PostgreSQL still returned `alpha` and SQL NULL for ID 1. |
| G06 | In the PostgreSQL fixture, item 1001 pointed to parent `(2,1)`. Forward navigation applied both predicates and displayed only `(2,1,Other tenant)`. Reverse navigation applied `tenant_id=2 AND parent_id=1` and returned only ID 1001, excluding fixture rows that shared one key component. The picker staged both fields as `(1,2)`. Cancel restored `(2,1)`, and independent psql confirmed that ID 1001 and sibling ID 1002 kept their original keys. The picker action was clipped at a 1280-pixel window width and became visible at 1800 pixels. |
| D01 | The quoted preview showed type, default, NOT NULL, multiline comment and rename. Cancel preview left PostgreSQL with nullable `note text` and no default. Persist changed it to `details varchar(80) NOT NULL` with the exact SQL default and two-line comment. The open Properties tab reflected the catalog change and Revert removed a later draft name. |
| D02 | After an independent psql session changed the comment, Persist reported a concurrent change and kept the draft. Refresh and Discard read the external comment. A second preview dropped the default, changed the type to varchar(81), and set an invalid default function. PostgreSQL rejected the function and an independent catalog read showed varchar(80), the original default, NOT NULL and the external comment. |
| D03 | Created `release_check.ddl_scratch` from the schema menu with an ID primary key and `note text`. The quoted preview matched independent catalog read-back and the table appeared in the Navigator. Added `scratch_col text` and renamed it in the Navigator. Dropped a disposable index seeded through psql and verified its absence with `to_regclass`. D01 also exercised column type change. A separate fixture dropped the scratch column and a check constraint through their quoted previews and confirmed both were absent. Create View returned 34, ALTER VIEW renamed it with the same result, and Drop View removed it. Rename Table changed the catalog name and refreshed the Navigator. The desktop had drop actions for indexes and constraints but no separate creation controls. Those fixtures were seeded in psql. |
| D04 | The Truncate and Drop previews identified the disposable `release_check.ddl_scratch` table. Independent psql read 1 row after Truncate Cancel and 0 after confirmation. Drop Cancel kept the table and confirmation removed it. Rename to an empty identifier disabled Execute. Rename to the existing `items` table returned the native duplicate-relation error and left both tables present. |
| V01 | Value showed SQL NULL for the first note, JSON with the exact large integer and both duplicate keys, and unchanged mixed XML. The binary viewer distinguished `00 01 7f ff`, zero bytes and SQL NULL. IDs 1 and 2 displayed the same UTC instant. Independent psql matched the stored JSON and XML, byte values and both epochs. |
| V02–V03 | Double-clicking the binary cell reported that editing requires Value Editor. The grid context menu, footer and Value panel had no action that opened it. The candidate defines `open_value_editor` but has no call site. The typed edit/file workflow fails at entry and stale Apply is blocked by that missing entry. |
| X02 | A duplicate key in the second batch rolled back that batch. The GUI reported 256 committed rows. Independent psql counted all 256 expected IDs, neither second-batch probe row, and the original seed. On separate fresh tables, Escape and the window close control requested cancellation during a server-confirmed delayed insert in batch two. The wizard stayed open while cancelling. The final reported and independently read counts were 0 for Atomic and 256 for Per-Batch. Both attempts displayed an import-session cleanup timeout. PostgreSQL was still executing the Per-Batch insert after that outcome and later became idle with no additional committed rows. |
| X04 | Transfer from the active SQL result committed exactly IDs 2 and 3 with exact notes. UPDATE RETURNING and a DELETE CTE each executed once in the editor and left Transfer disabled. A duplicate in batch two left the target's two seed rows in Atomic mode and committed only IDs 1001–1256 in Per-Batch mode. Cancelling a server-confirmed delayed second batch reported 0 or 256 newly committed rows, matching independent PostgreSQL. Both cancellation outcomes also showed a transfer-session cleanup timeout. The Per-Batch insert remained active after the outcome. The separate cross-provider BLOB failure still applies. |
| X06 | Selecting UTF-8 on a UTF-16 LE file produced a preview error. Selecting UTF-16 LE imported the exact three fixture notes, confirmed by independent hexadecimal UTF-8 read-back. Clearing the default NULL token stored a literal backslash-N at ID 4. Turning Has Header off exposed Col 1 and Col 2, and Start Import showed a mapping error before a destination was mapped. Mapping Col 1 and Col 2 imported the exact ID 5 row. Omitting the note mapping on ID 7 stored the PostgreSQL column default. An invalid Integer conversion committed zero rows with Skip off, then reported one processed and one rejected with Skip on. Independent psql confirmed ID 6 stayed absent. Escape and the close button kept the wizard open until it displayed the outcome in X02. |
| X03 | With the same items filter `id <= 25 ORDER BY id` and fetch size 10, the native Save File dialog produced loaded and full CSV and SQL files. CSV parsing verified headers, ordered IDs 1–10 or 1–25, the Unicode name and the quoted LF. Loaded CSV represented NULL and empty as empty; full CSV represented NULL as `\N`. Both SQL files replayed into a temporary table with zero differing rows, including exact JSON, XML, binary, numeric and UTC values. The original table still had 1,000 rows. |
| X05 | The native replacement prompt overwrote a test file with the exact loaded CSV. In a two-statement SQL script, Export All on Result 2 produced only the header `active_statement` and value `520`. A separate 6,000-row full export reported 895 rows written before Cancel. FastDB reported the destination unchanged, the existing bytes matched, and no matching temporary file remained. Attempting to save to the container's read-only filesystem displayed the native error and created no file. Earlier native dialog Cancel left no destination. |
| R04 | The PostgreSQL connection dialog accepted the test CA and localhost and rejected an unrelated CA and the IP hostname mismatch. The client-certificate account rejected no identity and a cert-only PKCS#12 archive. The full archive passed. With SSH enabled, the wrong fingerprint failed and the correct fingerprint passed. Connect Database then opened a live session and independent `pg_stat_ssl` read TLS 1.3 and the expected client DN. |
| W03 | Auto-Commit Save was visible immediately in independent PostgreSQL. The Manual selector opened below the toolbar and within the window. An earlier pending update remained visible in the table after a later duplicate-ID Save failed, with feedback that only that batch rolled back. Independent psql still read the committed rows. Disconnect ended all fixture app sessions and left the earlier pending write uncommitted. Prior runs verified Manual commit, rollback and close. |
| W04 | The dialog displayed UTC, SQL type, duration, rows and result. Filtering by the synthetic bound value `qa_hidden_binding_w04` located the expanded SQL and its literal. Searching by the statement alias returned the same expanded text. The guide requires bound values to be omitted, so the redaction subcase fails. A second run produced 130 matching statements. Scrolling reached records past 100, and the same filter found the records again after closing both script tabs and disconnecting. |
| W05 | The first run revealed a zero-row result, and command and error runs also created result tabs. Dragging the divider to both extremes stopped near 15% and 85%. Script-1 retained the tall split and Script-2 its default split after switching. Closing the last result restored the full editor. The Grid, Text, Record and Panels rails were visible at the normal split. |
| W01 | Native and workspace title rows showed the connection and active script. Openbox moved the native window from `82,77` to `154,115`, while dragging the in-app title left the window at `82,77`. Double-click on the in-app title set and cleared both EWMH maximize flags. Dragging the divider widened Connections by about 82 pixels. At 570 pixels, horizontal wheel scrolling revealed the right side of the toolbar without overlapping window controls. |
| W02 | The four filter modes and active tooltip appeared. Connections refused the table-only term `items` and matched the saved connection. Catalog mode matched `release_other` as a schema. Tables kept `incoming` under its original Tables folder with `Filtered by search`. Clearing restored the expanded `release_check` tree. Files reopened Script-1 with bytes identical to the saved SQL. Clearing Tables did not offer Browse catalog objects. The Navigator only calls its search-results renderer when the query is nonempty. |
| W06 | Double-clicking a Navigator table loaded its two rows without a separate data action. Record opened at row 2 in Grid and retained it in Text. Copy record returned the formatted header and values. Double-click and F2 did not open an editor from the Record button view. Previous/Next, Escape and All rows returned the expected row and full presentation. Text scrolled horizontally to the final column. PostgreSQL and the fetch footer showed 2,001 rows, while the Text clipboard contained only IDs 1–2,000 with aligned columns and `[NULL]`. The 266-character note preview stopped at 255 characters. The Text header showed the loaded count of 2,001. |

The R01 sidebar restoration subcase fails. The restore code clamps a saved
width of 0 to a positive width. Retest with the panel hidden before clean
Quit, verify it stays hidden at relaunch, and then verify the first toggle
restores the panel. The temporary harness also had a stale expectation of
260 for the explicit show action. It observed 280, the current default.
The app logged a separate certificate-trust failure checking GitHub from
this isolated fixture.

## Linux desktop runbook coverage

This index covers the IDs in the [PostgreSQL runbook](0.2.0-postgres-manual-acceptance.md).
Partial results credit only the observed substeps in this report or the
[lifecycle smoke record](../WORKFLOW_ACCEPTANCE.md#postgresql-desktop-smoke-follow-up-2026-11-23).
Pending rows name an unverified desktop route. R05 is the Finder gate
deferred with macOS. Linux packaging has its own results above. The current continuation runs the remaining GUI checks on isolated Xvfb displays.
Of the 61 indexed IDs, forty-two pass as written, L04 passes for the planned
Linux contexts, sixteen fail a subcase, one is blocked, and one is deferred.

| Case | Status | Evidence or remaining check |
| --- | --- | --- |
| C01 | Pass | Verified selectable and deferred providers and PostgreSQL Back/Next draft retention at scales 1 and 2. |
| C02 | Pass | Wrong password and closed port produced distinct errors. Correct settings passed Test and connected to A. |
| C03 | Pass | B profile settings persisted. A retained edited name, timeout and application name after reconnect. The numeric field filtered nonnumeric keystrokes and validation rejected 0 and 121. |
| C04 | Pass | Both PostgreSQL profiles reconnected after one restart with saved credentials in Linux local credential mode. No prompts appeared. The seeded legacy MariaDB profile retained its fields and stayed disabled. |
| B01 | Fail (multiline grid layout) | Paging, scroll to ID 1000, resize/autofit and Grid/Text passed. At 1280 pixels and scale 1, the lines of an LF-containing note were cut by the fixed row bounds. Its clipboard bytes matched PostgreSQL exactly. |
| B02 | Pass | Generated order, NULL, empty, quoted literal, AND and OR matched native PostgreSQL read-back. |
| B03 | Pass | Back and Forward restored filter, order, rows and fetch sizes 10 and 20. Criteria Cancel and forward-branch replacement passed. |
| B04 | Pass | Confirmed both panels hide and return at their prior widths on two tabs, using Escape and Restore workspace. |
| M01 | Fail (narrow Properties layout) | Properties, full clipboard, native DDL replay and estimate labeling passed. The narrowed scale-2 Columns view wrapped and clipped the comment row. |
| M02 | Pass | Enter and double-click select the correct column editor. Verified zero rows, disabled Virtual, read-only identity/generated definitions and a distinct missing-column error. |
| M03 | Pass | Verified links, partition bounds, trigger/rule/policy/permissions, loaded-empty state and view refresh. |
| M04 | Pass | Verified all seven object kinds, native definitions, saved favorite across restart, externally renamed object feedback, search and reveal in A. |
| M05 | Pass | Verified composite selection, self and incoming edges, referenced-column arrows, badge-free cards, pan, zoom, Fit, Reset, Focus and earlier SVG export. |
| M06 | Pass | Header and row drag, reopen and zoom persistence, A/B layout and navigation isolation, and narrow/HiDPI layout verified. |
| G01 | Pass | Undo, Redo and Cancel changed only the draft. Save committed ID 1 on A and B kept the original name. |
| G02 | Pass | Insert and Duplicate saved the expected IDs, parent keys, names and default note. Undo restored a staged Delete. Saving both deletes restored the original count. |
| G03 | Pass | The two-row Save hit the check constraint, retained both drafts, and preserved both committed rows. Cancel cleared the drafts. |
| G04 | Pass | Drag/Shift-click, all three copy formats, rectangular paste with one Undo, atomic paste rejection and SQL NULL distinction passed. |
| G05 | Pass | Private Xvfb verified hide, move, two-key sort, restore, visible-order TSV, preserved drafts, Find and Record navigation. Independent psql confirmed the order and unchanged persisted values. |
| G06 | Pass | Both directions used the composite key, the picker staged both fields and Cancel preserved the original rows. The picker footer clips its action at 1280-pixel width. |
| S01 | Pass | Verified separate caret, selection and whole-script execution, result order and connection identity. |
| S02 | Fail (editor controls and viewport) | Format, Undo/Redo, selection and Unicode clipboard passed. Required line-edit and Find/Replace controls were absent and the long-paste caret remained clipped. |
| S03 | Fail (editor controls and diagnostics) | FROM and alias completion passed. Diagnostics appeared only on Execute. Snippets and SQL copy actions had no reachable rail controls. |
| S04 | Pass | Typed parameter results, integer validation and Cancel passed. Clipboard read-back verified the exact UTF-8 string. |
| S05 | Fail (Execution log route) | Script boundaries, native errors, counts and timing passed. The documented Execution log was absent and the PostgreSQL notice was not displayed separately. |
| S06 | Fail (timeout control) | Cancel, alternate-tab busy refusal and recovery passed. The documented Timeout control was absent. Retest setting it to five seconds, cancellation and reset when the control is available. |
| S07 | Pass | Explain and read-only Analyze displayed the tree and raw output, including actual rows, loops, timings and buffers. |
| T01 | Pass | Separate runs retained the temporary table on its owning SQL tab only. Closing and reopening that script removed the temporary table and left its sibling backend running. |
| T02 | Fail (toolbar Begin) | Manual mode began implicitly and its toolbar Rollback and Commit passed independent read-back. No explicit toolbar Begin was available. |
| T03 | Pass | SQL savepoint recovery returned 1 and explicit SQL Rollback ended the transaction, confirmed on the same backend by PostgreSQL. |
| T04 | Pass | Verified refusal with pending work, rollback before Auto-Commit, isolation read-back, and separate database and in-place schema contexts. |
| T05 | Pass | Verified Save, stale-write conflict and retained draft, Cancel/reload, and read-only view and join rejection. |
| D01 | Pass | Verified quoted preview, Cancel, Persist and catalog refresh, and Revert. Earlier tests also exercised dirty Close. |
| D02 | Pass | Verified stale snapshot refusal, Refresh and rollback when a later DDL statement failed. |
| D03 | Fail (index and constraint creation controls) | Verified table and column actions, view create/alter/drop, and index and constraint drop with independent PostgreSQL. The separate index and constraint creation controls were unavailable. |
| D04 | Pass | Truncate and Drop Cancel/confirm, empty-name validation, and native duplicate-name failure matched independent PostgreSQL checks. The [MySQL and SQLite retest](#linux-mysql-and-sqlite-table-mutations) verified the same flows and found an ineffective Cascade option on both providers. |
| V01 | Pass | Value viewer preserved exact JSON/XML, distinguished binary and NULL states, and displayed both matching UTC instants. Grid multiline clipping is tracked under B01. |
| V02 | Fail (Value Editor entry) | Binary grid edit refers to Value Editor, but the candidate has no entry point. Typed edits and file round trips remain unverified. |
| V03 | Blocked | Stale Apply requires a reachable Value Editor. |
| X01 | Pass | Cancel left zero rows. The UTF-8 comma import preserved all three exact notes. In the semicolon import, the preview showed the default unquoted `\N` as NULL. Independent psql confirmed ID 5 was SQL NULL and ID 6 was empty text with length 0. |
| X02 | Fail (cancel cleanup) | Duplicate and cancel durability matched the expected counts for both policies. Both cancel attempts reported cleanup timeout, and the Per-Batch SQL was still running after the GUI outcome. |
| X03 | Pass | On one capped, ordered query, loaded CSV/SQL contained 10 rows and full CSV/SQL contained 25. CSV content and independently replayed SQL matched the fixture. |
| X04 | Fail (BLOB fidelity and cancel cleanup) | SQL result scope, write-result refusal and duplicate/cancel durability matched native read-back. Cleanup timed out on both policies, and the BLOB runner changed or rejected valid bytes. |
| X05 | Pass | Native dialog Cancel, replacement, visible read-only filesystem failure, running-export cancellation with preserved destination, and active SQL result scope verified. |
| X06 | Pass | Checked UTF-16, NULL tokens, manual mapping, skipped-column default, rejected conversions and the wizard cancellation lifecycle. X02 records the underlying cleanup failure. |
| L01 | Pass | Verified query, Save and Refresh ownership, error isolation, same-name tabs and staged edit retention. |
| L02 | Pass | Explicit Cancel and Disconnect targeted A, B stayed usable on the same backend, and A did not reopen after the old query deadline. Earlier close/reopen discarded the old result. |
| L03 | Fail (stale departure feedback) | Dirty guards and durability checks passed. The obsolete Save or Cancel banner survived Cancel and reconnect with no active draft. |
| L04 | Pass for planned Linux contexts | Tab close, shortcut Quit, window close, and save failure/retry preserved the expected bytes. Native macOS menu and About paths are deferred. |
| R01 | Fail (sidebar) | Theme and zoom restored. The saved hidden Navigator reopened visible. The SQLite follow-up restored pane widths and both script tabs but reset column order and visibility. The transaction log reset on restart. The Timeout preset is unavailable. |
| R02 | Pass | Quit released the pending transaction, preserved exact script bytes, and restart left the unexecuted INSERT absent. |
| R03 | Pass | Read-only refused writes and writable destinations as expected. Independent PostgreSQL confirmed a normal A profile write. |
| R04 | Pass | Connection dialog verified SSH pinning, TLS CA and hostname, and client identity. PostgreSQL confirmed the connected session used TLS 1.3 and the intended client DN. |
| R05 | Deferred | The macOS Finder and clean-user DMG gate is outside the requested Linux run. |
| W01 | Fail (workspace title drag) | The native Openbox frame moved, but dragging the documented FastDB title row did not move the window. Double-click zoom, divider resize, separate rows and the narrow scrollable toolbar worked. |
| W02 | Fail (empty Tables query) | All mode searches, expanded tree restore and file open passed. Empty Tables search did not offer Browse catalog objects. |
| W03 | Pass | Verified immediate Auto-Commit, Manual visibility and durability, failed-batch rollback, close/disconnect and selector placement with independent psql. |
| W04 | Fail (parameter redaction) | A synthetic bound value appeared verbatim in Transaction log and matched a search. Paging and retention across tab close and disconnect passed. A later SQLite process opened with an empty session log. The controlled SQLite follow-up verified that closing the modal releases the pending snapshot descriptor. |
| W05 | Pass | Verified zero-row, command and error results, split bounds, independent tab layouts, the rail at normal height and full editor after the last result closed. |
| W06 | Pass | Private Xvfb verified Navigator autoload, read-only Record navigation, mode switches and copy, Text alignment, SQL NULL, horizontal scrolling and the 2,000-row / 255-character caps. The PostgreSQL SQL-result retest also verified exact decimal and UTC display and retained selection. |
| W07 | Pass | Setup retained values across tabs and deferred providers were unavailable. |

## Linux P0-05 release decision

**Fail on the tested Linux candidate.** Required cases B01, S05, S06, T02,
X02, X04 and L03 contain observed failures. The other 16 indexed gate
cases passed in the planned Linux contexts. The runbook case rows above
describe the exact failures and retest conditions. The macOS and supplemental
release gates remain separately scoped.

### Evidence required for the seven Linux release retests

| Case | Retest and pass evidence |
| --- | --- |
| B01 | At 1280 pixels and scale 1, display both lines of the PostgreSQL fixture's LF-containing note within its row and compare the copied bytes with psql. |
| S05 | Run the success, notice and failure statements through the SQL editor. Verify a reachable Execution log shows the native notice separately and reports each statement's outcome. |
| S06 | Set the tab timeout to five seconds, run a query longer than the preset, verify it ends and the tab recovers, then clear the preset and verify it no longer cancels that query. |
| T02 | Invoke explicit Begin from the toolbar. Verify with an independent connection that an update remains invisible before Commit, persists after Commit and disappears after Rollback. |
| X02 | Cancel during a server-confirmed PostgreSQL insert in batch two under both commit policies. Verify the active insert terminates, cleanup succeeds and independent reads see the policy-specific committed rows. |
| X04 | Verify exact MySQL-to-SQLite BLOB read-back on a corrected candidate. Separately cancel PostgreSQL transfer during batch two under both policies and confirm writer cleanup and committed row counts. |
| L03 | Stage an edit, trigger the departure guard, cancel the edit and reconnect. Verify the guard clears when the draft is discarded, stays clear after reconnect and native data is unchanged. |

## CI coverage and supplemental gates

| Scope | Automated evidence | Remaining Linux acceptance |
| --- | --- | --- |
| Builds, engine, drivers and transport | [CI](../../.github/workflows/ci.yml) runs the workspace suite in the Linux app job and requires live PostgreSQL/MySQL and a separate TLS script in the integration job. This report adds live MariaDB sessions, TLS, SSH cancellation and recovery, and credential-store evidence. | Live vendor ODBC. |
| Navigator, editor and disabled capabilities | The desktop cases above cover the observed routes. PostgreSQL plan snapshots and driver definition search have separate evidence in this report. | Remaining keyboard and preference routes beyond the tab-order, group, color and timeout tests above, plus object actions, result controls and disabled states. |
| Transfers and native jobs | CSV, same-provider transfer and native backup retests are recorded above. | All three planned GUI scalar directions and the cross-provider duplicate, FK and Cancel variants have native read-back above. BLOB rejection and silent byte changes remain failures. MySQL and SQLite native-tool GUI operations are blocked by missing entries. PostgreSQL GUI evidence covers full and scoped backups, restore success and populated-target refusal, maintenance, and Cancel with a controlled wait-only child. |
| Administration and Session Monitor | The live and app-level Task 8 retests cover mutations, native privileges and target identity. | All eight Administration families and the Session Monitor chain have packaged GUI results above. Administration catalog filtering is absent, failure cleanup wording is ambiguous, and one restricted monitor row overlaps the details pane. |
| Packages and updater | The tag-triggered [release workflow](../../.github/workflows/release.yml) checks apt install/remove, binary identity, launcher presence and linked libraries on Ubuntu 24.04. It does not launch the installed app. The retained package matches the recorded SHA-256 and its `Depends` field has no Fontconfig entry. The [minimal Ubuntu 24.04 retest](#minimal-ubuntu-2404-package-startup) reproduced the abort and verified startup when Fontconfig and its font dependencies were installed. | Retest once the dependency changes. Real signed update is Blocked while v0.3.0 is the latest published release. |
| Performance | The [desktop performance workflow](../../.github/workflows/benchmark.yml) is manually dispatched on a self-hosted macOS runner. This report records the Linux release PostgreSQL workload and private-display observations. | Approved Linux display policy run against a compatible baseline and separate dedicated-host cold start. |

## Linux acceptance reconciliation

The [test plan](../plans/2026-11-23-full-feature-test-plan.md) and its
[findings matrix](2026-11-23-test-findings.md#supplemental-observations)
track the app, connection, grid, SQL, updater and performance cases.
The [separate PostgreSQL runbook index](#linux-desktop-runbook-coverage)
tracks the release workflow. The two indexes cover all 136 IDs in their
source checklists, each exactly once. Of those, 131 were exercised and
86 passed. The five cases without an executed verdict are listed below.
The phase checks and standalone provider fixtures have separate results.

| Matrix | Pass | Fail | Blocked | Skipped or deferred | Total |
| --- | ---: | ---: | ---: | ---: | ---: |
| Supplemental plan | 43 | 29 | 2 | 1 | 75 |
| PostgreSQL runbook | 43 | 16 | 1 | 1 | 61 |

| Scope | Disposition and evidence |
| --- | --- |
| Phase A | Formatting, Clippy with warnings denied, all-targets build and release build have passing Linux evidence. [Build evidence](#multiline-grid-regression-retest-2026-11-23) |
| Phase B | The workspace run that includes the multiline fix passed 836 harness cases across 42 suites. The Linux attempt at the macOS bundle test reported a Minisign harness failure. macOS execution is deferred. [Bundle test finding](2026-11-23-test-findings.md#f-04--citest_bundle_macospy-cannot-fully-pass-on-linux-and-is-not-run-by-ci) |
| Phase C | C1–C8 have passing Linux automated evidence, including the later C6 native backup retest with 22 checks. C9 vendor ODBC remains Blocked because vendor drivers and authorized server fixtures are absent. [Original results](2026-11-23-test-findings.md#results), [backup retest](#linux-native-backup-retest) |
| Phase D | D3 and D4 Linux packaging and installed launch/removal passed. The minimal-install Fontconfig issue remains recorded. D5 failed because Linux exposes no About entry. D1 and D2 are macOS-only and deferred. [Packaging and updater](#linux-packaging-and-updater), [desktop installation](#ubuntu-2404-desktop-launcher-and-removal) |
| Phase E | The overall Linux P0-05 gate is Fail. Test coverage includes an empty-profile launch and shortcut/window Quit. APP-01 retains the earlier 2,874 ms Fail. The later Xvfb diagnostics are below budget, with dedicated cold startup still open. APP-05 remains Fail because the Quit menu action has no reachable entry. [Release gate](#linux-p0-05-release-decision), [startup](#linux-empty-profile-startup), [Quit](#linux-quit-guards-and-save-recovery) |

## Open acceptance cases

| Scope | Remaining work or prerequisite |
| --- | --- |
| Linux P0-05 | Retest B01, S05, S06, T02, X02, X04 and L03 on a fixed candidate. The crash fix is already present in this package. B01 still clips the second line, and X04 still records BLOB fidelity and cleanup failures. |
| Other desktop failures | Each Fail row records the observed result or absent route. Retest the Value Editor file and UTC actions, SQL timeout preset, and MySQL and SQLite native-tool dialogs when their UI entry points are available. |
| Remaining provider subcases | The [native-tool checklist](p2-02-manual-acceptance.md#results) and [metadata results](#linux-navigator-folder-and-key-metadata) retain their provider-specific coverage. |
| Release packaging and performance | The minimal DEB startup reproduced the Fontconfig failure. AppImage extract-and-run passed and mounted launch requires FUSE. The dedicated cold-start check still requires the approved Linux host. |
| Vendor providers | The live SQL Server, Oracle and Db2 fixtures remain Blocked by the agreed provider scope. Their directive parser cases passed. [Provider prerequisites](#linux-definition-search-ssh-and-credential-store-retest) |

| Case without an executed verdict | Disposition and prerequisite |
| --- | --- |
| V03 | Blocked. A reachable Value Editor is required to stage and reject a stale Apply. |
| UPD-04 | Blocked. The latest public stable release is v0.3.0. In-app acceptance requires a compatible newer package signed with the production trust key. |
| PERF-02 | Blocked. An approved X11 display configuration and comparable baseline are required. PERF-03 recorded native Wayland frame intervals separately. |
| PERF-01 | Skipped. This run covers Linux. The fixed-display macOS benchmark is deferred. |
| R05 | Deferred. This run covers Linux. Finder and signed macOS package acceptance are deferred. |

## Documentation audit

The candidate package SHA-256 is
`23e7a07a81a58dd45f638ec0bbe57dfc5bb384ffbe69cfec6a0829a207823a77`.
Comparing the current tracked tree directly to `7c248df` finds only
acceptance-documentation differences.
Local links and anchors in the two plans, both reports and the native-tool checklist were checked
along with `git diff --check` and `cargo fmt --all -- --check`.
