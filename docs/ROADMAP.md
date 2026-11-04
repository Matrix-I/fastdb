# FastDB roadmap and feature audit

Audited: **2026-11-02** against the current working tree, including in-progress PostgreSQL session/query-plan changes. This is a source audit, not a live database or desktop acceptance report, and does not describe the older v0.1.0 release. Existing implementation plans describe intended work; they are not proof of completion.

Follow-up: [DBeaver GUI and business workflow analysis](DBEAVER_WORKFLOWS.md) specifies the column editor, draft/persist lifecycle and screen-family coverage. Direct DBeaver inspection is pending desktop permission; documentation-backed behavior is distinguished from proposed FastDB contracts.

Architecture & technical debt backlog: [Codebase Architecture & Improvement Plan](plans/2026-11-03-codebase-architecture-improvements.md) documents God files, bad designs, and phased refactoring tasks (P1–P3).

## Status and completion rules

| Status | Meaning |
| :--- | :--- |
| Wired | A reachable UI action connects to implementation and presents an outcome in the stated scope. Runtime acceptance still needs evidence. |
| Partial | A useful subset is wired, but the wider workflow or provider coverage is incomplete. |
| UI only | A visible control or section has no action or real result for that feature. |
| Backend only / stub | Library types or code exist, but there is no usable application workflow. |
| Missing | No implementation of the described application workflow was found in the inspected code. |

A feature is complete only when the path **UI entry → action → driver/service → real result or persisted change → success/error/cancel feedback** works. For read-only viewers, require real source data and meaningful loading, empty, unsupported and failure states. A tab, icon, type definition, SQL preview or passing unit test alone does not close an end-to-end feature.

Completion must name supported providers and limits, add owning-crate tests, and record an app smoke test. Database changes need execution and read-back verification; destructive actions need SQL preview/confirmation and appropriate rollback/error behavior. Keep result processing bounded and heavy work off GPUI render. Use `cargo fmt --all`, `cargo clippy --workspace --all-targets`, and `cargo test --workspace`; live tests must distinguish skipped from passed. These are implementation gates, not claims that this documentation audit exercised every workflow.

## Confirmed misleading or incomplete UI

Paths below are relative to `crates/app/src/workspace/` unless linked otherwise.

| Surface | Status and evidence | Required result before closing |
| :--- | :--- | :--- |
| Properties: Dependencies, References | **Implemented for PostgreSQL**: direct catalog dependencies/dependents, relation navigation and refresh; GUI acceptance pending. | Transitive dependency visualization remains separate. |
| Properties: Partitions | **Implemented for PostgreSQL**: direct parent/child links and native bounds; GUI acceptance pending. | Partition mutation actions remain separate. |
| Properties: Triggers, Rules, Policies | **Read-only readers implemented for PostgreSQL**: native definitions, enabled states and RLS details. | Mutation actions remain separate. |
| Properties: Permissions | **Read-only ACL reader implemented for PostgreSQL**. | Effective privileges and grant/revoke UI remain separate. |
| Properties: Virtual | **Disabled with explanation**; no virtual-key/relationship workflow. | Persist user-defined keys/relationships and use them in navigation/editing with explicit semantics. |
| Table toolbar: Expand | **Implemented**: navigator/value panes hide with preserved widths; Restore button/Escape works from every tab. | GUI acceptance pending. |
| Table toolbar: Criteria | **Implemented**: catalog column picker, AND/OR predicates, NULL/value distinction, provider literals, source preview and execution. | GUI acceptance pending. |
| Table toolbar: history | **Implemented**: per-tab executed filter/order/fetch-size snapshots with bounded back/forward and branching. | GUI acceptance pending. |
| Diagram | **Partial**: [`diagram_panel.rs`](../crates/app/src/workspace/diagram_panel.rs) renders cached multi-table PK/FK graphs with column links, two-axis scrollbars, zoom/pinch and navigation; GUI acceptance pending. | Multiple entities, real FK edges, incoming/outgoing relationships, navigation, layout, zoom/pan and export. |
| SSH | **Backend stub**: [`ssh_tunnel.rs`](../crates/db-wire/src/ssh_tunnel.rs) binds a local listener but has no accept/forward loop; [`connection_state.rs`](../crates/app/src/workspace/connection_state.rs) sets `ssh_tunnel: None`. | Connection dialog, verified host keys/authentication, actual forwarding, lifetime management, failures and database round-trip through the tunnel. |
| Light theme | **Backend only**: [`theme.rs`](../crates/ui-core/src/theme.rs) has a palette; workspace initializes `Theme::dark()`. | Settings action, live propagation to every pane, persistence and restart verification. |

