# Not built yet

- **Navigator bulk actions.** Shift+click range selection and Escape clearing are implemented; the user accepted the selection behavior. Context-menu actions still apply only to the clicked target; bulk context-menu actions are not implemented.

- **Transaction log acceptance.** The split transaction toolbar selector (`T`/arrow for the mode menu, `Auto`/`Manual` label for the session-only Transaction log) and bounded disk-backed log modal have automated coverage; manual GUI acceptance is pending.

- **Table transactions acceptance.** Table tabs own dedicated physical sessions with Auto-Commit and Manual modes, Save/Commit/Rollback transaction boundaries, read-your-writes, and batch savepoint recovery; manual GUI verification is pending.

- **Workspace header acceptance.** Separate title/toolbar/tab rows, always-visible transaction controls (Commit/Rollback disabled in Auto-Commit) and button-anchored menus are implemented; native dragging/zoom, narrow-window scrolling and popup placement await manual GUI acceptance. Smart commit remains unavailable.

- **Navigator context menu acceptance.** Manual verification of menu placement, node-specific actions and metadata refresh is pending. PostgreSQL schema and database deletion dialogs also await desktop acceptance.

- **Navigator search acceptance.** The single search field, mode menu, tooltips and mode-specific results are implemented; manual acceptance of connection, schema/object, saved-file filtering and selection remains pending.

- **Scoped database search.** The Connections panel omits definition search and multi-table data search. Search table data is available from the open table’s footer.

- **Query plan coverage.** PostgreSQL, MySQL and SQLite estimated plans, PostgreSQL/MySQL/MariaDB actual-plan adapters, expensive-node navigation and versioned Save/Open JSON comparison are wired with automated coverage. MariaDB desktop connections remain disabled. SQL Server estimated plans await live ODBC acceptance. Oracle and Db2 plan adapters and remaining metadata/DDL coverage are open; desktop acceptance of the new controls is pending.

- **Transfer formats and analysis.** HTML, Markdown and JSON export of loaded results and focused ER SVG export are wired; desktop acceptance is pending. JSON import and full-source export, spreadsheet transfer, format options and result aggregates/charts remain open.


- **Record presentation acceptance.** Record now combines with Grid/Text in both table and SQL results; transposed grid/text layout, Tab toggling, Escape handling, inline editing and the Value panel in the table grid row preview, switching and scrolling await manual GUI acceptance.

- **Plain-text result acceptance.** Table tabs and SQL query results share a monospace text table; visual alignment and horizontal scrolling await manual GUI acceptance.

The [audited roadmap](../ROADMAP.md) separates working paths, partial features, UI-only controls and missing workflows. Major gaps include:

- **Title bar zoom acceptance.** Double-clicking the title or empty top bar toggles window maximize/restore; manual desktop acceptance, including tab and button click isolation, remains pending.

- **Deferred desktop providers for v0.2.0.** MariaDB, Oracle, SQL Server and Db2 connection choices are disabled with **Feature is coming**; existing profiles cannot Test/Connect. PostgreSQL is the current release acceptance gate; MySQL and SQLite remain selectable.
- **Data transfer limits.** Transfer and import write only into existing tables (no create-table-from-source). The destination picker lists tables whose columns were loaded when the connection opened; refresh the connection after creating a table. Mapping choices are not saved between runs. CSV import, transfer and export desktop acceptance is pending.

