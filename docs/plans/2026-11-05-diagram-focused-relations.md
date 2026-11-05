# Focused relationship diagram

P1-05 is deferred by the user. This change only addresses the table diagram.

- Show the selected table and its direct incoming/outgoing FK neighbors, preserving qualified identities, composite mappings and self references.
- Center the main table and balance neighbors around four sides without overlapping cards, following the user’s layout clarification. Center the initial viewport on the main table. Cache the focused graph outside repeated rendering work.
- Route column-to-column links with visible arrowheads. Clicking a line or relationship row selects its constraint and highlights the mapped columns at both ends.
- Add regression tests for filtering, layout, routing and selection; update the guide, run workspace verification and launch the application. Manual GUI acceptance belongs to the user.

No commit is created because the user has not requested one.

## Reproduction

Base: `6413e6e`, macOS, Rust 1.99.1. The existing builder includes every connection table; the panel only highlights the edge on selection. A regression fixture with root, parent, child, unrelated and grandparent returned 5 nodes instead of the expected 3 direct neighbors/root before the fix (`cargo test -p fastdb-app focused_graph_excludes -- --nocapture`, failed 5 vs 3). The focused version passes the same test. GUI reproduction/acceptance is assigned to the user; no Computer Use is performed.

Independent review found that top/bottom links needed exterior routing and the initial viewport needed centering. Both findings were addressed with regression coverage. No public API, database, dependency or persisted configuration changes are involved.

## Verification and self-review

- `cargo fmt --all` and `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets`: passed.
- `cargo test --workspace`: passed. Live provider variables are unset; this does not establish live provider acceptance.
- `cargo build -p fastdb-app`: passed.
- `cargo run -p fastdb-app`: launched `target/debug/fastdb`; macOS emitted XPC/LaunchServices warnings (`Connection invalid`, notification error -50). GUI operation is not established by startup alone and remains pending user acceptance.
- Full tracked diff and the untracked plan were reviewed; no unrelated edits, secrets or temporary artifacts were added. Cargo's runnable binary and reusable build dependencies are retained.
- Independent review: both findings addressed; re-review reported no remaining actionable findings.

1. Requirements/correctness: direct incoming/outgoing scope, isolated roots, qualified identities, self links, composite mapping, four-side placement and initial centering have regression coverage.
2. Performance: focused metadata/layout is cached per tab and metadata snapshot; existing visible-card culling remains, with no database calls added to rendering.
3. Security: only local graph/render logic changes; no SQL execution, credentials, authorization or external input handling changes.
4. Robustness/dependencies: no new external calls, dependencies or persisted formats; cache replacement clears stale selection and remains bounded to one snapshot per tab.
5. Syntax/naming: Rust compilation, workspace formatting and Clippy pass using existing conventions.
6. Modern APIs: existing GPUI canvas, scroll and pointer APIs are retained; no deprecated API introduced.
7. Business clarity: P1-05 is deferred; the user's four-side layout clarification and direct-neighbor scope are recorded explicitly.
8. Test coverage: filtering regression failed before the fix (5 vs 3) and passes after; layout, routing, composite highlighting and cache/viewport behavior are tested. Actual GUI clicks/readability remain unverified by user preference, so desktop acceptance is incomplete.
9. Design: graph filtering, focused layout, routing, selection mapping and viewport state are separate responsibilities within the existing diagram module.

Exceptions: no plan commit was made because the project explicitly prohibits commits unless asked. Automated GUI end-to-end interaction is omitted under the user's explicit no-Computer-Use preference; manual desktop acceptance remains pending. No reported release version was supplied, so reproduction used the current base commit only.

## Column-label clarification

Removed PK/FK text badges and the unused per-row FK lookup at the user's request. Rows show column names/types; connector selection still colors the mapped columns. The existing composite-selection regressions cover the preserved behavior; actual badge absence/readability is pending manual GUI acceptance. Independent review found no regression. Format, Clippy, build and workspace tests passed again. The first restricted test runs failed on loopback socket creation and macOS system configuration; the full suite passed when run with the required local environment access. No production logic outside the diagram was changed.

## Independent connector paths

The user reported merged connector trunks. The pre-fix 12-neighbor regression failed with `links 0 and 4 share a segment`. Routes now use distinct bounded lanes without modulo reuse and fan separate ports within their owning column rows. The regression also exercises 17 neighbors with 34 incoming/outgoing links and duplicate mappings between the same tables, checking every pair for positive-length collinear overlaps. All horizontal and vertical coordinates are reserved across routes to prevent endpoint/bend collisions. Independent review also identified same-row self links leaving the row; a 32-self-link regression failed at link 23 before allocating both ports inside the row. Routes are cached when graph/layout changes, keeping port allocation off repeated renders. Visual separation and pointer acceptance remain pending manual verification.

Final connector verification: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets`, `cargo test --workspace --quiet` and `cargo build -p fastdb-app` passed after the final code edit. Independent re-review found no remaining actionable issues. `cargo run -p fastdb-app` launched the updated binary; the same macOS XPC/LaunchServices warnings remain, and GUI acceptance is pending. The nine-item self-review above was repeated: updated requirements and same-row boundaries are covered by the failing-before/passing-after regressions; endpoint counting is linear and cached routes are reused; security, dependencies, persisted formats and business scope are unchanged; Rust syntax/APIs pass compilation/Clippy; filtering, allocation and routing remain separate methods. Existing unrelated credential changes in the working tree were preserved.

## Column name display and compact card width

The user reported that table cards were excessively wide and requested that diagram cards be narrowed down to only display column names.

1. Column rows in diagram cards now render only `column.name`; the secondary data type view (`raw_type` / `data_type`) is removed from the card row. Full column type information remains accessible in the footer when a column is clicked.
2. `fit_card_widths` now measures only the table header and column names, avoiding unnecessary horizontal stretching from lengthy data type labels like `timestamp with time zone`.
3. Default `WIDTH` constant is reduced to `180.0`, and `DiagramModel::build`'s fallback sizing logic excludes column type lengths.
4. Regression coverage in `measured_card_fits_widest_column_name_without_types` verifies that long data types do not expand card width and that cards fit only the column names.