## Existing workflows: preserve their actual scope

| Area | Status | Evidence and limits |
| :--- | :--- | :--- |
| Connection CRUD/test and credentials | Wired | `connection_dialog.rs`, `connections.rs`, `connection_config.rs`; native PG/MySQL/SQLite and vendor ODBC paths. Keychain save/migration errors have feedback. Provider availability is not equal to feature parity; see [provider validation](PROVIDERS.md). |
| TLS | Partial | PG/MySQL certificate/hostname verification and optional root CA are wired. Do not generalize to every ODBC/platform/authentication combination. |
| Navigator | Partial | `navigator/`, `nav_context_menu.rs`: tables/columns/constraints/indexes and connection actions. Schema tree currently groups results under Tables; no complete object-type explorer or object DDL action menu. |
| Table browsing | Wired | `query.rs`, `fetch_plan.rs`, `data_panel.rs`: streamed results, filter text, header ordering, fetch-next, cancellation and errors. Advanced criteria/history controls above remain unfinished. |
| Grid edits | Partial | `data_edit.rs`, `db-wire/src/sql.rs`: table row edit/insert/duplicate/delete and transactional Save/Cancel. `can_edit_active_rows` requires a table target; arbitrary script results do not become editable merely because they use the same grid. |
| Clipboard/export | Wired, bounded scope | `grid_clipboard.rs`, `export.rs`: TSV/CSV/SQL copy and loaded-result CSV/SQL file export. Not a full-table transfer, import, schema backup or lossless binary backup; script SQL exports target `query_result`. |
| Value panel | Partial | `value_panel.rs`, `value_format.rs`: selected-cell formatted display, format picker, wrap and copy have actions. Not a structured value editor or BLOB file import/export workflow. |
| Properties metadata and DDL | Partial | `properties/`, `ddl.rs`, `db-wire/src/postgres/metadata.rs`: real PG columns/constraints/indexes/FKs/statistics and scoped structural DDL. Other providers have partial metadata/DDL; viewing/copying DDL does not create or alter objects. |
| SQL scripts | Wired, bounded scope | `sql_script.rs`, `script_store.rs`, `script_tools.rs`, `script_view.rs`: local autosave, chooser, rename/delete, execution, recent history and formatting. Sequential script parsing has documented dialect restrictions; retains latest 20 results. |
| SQL transactions/session state | Partial | `pinned_script.rs`, `db-wire/src/postgres/script.rs`: PostgreSQL persistent tab sessions, Begin/Commit/Rollback and SQL savepoints. Other providers do not have equivalent pinned sessions. Session persistence means across runs in an open tab, not restoration after app restart. |
| Query plans | Partial | `query_plan.rs`, `db-wire/src/query_plan.rs`: PostgreSQL tree/raw JSON viewer, Explain and executing Analyze. Other provider plan adapters, comparison and richer profiling remain open. |
| Performance/packaging | Partial | Paged/spilling results, virtual grid, benchmark runner, macOS preview and updater exist. Original performance targets, broader platform distribution and signed/notarized release acceptance remain separate work. |

## Delivery order

Priorities below are proposed implementation order, not release dates. PostgreSQL is first for catalog and administration work; provider expansion follows capability tests. Every unchecked item is still open even if part of its UI exists. Checkboxes track implementation and automated validation; manual GUI acceptance is separately marked pending at the user’s request.

### P0 — Make the existing UI truthful and finish its result paths

