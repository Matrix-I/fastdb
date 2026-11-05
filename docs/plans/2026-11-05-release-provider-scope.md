# v0.2.0 provider scope and manual acceptance

The user requires PostgreSQL acceptance for the current release and explicitly
requests disabled MariaDB, Oracle, SQL Server and Db2 connection choices with
“Feature is coming”. MySQL and SQLite remain available; they are outside this
manual release gate. Existing saved profiles and driver implementations remain
compatible, but deferred profiles must not connect through the navigator.

1. Add one app-level provider availability policy, used by provider tiles and
   Test/Connect configuration; cover every provider and saved-profile loading.
2. Render unavailable tiles without click handlers, show the requested text,
   disable Test/Connect for a loaded unavailable profile, and report the same
   reason when a saved profile is opened for connection.
3. Document the scope and provide an ordered PostgreSQL manual acceptance runbook
   with fixture SQL, expected results and pass/fail/blocked recording.
4. Verify regression tests, workspace format/Clippy/build/tests, launch the app
   without Computer Use, and get independent review. Manual GUI remains pending.

No commit is made because commits were not requested. No database schema or
persisted configuration format changes are required by this app-level gate.

## Verification and self-review

The provider regression failed before the availability policy (`MariaDb` remained
available), then passed with all four deferred saved-profile kinds blocked and
PostgreSQL/MySQL/SQLite still producing usable configs. The fixture executed on
PostgreSQL 16.15 and produced baseline counts 1000/3/1/2/0/0/0. Full workspace tests
with live PostgreSQL passed; environment-dependent other-provider tests returned
early as documented. Format, Clippy with `-D warnings`, and all-target build
passed. The isolated app launched; manual GUI acceptance is still pending.
An initial full-suite attempt overlapped external test-first edits and failed
with missing `TabState::close_result`; after that external implementation arrived,
the complete rerun passed. Those external changes were preserved.

Independent review confirmed tile, Test, Connect, keyboard Enter and saved-profile
navigator gating; its runbook findings corrected missing transfer/configuration
and full-export cancellation/progress controls and the literal CSV NULL token.

1. Requirements: all four requested providers are visibly unavailable; supported providers and saved profiles remain intact.
2. Performance: availability uses a constant-size enum match and adds no I/O or render-path work.
3. Security: rejected providers never reach driver creation; no credentials are logged or persisted differently.
4. Robustness: normal configuration validation still runs for enabled providers; no dependencies or formats changed.
5. Syntax/naming: Rust formatter, compiler and strict Clippy checks pass with existing naming conventions.
6. Modern APIs: existing GPUI `when`/listeners and Rust enum matching introduce no deprecated API.
7. Business clarity: PostgreSQL alone is the release provider gate; MySQL/SQLite availability is preserved.
8. Coverage: regression exercises all seven kinds and deferred saved-profile loading; actual GUI remains explicitly unverified under the no-Computer-Use preference.
9. Design: one app-level policy is shared by rendering and connection configuration, leaving driver capabilities separate.

The SQL fixture is a test-only setup script for new disposable databases, not a
migration; no shared data-changing statement was run. No plan commit was made
because commits were not requested. No release artifact or publication is claimed.
