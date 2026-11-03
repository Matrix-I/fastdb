# Not built yet

The [audited roadmap](../ROADMAP.md) separates working paths, partial features, UI-only controls and missing workflows. Major gaps include:

- **Additional Properties mutations.** Virtual keys/relationships are not implemented. PostgreSQL catalog sections are read-only; other providers explicitly show unsupported sections.
- **Advanced column editing.** PostgreSQL ordinary-column drafts, preview/Persist and Revert are implemented; identity/generated/storage/collation/permissions transitions and other-provider mutation support remain open. The [GUI/business analysis](../DBEAVER_WORKFLOWS.md) specifies this workflow and broader screen coverage.
- **Core DBeaver workflows.** ER diagram export/manual positioning, object create/alter/drop UI, richer object navigation/search, SQL find/replace and parameter prompts, data import/transfer, structured value editing, backup/restore, session/lock monitoring and workspace restoration remain open. See the roadmap for scope and acceptance criteria.
- **Advanced script execution.** Transaction chaining, pinned sessions/transaction scripts for other providers, and vendor procedural/batch syntax remain unsupported. PostgreSQL supports persistent tab sessions, transaction controls and SQL savepoints.
- **SSH tunnels.** `ssh_tunnel.rs` is a stub: it does not forward traffic or verify host keys.
- **Additional metadata for other providers.** PostgreSQL Properties has catalog metadata and statistics; other providers label unsupported fields as unavailable. PostgreSQL row counts are estimates, and partition-parent sizes are unavailable rather than a misleading zero or a sum of partitions. PostgreSQL DDL uses catalog definitions for tables and views (see provider documentation for scope); other providers retain a partial preview.
- **Light theme.** A light palette exists in `ui-core`, but there is no way to switch to it.
- **Performance target validation.** The desktop regression suite measures startup, RSS, a million-row SQLite stream, scrolling and release executable size; cold-cache startup, live-provider latency and the original target claims still need separate measurements.