- [x] **P0-01 — Capability states (source audit complete; manual testing skipped by user).** Metadata availability, per-section failures, permission errors and loaded-empty states are distinct. Virtual is disabled with an explanation. Source audit found real actions on Expand/Criteria/history/diagram and no exposed SSH/theme no-op controls. Columns no longer invent default collation or blank identity metadata. Provider declarations have automated coverage; desktop/provider acceptance is not claimed.
- [x] **P0-02 — Missing Properties readers (implemented; GUI acceptance pending).** Implement Dependencies, References, Partitions, Triggers, Rules, Policies and Permissions for PostgreSQL. Acceptance: catalog fixtures with real objects produce navigable rows; empty/error states and refresh work. Keep mutation actions a separate milestone.
- [x] **P0-03 — Filter workflow (implemented; GUI acceptance pending).** Implement criteria builder and per-tab history; define expand behavior. Acceptance: generated predicate/order, executed SQL and returned rows agree; back/forward and refresh retain the intended state.
- [x] **P0-04 — ER relationships (implemented; GUI acceptance pending).** Replace the single entity preview with a usable FK diagram. Acceptance: composite/self-referencing/incoming/outgoing FKs render correctly and open the right connection/schema/table; layout and viewport remain responsive.
- [ ] **P0-05 — Workflow acceptance matrix (protocol prepared; execution pending).** [Provider matrix and repeatable protocol](WORKFLOW_ACCEPTANCE.md) cover connect → browse → run → edit/save/read-back → export/read-back → cancel/disconnect for each claimed provider, multi-connection identity, stale async completion, pending edits and failed transaction recovery. Manual execution is deferred to the user; no pending or skipped test is validated support.

- [x] **P0-06 — Columns metadata layout and interaction (implemented; manual testing skipped by user).** Single-line clipped rows, full native metadata inspection/copy and double-click/Enter object navigation are wired. Missing collation/identity are labelled not reported; loaded-empty Columns has explicit feedback. Narrow/HiDPI and desktop clipboard behavior remain unverified.
- [x] **P0-07 — Column object editor and schema drafts (implemented; GUI acceptance pending).** Open a dedicated column tab with actual native attributes; edit name/type/nullability/default/comment with capability gating, Save SQL preview, Persist, Revert and catalog read-back. Include dirty-close handling, conflicts and failure recovery. See the [field and lifecycle contract](DBEAVER_WORKFLOWS.md). Keep identity/generated/storage/permissions transitions separately capability-tested.
- [x] **P0-08 — Reliable application Quit (implemented; manual testing skipped by user).** Global menu dispatch defers window access; keyboard/menu/window-close enter the save-before-quit path, with dirty-column guards, script saves, queue flush and dirty recheck. Save-queue failure/retry has automated coverage. Focus-context desktop dispatch remains unverified.

### P1 — Daily database development workflows

