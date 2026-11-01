# Auto Update Implementation Plan

**Goal:** Implement an in-app Auto Update feature for FastDB that checks GitHub Releases for new versions, notifies users via a native GPUI dialog with release notes, supports downloading updates directly with a progress bar, and integrates with the macOS menu and top bar.

**Architecture:**
- `updater.rs`: Core update logic (GitHub API querying, semver comparison, platform asset matching, state persistence in `<config_dir>/updater_state.json`, streaming download with progress, and opening installers).
- `update_dialog.rs`: GPUI modal overlay rendering the update state (checking, up-to-date, update available with release notes & download progress bar, error state).
- `Workspace` integration: Throttled background check on startup (at most once every 24h), manual check via menu `FastDB -> Check for Updates...`, top-bar update indicator badge, keyboard Esc handler.

**Tech Stack:** Rust, GPUI 0.2.2, `zed-reqwest` (HTTP/TLS/streaming), `semver 1.0`, `serde_json`, `platform-hal::paths`.

## Global Constraints
- Code, comments, identifiers in English matching surrounding codebase style.
- No git commits ("không commit" requested by user).
- Tests must use isolated temporary directories for filesystem operations.
- Handle networking errors gracefully without crashing or blocking the UI thread.
