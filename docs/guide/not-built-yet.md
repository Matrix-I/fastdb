# Not built yet

The [audited roadmap](../ROADMAP.md) separates working paths, partial features, UI-only controls and missing workflows. Major gaps include:

- **Title bar zoom acceptance.** Double-clicking the title or empty top bar toggles window maximize/restore; manual desktop acceptance, including tab and button click isolation, remains pending.

- **Deferred desktop providers for v0.2.0.** MariaDB, Oracle, SQL Server and Db2 connection choices are disabled with **Feature is coming**; existing profiles cannot Test/Connect. PostgreSQL is the current release acceptance gate; MySQL and SQLite remain selectable.
- **Data transfer UI gaps.** The backend supports streaming table/query transfer and configurable CSV mapping/encoding/NULL tokens, but the desktop app has no transfer entry point or manual mapping/encoding/NULL-token controls. CSV import currently uses automatic name-based mapping, UTF-8 and the default `\N` NULL token. Full-source export also lacks an exposed cancellation control and live row progress. These P1-08 GUI gaps require implementation or an explicit release scope deferral.

- **Additional Properties mutations.** Virtual keys/relationships are not implemented. PostgreSQL catalog sections are read-only; other providers explicitly show unsupported sections.
- **Advanced column editing.** PostgreSQL ordinary-column drafts, preview/Persist and Revert are implemented; identity/generated/storage/collation/permissions transitions and other-provider mutation support remain open. The [GUI/business analysis](../DBEAVER_WORKFLOWS.md) specifies this workflow and broader screen coverage.
- **Core DBeaver workflows.** ER diagram export (the focused four-sided layout, saved table positioning, whole-card dragging, content-fit card widths, separate connector lanes, clickable arrows and mapped-column highlights without PK/FK text badges or column data types await manual GUI acceptance), schema-aware XML editing, backup/restore, and session/lock monitoring remain open (CSV data import, streaming table transfer, full-source export, workspace session restoration and preferences are implemented; desktop acceptance pending). See the roadmap for scope and acceptance criteria.
- **Advanced grid productivity.** Loaded-result Find, Record, column display controls, multiple sort keys, validated plain TSV paste, and explicit NULL/default actions are available. FK navigation opens referenced or referencing rows in a filtered tab. PostgreSQL supports scalar non-key updates through Load editable result for a verified direct SELECT with a complete primary key. Broader SQL forms/providers, query-result row insertion/deletion and desktop acceptance remain open.
- **Execution feedback limits.** Execution logs and timeout choices live only in open SQL tabs. ODBC informational notices and structured error positions are not exposed; MySQL/MariaDB positions are limited to native error text. Desktop acceptance of Execution log/Timeout is pending manual verification.
- **Advanced script execution.** Transaction chaining remains unsupported for PostgreSQL. Dedicated ODBC tabs, supported database/schema selectors and vendor batch delimiters are implemented with automated fixtures. Live ODBC parity, separate MariaDB validation and desktop acceptance remain open.
- **SSH coverage.** PostgreSQL, MySQL, and MariaDB have fingerprint-verified SSH forwarding. Separate MariaDB validation and desktop acceptance remain to be verified.
- **Additional metadata for other providers.** PostgreSQL Properties has catalog metadata and statistics; other providers label unsupported fields as unavailable. PostgreSQL row counts are estimates, and partition-parent sizes are unavailable rather than a misleading zero or a sum of partitions. PostgreSQL DDL uses catalog definitions for tables and views (see provider documentation for scope); other providers retain a partial preview.
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