- [x] **P1-01 — Object explorer and search (implemented; desktop acceptance pending).** PostgreSQL extended object browsing and on-demand metadata/source plus connection-qualified persisted favorites are wired.  The active connection supports literal substring search over loaded relation/schema and column names, with qualified navigation. Reveal active object expands the owning live connection and native relation folder, selecting the qualified relation and scrolling it into view. Dedicated views/materialized views, sequences, functions/procedures, triggers, types and database/schema nodes with capability-aware loading; object search/filter, refresh, reveal active object, folders/favorites. Acceptance: selection resolves to the correct object and opens real metadata/source.
- [x] **P1-02 — Schema/object editing (implemented; desktop acceptance pending).** Create/alter/rename/drop tables, columns, constraints, indexes and views; truncate table with confirmation. Modal provides interactive form fields, column management for create table, live dialect-aware SQL preview, validation, execution on isolated connection with 15s deadline, transactional rollback indicator (PG/SQLite) vs non-transactional implicit commit warning (MySQL/MariaDB), destructive danger alerts, copy SQL, and automatic navigator catalog refresh on success. Acceptance: automated SQL generation, isolated execution, catalog mutation, and truncation verified with unit and integration tests; manual GUI acceptance remains deferred.
- [x] **P1-03 — SQL editing essentials (implemented; desktop acceptance pending).** Comment/uncomment and four-space indent/outdent toolbar actions operate on selected lines/current line with Undo and autosave. Find/Replace supports case-sensitive literal matching, wrapped navigation, selected-match replacement and one-step Replace All. External UTF-8 `.sql` files import as independent autosaved scripts; Save SQL copy writes a snapshot to another destination. Richer dialect keywords (PostgreSQL/MySQL/SQLite/ANSI) and parameter tokens (`:param`, `@param`, `$1`, `?`) highlighted. Real-time syntax diagnostics flag unterminated strings, comments, and syntax parse errors. Standard searchable SQL snippets and templates (Query, DML, DDL, Advanced) insert at cursor with full Undo support. Interactive parameter binding modal prompts for typed values with live SQL preview and tab-session persistence before execution. Acceptance: actions modify/execute the intended selection and remain undoable; parameters are typed safely.
- [x] **P1-04 — Execution feedback and controls (implemented; desktop acceptance pending).** SQL tabs retain the latest 100 execution outcomes while open (no restart persistence, per user choice), with timing, result/command count distinction, bounded PostgreSQL notices, MySQL warning details and native PostgreSQL error positions. Per-tab timeout presets default off; workspace-wide single-operation policy reports busy refusals and retains ownership through cancellation cleanup. ODBC informational diagnostics/structured positions remain unsupported. Acceptance: distinguish result rows from DML counts, expose errors without losing earlier statement outcomes.
- [ ] **P1-05 — Transaction/provider parity.** Pinned sessions for MySQL/MariaDB, SQLite and supported ODBC providers; autocommit/isolation controls, active database/schema selection, savepoints and transaction-state visibility. Add vendor batch/procedural syntax with dialect tests. Acceptance: session variables/temp tables survive across runs; commit/rollback and failure recovery use that same session. Partial implementation: SQLite/MySQL tab sessions, native transaction feedback, isolation selection and native MySQL/MariaDB plus client-managed PostgreSQL/SQLite autocommit controls are wired; Database/schema selectors, ODBC tab sessions and vendor batch preflight are wired. Live ODBC, separate MariaDB and desktop acceptance remain open.
- [x] **P1-06 — Grid productivity (implemented; desktop acceptance pending).** Multi-column sorting/filtering, hide/reorder columns, result find, record view, FK value lookup and related-row navigation. Add validated multi-cell paste, explicit NULL/default handling and editable query results only when origin/key mapping is unambiguous. Acceptance: operations affect the intended rows and preserve staged edits safely. Partial implementation: loaded-result Find, Record, multi-column sorting and criteria, column visibility/order, validated TSV paste, explicit NULL/default actions, and FK value lookup with related-row navigation in either direction are wired. FK candidate selection can stage a selected target key. PostgreSQL Load editable result supports verified direct schema-qualified SELECT * results with complete primary keys and scalar non-key updates, guarded writes and read-back. Broader query/provider editing remains capability-gated. Desktop acceptance is assigned to the user.
- [x] **P1-07 — Value editing (implemented; desktop acceptance pending).** Bounded text/JSON/XML source and binary hex editors, background value viewer and file load/save, strict UTF-8/UTF-16 encoding and timezone-aware UTC display. Apply stages one undoable change and rejects stale results/connections/edits; table Save preserves binary and NULL/empty/default distinctions. JSON formatting preserves original numeric tokens and duplicate keys; XML source stays unchanged. SQLite, PostgreSQL and MySQL have real write/read-back coverage. The 256 KiB limit is explicit; larger LOB streaming and schema-aware XML validation remain follow-ups. ODBC live checks stay deferred to Linux; Oracle empty binary edits are explicitly unavailable. Desktop acceptance is assigned to the user.
- [ ] **P1-08 — Data import and transfer.** CSV import with delimiter/encoding/header/NULL/type mapping, validation preview, batching and error policy; table/query-to-table transfer and full-source streaming export. Acceptance: read-back values/counts, progress/cancel, explicit partial-commit behavior and bounded memory.
- [ ] **P1-09 — Connection usability/security.** Real SSH tunnel; provider-supported client certificates/authentication options, advanced properties/timeouts, connection groups/colors and read-only safeguards. Acceptance: settings change the actual connection and failures are actionable without exposing secrets.
- [ ] **P1-10 — Workspace preferences.** Restore open tabs/layout and unsaved editor state after restart, configurable shortcuts/fonts/theme and persisted panel/grid preferences. Acceptance: reopen the right scripts/connections; never replay SQL or pretend to restore a live transaction.

