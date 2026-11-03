# Roadmap completion delivery plan

## Authorized scope

Complete P0–P2 in `docs/ROADMAP.md`; defer optional P3 extensions. The user
confirmed this scope on 2026-11-03 and will perform manual acceptance later.
Keep manual GUI and live-provider testing explicitly pending. Do not interpret
pending testing as a passing test or a supported release claim.

No commits or pushes are authorized. This plan remains in the working tree,
following the explicit no-commit instruction rather than the plan-commit convention.

## Delivery sequence

1. P0: audit reachable controls, finish capability feedback and column interaction,
   and supply a provider workflow acceptance matrix. Preserve existing column
   drafts, filtering, catalog readers, ER diagrams and save-before-Quit behavior.
2. P1-01/02: typed object explorer and PostgreSQL-first schema editing with
   preview, execution, conflict handling and read-back.
3. P1-03/04/05: editor commands/files/parameters, execution records and provider
   session parity. Preserve physical-session ownership and cancellation identity.
4. P1-06/07/08: grid/value workflows and bounded import/transfer/export with
   explicit NULL semantics, validation and partial-failure reporting.
5. P1-09/10: transport/security and persistent preferences/workspace restoration.
   Never restore or replay a live transaction automatically.
6. P2-01/02/03/04: provider-gated administration, native backup/restore,
   monitoring and bounded search.
7. P2-05/06/07/08: plans/statistics, comparison, formats/analysis and reusable
   tasks. Synchronization requires a reviewed write plan.
8. P2-09/10: platform/provider and performance evidence. Signing/notarization,
   vendor drivers, release credentials and dedicated benchmark hardware remain
   external prerequisites; do not substitute source implementation for evidence.

## Gates for each delivery

- Read owning code, callers and tests; refine the concrete behavior before edits.
- Add deterministic owning-crate tests; reproduce bugs before fixing them.
- Run formatting, Clippy, workspace tests and relevant executable flows.
- Update README and provider documentation with actual scope and limits.
- Obtain independent review for non-trivial changes and address its findings.
- Track implementation separately from manual acceptance; leave partially
  implemented milestones unchecked.
- Ask about unresolved business decisions before implementing dependent code.
  New public/config contracts and dependencies require review under repository rules.

## Current baseline

Branch: `feature/roadmap-completion`; initial commit:
`cf148fb` (`feat(app): distinguish metadata capabilities and load failures`).
The working tree was clean at the start of this delivery.
P0-02/03/04/07 are marked implemented with manual acceptance pending. P0-01
remains a partial control audit; P0-05/06/08 and P1–P2 remain open. README's
single-entity diagram description is stale relative to the roadmap and source.

## Delivery 1: P0 full-value access and acceptance protocol

Implemented a selected-column inspector after the Columns grid, with full-value
copying. Native case, Unicode, multiline values, empty strings and SQL NULL are
preserved; absent summary fields are labelled unavailable/not reported. Selection
is matched against the owning connection/schema/table. Removed columns no longer
produce details. The inspector follows the grid to keep double-click targets
stable, and stops mouse-down propagation to preserve selection during copying.

Added `docs/WORKFLOW_ACCEPTANCE.md` with all seven providers, read-back checks,
asynchronous identity, transaction failure and Quit scenarios. Corrected the stale
README ER-diagram checkbox. No P0 milestone is newly declared accepted; P1–P2
remain unimplemented by this delivery.

### Automated validation

- Test-first formatter tests initially failed to compile because the new helper
  did not exist; after implementation, both passed. A third test covers exact
  column names and removal after metadata refresh.
- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets --locked`: passed.
- `cargo build -p fastdb-app --locked`: passed.
- `cargo test --workspace --locked`: passed outside the sandbox, 324 harness
  passes. PG/MySQL/ODBC/keychain environment variables were unset; their live
  checks return early and are not validated provider coverage.
- The preceding sandbox run failed seven updater tests because local TCP binding
  and macOS system-configuration access were denied. No tests were weakened.
- `git diff --check`: passed.
- `FASTDB_CONFIG_DIR=$(mktemp -d /tmp/fastdb-roadmap-smoke.XXXXXX) cargo run -p fastdb-app --locked`:
  launched outside the sandbox and remained running without startup errors until
  stopped with Ctrl+C. This checks process startup, not interactive acceptance.
- Independent review identified selection-clearing and moving double-click targets;
  both were corrected and re-reviewed with no further actionable findings.

### Self-review

1. Correctness: exact values and missing/empty distinctions have three tests;
   the inspector only resolves the active table's selected column.
2. Performance: looks up one selected column in already loaded metadata; no
   database queries, result scans or background tasks were added.
3. Security: clipboard copying is explicit; no SQL execution, credentials or
   additional logging were introduced.
4. Robustness: removed columns return no details; no external calls, resources,
   dependencies or persisted formats changed.
5. Syntax/naming: formatting, Clippy and build pass using surrounding Rust patterns.
6. Modern APIs: existing GPUI event/clipboard APIs are reused; no deprecated API
   was introduced or reported by Clippy.
7. Business clarity: P0–P2 scope and deferred manual acceptance were confirmed;
   missing summary values do not imply known absence or default collation.
8. Coverage: formatter/selection tests pass; clipboard, focus and display acceptance
   are explicitly pending, so end-to-end release readiness is not claimed.
9. Design: formatting stays with Properties; existing selection and clipboard
   ownership are reused without a new shared state subsystem.

Manual acceptance and the plan-only commit convention are the deferred gates:
the user will test later and did not authorize commits. No commits were created.

## Delivery 2: P1-03 line editing

Implemented Comment/Uncomment, Indent and Outdent on the current/selected lines,
excluding an endpoint at the next line's start. Technical defaults: four spaces
for indent, one tab/up to four spaces for outdent, `-- ` after indentation for
comments, unchanged blank lines when commenting. Each command makes one buffer
edit with normal Undo and autosave, retains selected lines, and preserves native
line endings and Unicode. No public API or persisted-format changes.

Independent review found a caret advancing to the next line after a current-line
command. Mapped caret restoration and a non-final-line regression test address
it. Broader P1-03 work and manual toolbar acceptance remain pending.

Self-review:
1. Correctness: four tests cover boundaries, CRLF, Unicode, blank/empty input,
   selection, repeated current-line commands and Undo.
2. Performance: linear text transformation runs only on explicit commands;
   no work added to rendering or database fetching.
3. Security: changes local script text only; never executes generated SQL.
4. Robustness: uses existing buffer and save queue; file-busy guard prevents
   editing while script files move. No resources/dependencies added.
5. Syntax/naming: follows existing private workspace command patterns.
6. Modern APIs: reuses existing buffer, GPUI and autosave APIs.
7. Business clarity: indentation/comment conventions documented explicitly.
8. Coverage: deterministic real-buffer output/selection/Undo assertions;
   manual toolbar routing remains pending at user request.
9. Design: text transformation separated from toolbar and save orchestration.

Validation: `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked`,
`cargo build -p fastdb-app --locked` and `cargo test --workspace --locked` passed.
Workspace tests reported 328 harness passes, including four line-command tests;
live provider/keychain checks remain unconfigured and return early. An isolated
`cargo run -p fastdb-app --locked` launched without startup errors outside the
sandbox and was stopped with Ctrl+C. Interactive toolbar acceptance remains
pending. Independent re-review confirmed the caret fix with no further findings.

## Delivery 3: P1-03 literal Find/Replace

Added a toolbar dialog bound to stable script tab ID. Technical defaults are
literal case-sensitive whole-document search, wrapped Find Next, exact selected
match Replace followed by Find Next, and non-overlapping Replace All in one Undo
step. Empty replacement deletes; empty search does nothing. No regex expansion.
Tab switches fields; Enter finds; Escape clears the active field's selection
before closing; Cmd/Ctrl+W closes the dialog. Quit closes the dialog before
entering existing save/dirty guards. All changes use existing autosave.

Self-review: (1) real-buffer tests assert Unicode, wrap, case, literal replacement,
empty/no-match cases and Undo; (2) scans occur on explicit actions, outside render;
(3) no SQL execution or logging; (4) stable tab ID and file-busy guards protect
targets, no external calls or dependencies; (5) existing Rust/GPUI patterns;
(6) existing nondeprecated buffer/input APIs; (7) search conventions explicit;
(8) real-buffer tests cover logic, manual dialog/clipboard/focus acceptance pending;
(9) search state/logic and rendering isolated in a workspace module. No API or
persisted format changed. Milestone P1-03 remains incomplete.

Validation: formatting, Clippy (workspace/all targets), app build and workspace
tests passed with `--locked`; 331 harness passes include three new search tests.
Live provider/keychain checks remain unconfigured and are not validated support.
Independent review found caret-spanning wrap, Quit modal routing and cross-field
mouse selection issues; all were fixed and re-reviewed with no remaining findings.
`git diff --check` passed. Manual dialog, clipboard and focus acceptance remains
pending at the user's request; no commits were created.
The final build launched via `cargo run -p fastdb-app --locked` with a temporary
`FASTDB_CONFIG_DIR` outside the sandbox, remained running without startup errors,
and was stopped with Ctrl+C. This is startup evidence, not dialog acceptance.

## Delivery 4: external SQL copies

User confirmed importing external `.sql` as independent internal autosaved copies.
Added Import SQL copy and Save SQL copy toolbar actions. Input/output is UTF-8,
limited to 16 MiB; import rejects nonregular paths and invalid encoding. Import
allocates a new numbered script under the captured connection and installs a tab
only for that owner. Output uses a unique temporary file, fsync and atomic
replacement. Cancellation and errors return to the originating tab. Copy saves
the text captured when invoked, never executes SQL or changes the source path.

A separate per-tab dialog flag prevents repeated file operations while leaving
normal editing/autosave/close/Quit guards enabled. This addresses independent
review's finding that reusing file_busy could skip dirty saves. Review also
requested validating regular files before opening; both pre-open and descriptor
checks are present. No dependency/public API/persisted-format changes.

Self-review: (1) real filesystem tests verify exact Unicode/line-ending content,
independent imports, errors and limits; (2) bounded file reads and background I/O;
(3) no SQL execution, credentials or content logging; (4) temporary resources
released on failure, errors reported, source autosave remains enabled;
(5) surrounding Rust/GPUI naming/patterns; (6) existing path-dialog/tempfile APIs;
(7) copy/import semantics explicitly confirmed by user; (8) automated file tests,
native dialog and lifecycle acceptance pending; (9) file logic isolated from
tab installation and existing autosave. P1-03 remains open.

Validation: `cargo fmt --all`, `cargo clippy --workspace --all-targets --locked`,
`cargo build -p fastdb-app --locked` and `cargo test --workspace --locked` passed.
The final workspace run reports 335 harness passes, including four real-filesystem
tests for SQL files. Live provider/keychain checks remain unconfigured and return
early. The first test compilation exposed a GPUI glob-import collision with
`#[test]`; explicit test imports fixed it without changing the test assertions.
Independent re-review confirmed both file-lifecycle findings addressed. Startup
with a temporary FASTDB_CONFIG_DIR outside the sandbox succeeded and remained
running until Ctrl+C. Native-dialog/focus/close-during-dialog acceptance is pending.
No commits or pushes were made.

## Delivery 5: P1-03 SQL editing essentials completion

Implemented the remaining scope of P1-03:
1. Dialect syntax highlighting & diagnostics: Added dialect keywords for PostgreSQL,
   MySQL, SQLite, and ANSI SQL. Highlighted parameter tokens (`:param`, `@param`, `$1`,
   `?`) with parameter theme coloring. Real-time syntax diagnostics flag unterminated
   string literals, unclosed block comments, unmatched brackets, and syntax parse errors.
2. Snippets & templates: Added `sql-intel::snippets` with standard templates across
   Query, DML, DDL, and Advanced categories. Integrated toolbar `Snippets…` modal with
   filtering, preview, and undoable buffer insertion.
3. Parameter binding & variable prompts: Added `sql-intel::params` with parameter
   scanner, typed definitions (String, Integer, Float, Boolean, Date, Null, Raw), and
   safe substitution. Added `ParameterBindingModal` in workspace, intercepting queries
   containing unbound parameters, providing live SQL preview, tab-session parameter
   persistence, and execution dispatch.

Validation: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets`, and
full workspace tests pass. Manual UI acceptance remains pending at user request.
