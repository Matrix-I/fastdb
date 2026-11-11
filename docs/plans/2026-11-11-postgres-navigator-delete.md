# PostgreSQL Navigator Delete Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: development-superpowers:subagent-driven-development or executing-plans-strict. Steps use `- [ ]`.

**Goal / Architecture / Tech Stack / Branch** — Add PostgreSQL Delete entries to the database and schema menus. Use a workspace-owned confirmation dialog and the existing driver factory and DDL executor. Rust, GPUI and Tokio. Work in the current checkout without committing.

## Confirmed Requirements

- PostgreSQL only.
- Schema deletion offers explicit CASCADE, with RESTRICT as the initial mode.
- Database deletion requires the exact database name, uses a separate maintenance database and keeps the saved profile after disconnecting. PostgreSQL reports sessions that prevent deletion. Do not force-terminate them.

## Global Constraints

- Keep existing menu actions and reuse the current provider, read-only and DDL conventions.
- No public API, persisted format, dependency or migration change. Do not run destructive SQL against a shared database.
- Verify with `cargo fmt --all`, `cargo clippy --offline --workspace --all-targets`, `cargo test --offline --workspace --no-fail-fast` and `cargo build --offline -p fastdb-app`. Launch the app. The user performs desktop acceptance.
- Commit only when explicitly asked.

## Technical Defaults

- Use the live PostgreSQL configuration with only the database changed to `postgres` (or `template1` when deleting `postgres`). Preserve SSH, TLS, credentials, timeouts and the server address.
- Do not issue DDL on a stale or replaced session. Reject a drop while the owning tabs have staged edits, an open transaction or a tracked SQL operation in flight. Open the maintenance connection before releasing the owning session.
- Quote names with the existing `SqlDialect::quote_ident`. The confirmation input cannot change the SQL target.

## Risks & Rollback

- DROP is irreversible after PostgreSQL accepts it. Preview the SQL, identify the exact target, require confirmation and block concurrent submissions. A failed DROP reports the server error. If the target session was released, the retained profile can reconnect.
- Reverting the change removes the menu entries and dialog. It cannot restore objects already removed by the server.

## Task 1 — Confirmation and execution

Files: `crates/app/src/workspace/{nav_context_menu,mod,keyboard,connection_state,connections,script_saves,tab_state}.rs`, `crates/app/src/workspace/navigator/connection.rs` and new `crates/app/src/workspace/drop_object_modal.rs`. Consumes `ConnectionConfig`, `DatabaseDriver::execute_schema_ddl`, `Workspace::disconnect_by_id_now` and `Workspace::refresh_connection_for`. Produces the app-private `DropTarget` and `DropObjectModal`.

- [x] Add tests for exact database confirmation, quoted identifiers, RESTRICT and CASCADE previews, and separate maintenance database routing. Run `cargo test --offline -p fastdb-app drop_object_modal::tests` and expect the new interfaces to be missing before implementation.
- [x] Wire Delete actions on connected writable PostgreSQL database and schema rows. Render confirmation, SQL preview, cancel, and disabled execution while a request is running. Execute schema DROP on the captured driver and refresh metadata on success. Open a maintenance driver for database DROP, release the owning sessions, run DROP and retain the profile.
- [x] Re-run the focused suite. Ready to commit the implementation and guide as `feat(app): delete PostgreSQL databases and schemas from the navigator`.

## Verification & Docs

- [ ] Update `docs/guide/navigator-and-tables.md` and `docs/guide/not-built-yet.md`. Inspect the complete diff, run the workspace verification commands, and get an independent review. Re-run the final gate after any change.
- [ ] On an isolated PostgreSQL server, exercise a schema with a dependent table and a disposable database. Verify RESTRICT fails, CASCADE removes the dependent table, a wrong confirmation sends no request, a second session blocks database deletion, and the target database stops accepting connections after a successful drop. During desktop acceptance, confirm that the saved profile remains listed and disconnected. Report live and GUI gaps.