### P2 — Administration, analysis and broader parity

- [ ] **P2-01 — Database object administration.** Routine source/edit/run with arguments, sequence inspection/reset, trigger/rule/policy editing, partition management, roles/users and grant/revoke UI. Acceptance: preview, provider execution, refreshed catalog and permission-error feedback.
- [ ] **P2-02 — Backup/restore and maintenance.** Native-tool backup/restore, vacuum/analyze/reindex and provider equivalents. Acceptance: executable discovery/options, progress/logs/cancel and a verified restore into an isolated database.
- [ ] **P2-03 — Session/lock monitoring.** Active queries, sessions, blocking chains and terminate/cancel actions with confirmation. Acceptance: live refresh and verification that the selected server session was targeted.
- [ ] **P2-04 — Data/schema search.** Search object definitions and table data across a selected scope with bounded results, progress/cancel and navigation to matches.
- [ ] **P2-05 — Plan and statistics coverage.** MySQL/MariaDB, SQLite and ODBC plan adapters; estimates vs actuals, expensive-node navigation and saved plan comparison. Complete other-provider metadata/DDL and currently unsupported PostgreSQL relation kinds.
- [ ] **P2-06 — Comparison.** Start with read-only object/schema and keyed data diffs; then reviewed synchronization SQL, dependency ordering and execution/read-back. Handle large datasets and cross-provider type differences explicitly.
- [ ] **P2-07 — Transfer formats and analysis.** JSON and spreadsheet import/export, HTML/Markdown output, configurable formats, result aggregates/charts and ER image export. Validate type/NULL handling and output contents for each format.
- [ ] **P2-08 — Reusable tasks.** Saved import/export/script/maintenance configurations, task history and optional scheduling with credential references, cancellation and useful failure reports.
- [ ] **P2-09 — Provider/platform validation.** Live Db2 validation, broader ODBC TLS/OS tests, native Windows support if adopted, repeatable Linux artifacts and macOS signing/notarization. Do not equate a provider preset or successful compile with a supported release workflow.
- [ ] **P2-10 — Performance acceptance.** Cold-cache startup, live-provider latency, large metadata trees, mixed-width/LOB results and memory/scroll regressions. Publish reproducible measurements separately from aspirational targets.

### P3 — Optional extensions, not prerequisites for core parity

- [ ] Cloud/NoSQL/analytical providers, IAM/SSO and proxy integrations based on demand; preserve native driver architecture.
- [ ] Remote storage, team/shared workspace configuration and collaborative workflows.
- [ ] AI SQL assistance and database-specific procedure debugging, separately scoped with provider and edition comparisons.

DBeaver has a broader driver/plugin ecosystem; replicating JDBC driver management is not a FastDB goal. Additional native providers need their own proposal and acceptance matrix.

## Comparison baseline and references

The baseline is common desktop database work, primarily DBeaver Community workflows. Commercial capabilities are optional comparison targets, not claims that every DBeaver edition includes them. In particular, DBeaver's detailed Schema Compare is documented for Enterprise/Ultimate; simple object comparison and full migration must not be conflated. Check edition/provider support when starting each item.

Official references consulted on 2026-11-02:

- [DBeaver feature overview](https://dbeaver.com/features/): connections, metadata, object editing, diagrams, transfer and administration categories.
- [Data Editor](https://dbeaver.com/docs/dbeaver/Data-Editor/): data viewing/editing, ordering and result workflows.
- [SQL Editor](https://dbeaver.com/docs/dbeaver/SQL-Editor/): script editing, execution and connection context.
- [Database Navigator](https://dbeaver.com/docs/dbeaver/Database-Navigator/): explorer/navigation workflow.
- [Structure and data compare](https://dbeaver.com/docs/dbeaver/Structure-and-Data-Compare/) and [Schema Compare](https://dbeaver.com/docs/dbeaver/Schema-compare/): comparison scope and edition distinctions.

The backlog is FastDB's proposed scope informed by those categories, not a promise of exact DBeaver feature or edition equivalence.