- **Additional Properties mutations.** Virtual keys/relationships are not implemented. PostgreSQL catalog sections are read-only; other providers explicitly show unsupported sections.
- **Advanced column editing.** PostgreSQL ordinary-column drafts, preview/Persist and Revert are implemented; identity/generated/storage/collation/permissions transitions and other-provider mutation support remain open. The [GUI/business analysis](../DBEAVER_WORKFLOWS.md) specifies this workflow and broader screen coverage.
- **Core DBeaver workflows.** ER diagram desktop acceptance (including SVG export), schema-aware XML editing, backup/restore, and session/lock monitoring remain open (CSV data import, streaming table transfer, full-source export, workspace session restoration and preferences are implemented; desktop acceptance pending). See the roadmap for scope and acceptance criteria.
- **Advanced grid productivity.** Loaded-result Find, Record, column display controls, multiple sort keys, validated plain TSV paste, and explicit NULL/default actions are available. FK navigation opens referenced or referencing rows in a filtered tab. PostgreSQL supports scalar non-key updates through Load editable result for a verified direct SELECT with a complete primary key. Broader SQL forms/providers, query-result row insertion/deletion and desktop acceptance remain open.
- **Execution feedback limits.** Execution logs and timeout choices live only in open SQL tabs. ODBC informational notices and structured error positions are not exposed; MySQL/MariaDB positions are limited to native error text. Desktop acceptance of Execution log/Timeout is pending manual verification.
- **Advanced script execution.** Transaction chaining remains unsupported for PostgreSQL. Dedicated ODBC tabs, supported database/schema selectors and vendor batch delimiters are implemented with automated fixtures. Live ODBC parity, separate MariaDB validation and desktop acceptance remain open.
- **SSH coverage.** PostgreSQL, MySQL, and MariaDB have fingerprint-verified SSH forwarding. Separate MariaDB validation and desktop acceptance remain to be verified.
- **Additional metadata coverage.** PostgreSQL Properties includes materialized-view columns, native types, details and structural DDL with no data; foreign/inherited/typed/temporary relation DDL remains open. PostgreSQL row counts are estimates, and partition-parent sizes are unavailable rather than a misleading zero or a sum of partitions. MySQL/SQLite have native scoped DDL and column readers; broader native index/constraint/statistics details remain incomplete. Other providers label unsupported fields as unavailable or retain a partial preview.
- **Theme and preferences.** Light theme toggle (View menu / `Cmd+Shift+T`), configurable editor font zoom (`Cmd+=` / `Cmd+-` / `Cmd+0`), and persistent panel/grid preferences are implemented; desktop acceptance belongs to the user.
- **Performance target validation.** The desktop regression suite measures startup, RSS, a million-row SQLite stream, scrolling and release executable size; cold-cache startup, live-provider latency and the original target claims still need separate measurements.

- **Value editing limits.** Text/JSON/XML source and binary hex editing, bounded background viewing, strict UTF-8/UTF-16 file load/save and timezone-aware UTC display are implemented. Editing/viewing is limited to 256 KiB; larger LOB streaming, XML schema validation, broader query-result types and restart persistence of value preferences remain open. Live ODBC validation stays deferred to Linux, and GUI acceptance belongs to the user.

- **Credential storage preferences.** Local development can opt into plaintext file credentials with `FASTDB_LOCAL_CREDENTIALS=1` (see [Connections](connections.md#local-development-without-keychain-prompts)); an in-app toggle and automatic migration between Keychain and local files are not implemented. Manual desktop acceptance is pending.

- **Credential notification acceptance.** Missing saved credentials at startup are logged without a persistent workspace banner; manual desktop acceptance of this presentation change remains pending. Save failures still expose a retry action.

- **Result tab close acceptance.** Each script result tab has a × button; closing results and selecting the adjacent result have automated coverage. Manual desktop acceptance remains pending.

- **Two-step connection dialog acceptance.** Database selection and tabbed configuration share a fixed-size frame with no scrolling form; manual desktop acceptance of spacing, all tab contents and footer visibility remains pending.

- **Execute boundary acceptance.** Automatic whole-script syntax warnings are disabled. Selected SQL and caret-based statement boundaries have automated coverage; manual desktop acceptance remains pending.

- **Provider logo desktop acceptance.** PostgreSQL, MySQL and SQLite now use SVG artwork instead of 16px PNGs. Native SVG rendering and Retina appearance in the picker/navigator await manual GUI acceptance.

- **SQL action rail acceptance.** Vertical icon buttons beside the line numbers, hover explanations, independent scrolling and Execute/Cancel presentation await manual desktop acceptance.

- **Result rail label and icon acceptance.** Explicit label sizing, unmirrored glyph coordinates, left/right reading directions and the grid-and-gear Panels asset have automated coverage; the user confirmed the labels and Panels icon during desktop acceptance.

- **Script result panel acceptance.** Initial result hiding and bounded per-tab vertical resizing have automated coverage; the user confirmed initial hiding, vertical resizing and closing the last result during desktop acceptance.
