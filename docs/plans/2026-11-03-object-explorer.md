# Object explorer delivery

User scope: implement all roadmap points except AI; skip manual testing for now.
Automated tests/build/review remain required. Preserve unrelated in-progress
architecture and minisign changes in the working tree.

First deliver typed relation folders from existing provider metadata: Tables,
Views, Materialized Views and Other Relations. Reuse existing query/metadata
navigation and connection identity. Keep unknown relation kinds visible rather
than pretending they are tables. Use independent expansion keys for folders.
No driver public API or persisted format changes in this step.

Next extend provider object discovery/source navigation to sequences, routines,
triggers and types, then object search/favorites and schema editing. Provider
capabilities and real catalog data must determine supported object kinds. Any
new public catalog contract requires a concrete design review before changes.
P1-01 remains open until all its object types and actions are wired.

## Implementation and review

Four relation folders now classify native type labels, keeping unknown kinds in
Other Relations. Expansion keys encode schema length/name and category, preventing
collisions for quoted schema names containing separators. Existing connection,
schema and object navigation paths are reused. No new database query is added;
each category filters already-loaded metadata. Folders do not imply provider
discovery support, and empty states explicitly refer to loaded objects.

Manual testing is skipped as explicitly requested by the user; no visual or live
provider acceptance is claimed. Existing in-memory table-folder expansion keys
change to category-qualified keys; no persisted data or config changes.

Self-review: correctness has native-kind/unknown/key-collision unit assertions;
performance remains linear in loaded relation count with four fixed categories;
security adds no SQL, logging, auth or external calls; robustness uses existing
owned metadata with no resources/dependencies; syntax follows existing GPUI
patterns; APIs are existing supported methods; category semantics are explicit;
tests assert classifications and independent keys, manual testing skipped;
design confines category logic to schema rendering.

Validation: formatting, Clippy workspace/all-targets, app build and full workspace
tests passed with `--locked` (336 harness passes; unconfigured provider/keychain
tests remain early returns). Independent review found default expansion producers
still using legacy raw schema names. Both NavigatorState initialization and
connection initialization now use a shared table-folder key; regression assertions
confirm initial public Tables expansion without expanding Views. Re-review found
no remaining actionable issue. `git diff --check` passed. Manual app/GUI testing
was skipped per explicit user instruction; native display behavior is unverified.
No commits or pushes were made for this delivery.

## Search loaded metadata

Search the selected connection's loaded relations by schema or name and its
columns by name. Use a case-insensitive literal substring. Keep qualified
identities through navigation, run the scan outside render, bound visible results
and reject results after metadata replacement. Reuse the existing table/column
open paths and shared single-line editing controls. Verify lookup, empty input,
literal punctuation, duplicate names and the result limit before delivery.

Search implementation now uses the selected connection's Arc-owned metadata.
Enter starts a background scan, capped at 51 hits to detect overflow, and the
dialog displays at most 50. Qualified names narrow repeated column names.
Selecting a relation reuses existing query navigation. Selecting a column
displays the existing cached native detail formatter within the dialog.
Metadata replacement or connection changes invalidate result navigation.

Independent review identified unsupported column-editor navigation and
unreachable repeated column names. Both were addressed. A follow-up found
unnecessary data queries for column inspection; final inspection reads cached
metadata without creating a query. Re-review reported no actionable findings.

Self-review:
1. Correctness: tests assert schema/relation/column identity, qualified paths,
   Unicode, literal punctuation, empty/missing input and overflow bounds.
2. Performance: scans run on the background executor; render handles at most
   50 results, and column metadata is shared through Arc rather than copied.
3. Security: searches add no SQL, external calls, credentials or logging.
4. Robustness: task ownership cancels replaced searches; owner and metadata
   identity are checked before opening results; no dependencies are added.
5. Syntax/naming: formatter, workspace Clippy and app build pass using existing
   Rust/GPUI conventions.
6. Modern APIs: existing GPUI tasks, Arc and shared line input are reused.
7. Business clarity: user selected schema/relation and column names on the
   active connection; literal substring matching and the cap are documented.
8. Coverage: two search tests pass. Full workspace tests pass after rebuilding
   stale xtask artifacts that embedded a removed integration-worktree path.
   App startup with an isolated SQLite profile succeeded. Computer Use reported
   permissions not granted, so clicking/searching/clipboard and layout acceptance
   remain unverified. This feature is incomplete for desktop release acceptance.
9. Design: one focused module owns search state, lookup and dialog behavior.

Commands passed: cargo fmt --all --check,
cargo clippy --workspace --all-targets --locked,
cargo build -p fastdb-app --locked, cargo test --workspace --locked,
and git diff --check. Unconfigured live-provider and keychain tests may return
early; these are not live-provider validation. No search commits or pushes were
created. The earlier P0 integration is on local main at 3924365.

## Reveal active relation

Add a navigator action that locates the active relation/column tab using its
owning live connection and loaded metadata. Expand its schema and native relation
folder, select by schema plus name, and scroll the row into view. Script tabs,
disconnected sessions and objects absent from loaded metadata have no reveal
action. Reuse live connection activation without running SQL. Keep existing
selection/query behavior outside this navigation change.

Reveal is wired for loaded relation/column tabs in live sessions. The action
activates the owning session, expands the schema/native folder and anchors the
qualified selected row. Selection now retains schema plus relation name for
ordinary click/query navigation as well. No database query or reconnect is added
by reveal. A shared ScrollAnchor waits for the expanded target's prepaint before
scrolling, then notifies the cached navigator to repaint the new offset.

Independent review found initial anchor scrolling used stale prepaint coordinates.
Deferring it through the target's layout frame addressed this. Re-review found
no further actionable issues. Two tests cover native/unknown folder kinds,
ancestor expansion, preserving prior expanded schemas, qualified selection,
duplicate relation names, absent objects and script tabs.

Self-review:
1. Correctness: owner session must be live and the qualified relation must exist
   in loaded metadata; absent targets produce no action.
2. Performance: target lookup scans cached relation names without database I/O;
   render adds one target lookup and no metadata copying beyond the target row.
3. Security: no SQL, credentials, logging or permission changes are added.
4. Robustness: existing live activation and owned GPUI handles are reused; no new
   network operation, resource acquisition or dependency is introduced.
5. Syntax/naming: workspace formatter, Clippy and app build pass.
6. Modern APIs: ScrollAnchor, on_next_frame and existing GPUI notifications are
   used according to the bundled GPUI layout lifecycle.
7. Business clarity: reveal targets the active tab's owning live connection and
   cached relation, including its native folder; scripts have no relation target.
8. Coverage: deterministic tests cover expansion/identity; isolated SQLite app
   startup succeeds. GUI scroll, collapsed/distant rows and owner switching still
   require desktop acceptance because Computer Use permissions are unavailable.
9. Design: target resolution and expansion are isolated in navigator_reveal;
   existing query navigation remains the data execution entry point.

Desktop acceptance remains incomplete. No commit or push was created for reveal.
