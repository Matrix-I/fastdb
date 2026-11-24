# FastDB full feature acceptance results

Candidate: FastDB 0.3.0 at `359502a`. Linux x86_64, functional GUI on a private Xvfb display with software Vulkan. PostgreSQL 16.16, MySQL 8.4.13, MariaDB 11.4.13 and disposable SQLite files. See the [test plan](../plans/2026-11-22-full-feature-acceptance.md).

The continuation covers Linux as requested. The macOS matrix is deferred for this pass.

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

The PR's hosted checks passed for live engine and drivers, Linux build
and macOS build/test at `4d2fc89`. The manual macOS matrix remains deferred.

### L01 overlap retest after push

The debug build of `227a16d` ran against disposable PostgreSQL A/B fixtures on
private X11. Independent PostgreSQL reads confirmed that each A operation was
waiting on a test lock while B was active.

| Operation | Observed result | Status |
| --- | --- | --- |
| Save | After the lock was released, only A's intended ID 4 changed. B kept its database-specific value. | Pass |
| Refresh | Only A received the refreshed row and new result column. B kept its original rows and columns. PostgreSQL confirmed the new column existed only in A. | Pass |
| Failing Save | A displayed the check-constraint error and retained the staged value. B showed no error or row change. Independent PostgreSQL reads confirmed that both stored amounts remained 0.00. | Pass |

## Open acceptance cases

| Plan tasks | Remaining coverage |
| --- | --- |
| 1–2 | Vendor ODBC clients and disposable Oracle, SQL Server and Db2 servers are unavailable. MariaDB-specific transport and tab-session coverage requires a driver-specific probe. |
| 3–6 | Linux connection and preference matrix, broader metadata and schema mutations, record/text/transaction grid cases, and remaining editor actions. |
| 7–8 | Broader import cancellation and batch policies, transfer and native tools GUI matrix, and the Administration transaction and privilege cases outside the observed subcases. |
| 9 | Ubuntu 24.04 interactive desktop acceptance and a newer production-signed update. Fontconfig omission remains open for minimal installs. |
| 10 | Approved-display benchmark, compatible baseline comparison, and cold startup after a coordinated reboot. |
| 11 | Final case-by-case reconciliation after the open runs. P0-05 remains open. |
