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
cargo run -p fastdb-app     # run the app (binary: fastdb)
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

## Implementing

- For a feature that spans several crates or more than a few files, write a plan first in `docs/plans/YYYY-MM-DD-<topic>.md` and commit it on its own (`docs: add <topic> implementation plan`).
- Add or update tests in the crate that owns the logic. Tests that need a live database must skip cleanly when the `FASTDB_TEST_*` variables are unset (CI sets `FASTDB_REQUIRE_DB_TESTS=1` to make them mandatory).
- Before calling work done: `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo test --workspace`, and run the app for UI changes.
- When user-visible behavior changes, update `README.md` ("What works today" / "Not built yet") in the same change; update `docs/PROVIDERS.md` for provider changes.

## Committing

- Conventional Commits: `<type>(<scope>): <summary>`.
  - Types: `feat`, `fix`, `docs`, `build`, `chore`, `refactor`, `test`, `perf`.
  - Scope is the crate name without the path (`app`, `db-wire`, `ui-grid`, `sql-intel`, ...); omit it for workspace-wide changes.
  - Summary in lowercase imperative mood, describing the behavior, not the code (`fix(ui-grid): fit columns to cells measured with the window's fonts`).
- One logical change per commit; keep docs-only and build-only changes in their own commits.
- Commit only the files that belong to the change; do not sweep unrelated working-tree edits into it.
- Commit or push only when asked. Never force-push `main`.

## Releasing

Follow the per-release checklist in `docs/RELEASE_PLAN.md` (section 3.2). In short:

1. `main` is green in CI.
2. Bump `version` in the root `Cargo.toml` (`[workspace.package]`); commit `chore(release): vX.Y.Z`.
3. Smoke test the built `.app` launched from Finder.
4. Tag with `git tag -a vX.Y.Z -m "Release vX.Y.Z - <summary>"` and push the tag. The in-app updater reads GitHub Releases and parses the tag as semver, so tags must be `vX.Y.Z`.
5. Build with `./ci/build-macos.sh` (and `./ci/build-linux.sh`), then publish the GitHub Release with the artifacts and `SHA256SUMS.txt`. Write the notes with the template in `docs/RELEASE_PLAN.md` (section 3.3), changes taken from `git log <prev-tag>..vX.Y.Z`. Every release must include the macOS Gatekeeper note (`xattr -dr com.apple.quarantine /Applications/FastDB.app`) until the app is notarized.

The `release.yml` workflow named in the release plan does not exist yet; Linux artifacts are built manually for now.
