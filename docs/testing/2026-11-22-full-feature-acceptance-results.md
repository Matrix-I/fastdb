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
| Debian package structure | The development binary passed the package script's launcher, metadata, payload and validation assertions. Candidate release package installation remains open. | Pass for development artifact |
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
- The retained Linux output directories contain Debian and standalone
  artifacts but no AppImage. The Linux build script generates an AppImage
  only when `appimagetool` is available, and this test shell does not have it.
  The conditional AppImage launch was not run.

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

Both tags predate the P3-05 verification commit `c8768a4`. This result covers
apt, startup with an empty probe profile, and preservation of the on-disk
test file. Interactive restart and saved-workspace read-back, verified updater
download and installer handoff remain pending.
The GitHub release list still identifies v0.3.0 as the latest published
release. The candidate has no newer published target for this upgrade gate.

## Linux release performance

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
The local `xtask benchmark-check` also rejected disposable copies of the
archived baseline when one field at a time was changed to an alternate
display ID or the current kernel release. Both runs returned
`baseline mismatch: environment`. These checks exercised the comparison
gate without launching FastDB or changing the archived reports.

The [Linux X11 lifecycle retest](../WORKFLOW_ACCEPTANCE.md#postgresql-desktop-smoke-follow-up-2026-11-23)
also verified window-manager close, Refresh and Criteria with a staged change,
and reconnect after Cancel. The stale Save or Cancel banner remained visible
after the successful reconnect, with clean grid state and unchanged database values.

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
This was a startup probe. Interactive Ubuntu 24.04 desktop acceptance remains open.

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
The desktop X04 controls remain pending.

## PostgreSQL catalog drop retest

On `c27ca70`, the app's `postgres_live_drop_schema_and_database` test passed
with `FASTDB_TEST_NAV_DROP_PORT` set to a disposable PostgreSQL 16.16 server.
The schema drop refused dependent objects without CASCADE and succeeded with
CASCADE. The database drop refused an active session and succeeded after its
release. An independent native client confirmed the target database was
absent and a marker in a sibling database still read `42`. Desktop preview,
Cancel and confirmed-action controls remain pending.

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
out of debug output. The remaining desktop controls are pending.

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
| B02 | Criteria generated `id ASC`. The ordered, NULL, empty, quote, AND and OR cases displayed IDs `1,2,3`, `1`, `2`, `3`, `3` and `1,3` respectively. Independent psql returned the same ID lists. |
| B01 | With explicit `ORDER BY id`, fetch size 20 showed IDs 1–20. Fetch Next loaded 40 and scrolling loaded 60. A subsequent 1,000-row fetch scrolled to ID 1000 and cleared the more-rows indicator. Drag changed the Name column width and double-click fit it again. Grid → Text → Grid worked. The multiline clipping noted earlier was still visible. Independent psql read 1,000 rows with IDs 1–1000. |
| B03 | Back and Forward restored SQL, order and rows. A follow-up at fetch sizes 10 and 20 restored IDs `1,2,3` ascending and `10,9,8` descending, respectively, with matching footer sizes and independent psql results. Changing size had created an intermediate history entry, so the two saved endpoints were two steps apart. Criteria Cancel preserved the active filter. Branching to `id = 2` left Forward disabled. |
| B04 | Expand hid the Navigator on the items tab and Escape restored its prior width and active filter. The value-panel and second-tab variants remain unverified. |
| S01 | Caret execution returned only `fastdb_accept_a, 271`. Selection returned only `314`. Whole script appended `314` then `fastdb_accept_a, 271` in separate results and the toolbar showed connection A. |
| S04 | `SELECT :value AS value;` preview escaped the apostrophe by doubling it. Copying the result returned the exact UTF-8 `O'Reilly 日本語` despite missing Japanese glyphs in the test image. Integer `-42`, Boolean `true` and SQL NULL returned their expected values. Integer `42x` left Execute disabled and displayed a validation error. Cancel closed a binding with a distinctive value without adding a result, and the script backend's last query remained `SELECT NULL AS value;`. |
| S05 | The exact three-statement script retained `11`, displayed the native missing-table error and produced no result for the third statement. A separate run showed zero affected rows for UPDATE and one result row for SELECT, with matching counts and elapsed times in the transaction log. The DO block succeeded. Delivery of its server NOTICE was not observed. Earlier invalid attempts contained leftover editor text. |
| S06 | PostgreSQL independently confirmed `pg_sleep(30)` was active before Cancel and no longer active after it. Tab 2's Execute showed a busy refusal and produced no result after cleanup. The same tab that ran the sleep then returned `1`. The log marked the sleep cancelled after about four seconds. The documented five-second Timeout control was absent from the script rail; the candidate source initializes the tab timeout to off and has no action that changes it. |
| T01 | Separate Create, Insert and Select runs in Script-1 returned ID `7` on backend 69. Script-2 used backend 71, reported the table invisible, and got a native missing-relation error when selecting it. Closing Script-1 removed backend 69. Reopening the saved script still returned missing-relation on its fresh session. Script-2 kept backend 71. |
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
| D03 | Created `release_check.ddl_scratch` from the schema menu with an ID primary key and `note text`. The quoted preview matched independent catalog read-back and the table appeared in the Navigator. Added `scratch_col text` and renamed it in the Navigator. Dropped a disposable index seeded through psql and verified its absence with `to_regclass`. D01 also exercised column type change. |
| D04 | The Truncate and Drop previews identified the disposable `release_check.ddl_scratch` table. Independent psql read 1 row after Truncate Cancel and 0 after confirmation. Drop Cancel kept the table and confirmation removed it. Rename to an empty identifier disabled Execute. Rename to the existing `items` table returned the native duplicate-relation error and left both tables present. |
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
deferred with macOS. Linux packaging has its own results above. Remaining GUI checks require manual verification.
Of the 61 indexed IDs, twenty-two pass as written, L04 passes for the planned
Linux contexts, 26 are partial, 9 are pending, two fail a subcase, and one
is deferred.

| Case | Status | Evidence or remaining check |
| --- | --- | --- |
| C01 | Partial | Picker, disabled-provider clicks and navigation exercised. Verify the remaining visual states at the target scales. |
| C02 | Pass | Wrong password and closed port produced distinct errors. Correct settings passed Test and connected to A. |
| C03 | Partial | B profile state and timeout 121 observed. Verify the full A edit/reconnect, server application-name read-back and other invalid timeout inputs. |
| C04 | Partial | Saved PostgreSQL reconnect and disabled legacy MariaDB profile observed. Verify both profiles reconnect across one restart and record credential prompts. |
| B01 | Partial | Initial 20, manual next page, automatic fetch, scroll to ID 1000, resize/autofit and Grid/Text passed. Multiline clipping remains open. |
| B02 | Pass | Generated order, NULL, empty, quoted literal, AND and OR matched native PostgreSQL read-back. |
| B03 | Pass | Back and Forward restored filter, order, rows and fetch sizes 10 and 20. Criteria Cancel and forward-branch replacement passed. |
| B04 | Partial | Expand and Escape restored the items layout. Verify both panes and the second-tab variant. |
| M01 | Partial | Basic columns and primary-key DDL observed. Verify all fixture fields and sections, full copy, estimates and scale. |
| M02–M03 | Pending | Verify column keyboard/pointer routing and empty/unsupported/error states. Check dependencies, partitions, triggers, rules, policies, permissions and refresh. |
| M04 | Partial | Native table DDL and owner-specific catalog refresh observed. Verify remaining object kinds, favorites, search/reveal and disappearing-object feedback. |
| M05 | Partial | Diagram rendered composite, self and incoming links. Fit and SVG export passed. Verify connector interactions and remaining display controls. |
| M06 | Partial | Double-click opened the related A table. Verify drag and layout persistence, A/B isolation, and narrow/HiDPI layout. |
| G01 | Pass | Undo, Redo and Cancel changed only the draft. Save committed ID 1 on A and B kept the original name. |
| G02 | Pass | Insert and Duplicate saved the expected IDs, parent keys, names and default note. Undo restored a staged Delete. Saving both deletes restored the original count. |
| G03 | Pass | The two-row Save hit the check constraint, retained both drafts, and preserved both committed rows. Cancel cleared the drafts. |
| G04 | Pass | Drag/Shift-click, all three copy formats, rectangular paste with one Undo, atomic paste rejection and SQL NULL distinction passed. |
| G05 | Pass | Private Xvfb verified hide, move, two-key sort, restore, visible-order TSV, preserved drafts, Find and Record navigation. Independent psql confirmed the order and unchanged persisted values. |
| G06 | Pass | Both directions used the composite key, the picker staged both fields and Cancel preserved the original rows. The picker footer clips its action at 1280-pixel width. |
| S01 | Pass | Verified separate caret, selection and whole-script execution, result order and connection identity. |
| S02–S03 | Pending | Verify editor commands and undo, diagnostics and editor assistance. |
| S04 | Pass | Typed parameter results, integer validation and Cancel passed. Clipboard read-back verified the exact UTF-8 string. |
| S05 | Partial | Missing-table stop, prior result retention, zero affected rows, one returned row and log timing passed. Server NOTICE display remains unverified. |
| S06 | Fail (timeout control) | Cancel, alternate-tab busy refusal and recovery passed. The documented Timeout control was absent. Retest setting it to five seconds, cancellation and reset when the control is available. |
| S07 | Pass | Explain and read-only Analyze displayed the tree and raw output, including actual rows, loops, timings and buffers. |
| T01 | Pass | Separate runs retained the temporary table on its owning SQL tab only. Closing and reopening that script removed the temporary table and left its sibling backend running. |
| T02 | Partial | Manual mode rollback and commit passed independent read-back. Verify explicit toolbar Begin. |
| T03 | Pass | SQL savepoint recovery returned 1 and explicit SQL Rollback ended the transaction, confirmed on the same backend by PostgreSQL. |
| T04 | Pass | Verified refusal with pending work, rollback before Auto-Commit, isolation read-back, and separate database and in-place schema contexts. |
| T05 | Pass | Verified Save, stale-write conflict and retained draft, Cancel/reload, and read-only view and join rejection. |
| D01 | Pass | Verified quoted preview, Cancel, Persist and catalog refresh, and Revert. Earlier tests also exercised dirty Close. |
| D02 | Pass | Verified stale snapshot refusal, Refresh and rollback when a later DDL statement failed. |
| D03 | Partial | Table creation, column add/rename/type change and index drop passed. Manual acceptance remains for column drop, index creation, constraint and view operations, and table rename. |
| D04 | Pass | Truncate and Drop Cancel/confirm, empty-name validation, and native duplicate-name failure matched independent PostgreSQL checks. |
| V01 | Partial | Multiline copy and the large-value editor limit were observed. Verify the remaining fixture value types; clipped multiline grid rendering remains open. |
| V02–V03 | Pending | Verify typed edits and file round trips, invalid encodings and stale Apply refusal. |
| X01 | Partial | Alternate delimiters, pre-execution cancel, three exact UTF-8 rows and NULL versus empty observed. The GUI probe still needs the default backslash-N marker path. |
| X02 | Partial | Atomic duplicate rolled back the preceding valid row. Verify per-batch duplicate and cancel under both policies. |
| X03 | Partial | Full CSV and loaded SQL replay passed in separate runs. Verify loaded/full CSV and SQL side by side on the same capped query. |
| X04 | Partial | Navigator self-transfer refusal and exact same-provider atomic transfer passed. Cross-provider BLOB fidelity failed at runner level. Verify desktop SQL-result scope, write-result refusal, duplicate policies and cancellation. |
| X05 | Partial | Native dialog Cancel left no file. Verify unwritable destinations, successful replacement, running-export cancellation and active SQL result scope. |
| X06 | Partial | Custom NULL token distinguished quoted and unquoted values. Verify UTF-16, cleared token, manual mapping, rejected conversions and cancellation. |
| L01 | Partial | A/B grid identity, draft retention, overlapping Save/Refresh and error ownership, and Navigator refresh ownership passed. Verify the overlapping query substep directly. |
| L02 | Partial | Disconnect ended A's query while B kept its backend. Closing/reopening removed the old result. Verify explicit Cancel and absence of late reconnect. |
| L03 | Partial | Dirty grid guards, cancel, reconnect and independent Save read-back were exercised. The stale banner after reconnect needs a retest. |
| L04 | Pass for planned Linux contexts | Tab close, shortcut Quit, window close, and save failure/retry preserved the expected bytes. Native macOS menu and About paths are deferred. |
| R01 | Fail (sidebar) | Theme and zoom restored. The saved hidden Navigator reopened visible. Remaining layout, tabs, logs and timeouts need verification. |
| R02 | Pass | Quit released the pending transaction, preserved exact script bytes, and restart left the unexecuted INSERT absent. |
| R03 | Partial | Read-only SELECT, rejected UPDATE/rename, disabled grid changes and lack of writable transfer target observed. Verify that the normal A profile stays writable. |
| R04 | Pending | The automated connections passed transport assertions. The desktop SSH/TLS and client-identity dialog acceptance is pending. |
| R05 | Deferred | The macOS Finder and clean-user DMG gate is outside the requested Linux run. |
| W01–W02 | Pending | Verify title and toolbar behavior at narrow widths, then all Navigator filter modes, expansion restore and file opening. |
| W03 | Partial | Manual commit, rollback and close were read back independently. Verify automatic commit, failed-batch recovery, disconnect and selector placement. |
| W04–W05 | Partial | Log fields and filtering, initial full editor and result presentation observed. Verify redaction, paging, restart, split limits and closing the final result. |
| W06 | Pass | Private Xvfb verified Navigator autoload, read-only Record navigation, mode switches and copy, Text alignment, SQL NULL, horizontal scrolling and the 2,000-row / 255-character caps. |
| W07 | Pass | Setup retained values across tabs and deferred providers were unavailable. |

## CI coverage and supplemental gates

| Scope | Automated evidence | Remaining Linux acceptance |
| --- | --- | --- |
| Builds, engine, drivers and transport | [CI](../../.github/workflows/ci.yml) runs the workspace suite in the Linux app job and requires live PostgreSQL/MySQL and a separate TLS script in the integration job. This report adds live MariaDB sessions, TLS, SSH cancellation and recovery, and credential-store evidence. | Live vendor ODBC. |
| Navigator, editor and disabled capabilities | The desktop cases above cover the observed routes. PostgreSQL plan snapshots and driver definition search have separate evidence in this report. | Remaining keyboard/focus, connection preferences, object actions, result controls, and exposed disabled states beyond the provider picker. |
| Transfers and native jobs | CSV, same-provider transfer and native backup retests are recorded above. | Cross-provider transfer, running-job cancellation and the native-tools dialog matrix. |
| Administration and Session Monitor | The live and app-level Task 8 retests cover mutations, native privileges and target identity. | Remaining Administration families and the complete Cancel/Terminate and read-only dialog flows. |
| Packages and updater | The tag-triggered [release workflow](../../.github/workflows/release.yml) checks apt install/remove, binary identity, launcher presence and linked libraries on Ubuntu 24.04. It does not launch the installed app. The retained package matches the recorded SHA-256 and its `Depends` field has no Fontconfig entry. The private Xvfb GUI cases used an explicitly installed Fontconfig. | Retest startup on a minimal Ubuntu 24.04 install once the dependency changes. Verify the packaged update flow with a newer compatible signed target. |
| Performance | The [desktop performance workflow](../../.github/workflows/benchmark.yml) is manually dispatched on a self-hosted macOS runner. This report records the Linux release PostgreSQL workload and private-display observations. | Approved Linux display policy run against a compatible baseline and separate dedicated-host cold start. |

## Open acceptance cases

| Plan tasks | Remaining coverage |
| --- | --- |
| 1–2 | Live vendor ODBC is Blocked. The remaining desktop preflight applies to the enabled providers. |
| 3–6 | Linux connection and preference matrix, broader metadata and schema mutations, SQL-result Grid/Text/Record combinations, provider-specific table searches and transactions, and remaining editor actions. |
| 7–8 | Cross-provider BLOB fidelity failed. Broader import cancellation and batch policies, transfer and native tools GUI matrix, and the Administration transaction and privilege cases remain open. |
| 9 | Remaining interactive package and update actions on Ubuntu 24.04 and a newer production-signed target. Minimal-install startup failed before the explicit Fontconfig install. |
| 10 | Approved-display benchmark, compatible baseline comparison, and cold startup after a coordinated reboot. |
| 11 | Final case-by-case reconciliation after the open runs. P0-05 remains open. |

## Documentation audit

All 61 PostgreSQL runbook IDs appear exactly once in the Linux index. The
plan and this report resolve their local Markdown paths and anchors.
`git diff --check` and `cargo fmt --all -- --check` passed. Comparing
`7c248df` with `c27ca70` showed documentation-only changes. The audited
Xvfb DEB has the recorded artifact hash, and the working-tree changes
are limited to the plan and this report. The open rows above still
require their own execution evidence.
