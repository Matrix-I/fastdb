# FastDB

Native Rust database client (GPUI UI, no JVM/JDBC) aiming to replace tools like DBeaver. Early prototype: see `README.md` for what works and what is not built yet, and `docs/PROVIDERS.md` for provider setup and live tests.

## Workspace layout

Cargo workspace (edition 2024, toolchain pinned in `rust-toolchain.toml`):

| Crate | Purpose |
| :--- | :--- |
| `crates/app` | Binary: GPUI workspace (`src/workspace/`), connection dialog, updater, embedded assets |
| `crates/db-wire` | Database drivers behind common traits: `postgres`, `mysql`, `sqlite`, `odbc` (Db2/Oracle/SQL Server), SSH tunnel, streaming, export |
| `crates/mem-engine` | Paged result store: LRU page cache, slotted pages, LZ4 disk spill |
| `crates/platform-hal` | Paths, keychain, keymap, Unix socket discovery |
| `crates/sql-intel` | SQL parsing (tree-sitter), aliases, completion, schema cache |
| `crates/ui-core` | Actions, history, theme |
| `crates/ui-editor` | SQL text editor (ropey buffer + view) |
| `crates/ui-grid` | Virtualized result grid, selection, change set for inline edits |

`vendor/xattr` is a patched crate (not a workspace member); see the comment in the root `Cargo.toml` before touching it.

## Commands

```bash
cargo build                 # dev build (deps optimized, own crates lightly)
cargo run -p app            # run the app
cargo test --workspace      # unit tests
cargo clippy --workspace --all-targets
cargo fmt --all
./bundle_macos.sh           # local macOS .app bundle
```

CI configuration lives in `ci/` and `.github/workflows/`. Live provider tests (`crates/db-wire/tests/provider_test.rs`) need real databases or vendor ODBC drivers; see `docs/PROVIDERS.md`.

## Conventions

- Code, comments, identifiers, commit messages and docs are written in English.
- Drivers stream rows into `mem-engine` as batches arrive; never collect a full result set in memory.
- Keep UI work off the render path: GPUI re-renders on every key press or hover, so avoid heavy work in `render`.
- New database support goes through the `db-wire` traits (`traits.rs`) and a provider preset in the connection dialog, not special cases in the workspace code.
- Match the surrounding code's style and comment density; run `cargo fmt` and `cargo clippy` before committing.
- Saved connection passwords are currently stored in plain text (README, "Connections"); do not log connection details.
