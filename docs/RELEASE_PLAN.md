# FastDB Release Plan (v0.1.0)

This plan adapts the release process used by
[laplace-terminal](https://github.com/Matrix-I/laplace-terminal) to FastDB.

## 1. How laplace-terminal releases

| Step | What laplace-terminal does |
|---|---|
| Version source | `Cargo.toml` `version` plus `CFBundleShortVersionString` / `CFBundleVersion` in a checked-in `resources/Info.plist`, bumped by hand |
| Build | `./bundle_macos.sh` runs `cargo build --release` (`lto = "fat"`, `codegen-units = 1`, `panic = "abort"`, `strip = true`) |
| macOS bundle | Copies the binary, `Info.plist` and `Laplace.icns` into `Laplace.app`, then signs it ad-hoc with `codesign --force --deep --sign -` |
| Local install | Copies the app into `~/Applications` for a quick smoke test |
| Artifacts | `Laplace.dmg` (with an `/Applications` symlink for drag-and-drop), `Laplace-macos.zip`, the raw `laplace-macos` binary, `laplace-linux` and `laplace-linux-x86_64.tar.gz` |
| Checksums | `dist/SHA256SUMS.txt` covering every artifact |
| Tag | Annotated tag `vX.Y.Z` with a one-line summary, e.g. `Release v0.3.0 - mouse reporting, scrollback reflow, ...` |
| Publish | GitHub Release on the tag with all artifacts attached; the README links to `releases/latest` |
| Gatekeeper | No notarization; the README tells users to run `xattr -dr com.apple.quarantine /Applications/Laplace.app` |
| CI | None for releases; everything runs locally |

## 2. Where FastDB stands today

FastDB already has most of the pieces, but they need work before a public build:

- `ci/build-macos.sh` builds `FastDB.app` and a `.dmg` with an ad-hoc signature and app icon. The DMG still has **no `/Applications` symlink**, and the version is hard-coded to `0.1.0`.
- `ci/build-linux.sh` builds a `.deb` and an AppImage, also with a hard-coded version. It falls back to a host build, so running it on macOS silently produces a macOS binary inside a `.deb`.
- `.github/workflows/ci.yml` builds and tests on macOS and Linux but publishes nothing.
- The workspace `Cargo.toml` has release tuning with thin LTO, one codegen unit, abort-on-panic and stripped symbols.
- `/dist` is in `.gitignore`.
- No tags exist in the local checkout.

Status checked against repository code on 2026-11-01; runtime smoke tests and published release state have not been verified.

### Release blockers found while reviewing

1. **Resolved: saved connection path.** Connections now use `platform_hal::paths::config_dir()/connections.json`, with migration from a legacy `./.fastdb_config.json`.
2. **Saved passwords are still plaintext.** OS keychain helpers exist under `com.fastdb.credentials`, but connection load/save does not call them. Wire up credential storage and migrate existing plaintext passwords before claiming keychain-backed storage. Once integrated, verify access across ad-hoc signed updates and document any re-prompts; Developer ID signing remains a later goal.
3. **Resolved: app icon.** The macOS bundle copies `resources/FastDB.icns` and sets `CFBundleIconFile`.

## 3. Target process for FastDB

Keep laplace-terminal's simple manual flow, with two changes: one version source, and Linux built on a real Linux runner instead of a host fallback.

### 3.1 One-time setup

- [x] Fix blocker 1 (config path): connections now live in `platform_hal::paths::config_dir()/connections.json`, and a legacy `./.fastdb_config.json` is moved there on first load.
- [x] Add the icon (blocker 3): `resources/icon.svg` is the source; `resources/FastDB.icns` and `resources/icon.png` are rendered from it.
- [x] Add release tuning to the workspace `Cargo.toml` (implemented with thin LTO):
  ```toml
  [profile.release]
  opt-level = 3
  lto = "thin"
  codegen-units = 1
  panic = "abort"
  strip = true
  ```
- [ ] Verify that `panic = "abort"` does not break required panic recovery in the tokio/GPUI paths; drop it if it does.
- [ ] Integrate keychain-backed connection passwords and migrate plaintext credentials (blocker 2).
- [ ] Move `Info.plist` out of the heredoc into `resources/Info.plist` (like laplace), preserving the existing `CFBundleIconFile` and adding `LSMinimumSystemVersion` and `NSPrincipalClass = NSApplication`.
- [ ] In both `ci/build-*.sh`, read the version from Cargo instead of hard-coding it:
  ```bash
  VERSION="$(cargo metadata --no-deps --format-version 1 \
    | jq -r '.packages[] | select(.name == "fastdb-app") | .version')"
  ```
  and stamp it into `Info.plist` with `plutil -replace CFBundleShortVersionString -string "$VERSION"`.
- [x] `ci/build-macos.sh`: sign ad-hoc (`codesign --force --deep --sign -`).
- [ ] `ci/build-macos.sh`: stage the DMG with an `Applications` symlink, also produce `FastDB-$VERSION-macos-arm64.zip`, and write `SHA256SUMS.txt`.
- [ ] `ci/build-linux.sh`: fail instead of falling back when not on Linux; add `fastdb-$VERSION-linux-x86_64.tar.gz` next to the `.deb` and AppImage.
- [x] Add `/dist` to `.gitignore`.
- [ ] Add a `.github/workflows/release.yml` triggered on `v*` tags that runs the Linux script on `ubuntu-latest` (same apt packages as `linux-app` in `ci.yml`) and attaches its artifacts to the draft release. macOS stays a local build for now, as in laplace, since there is no signing certificate to put in CI.
- [ ] Add a `LICENSE` file matching the workspace MIT declaration.
- [ ] README: add an Installation section with the `xattr -dr com.apple.quarantine /Applications/FastDB.app` notice and links to `releases/latest`.

### 3.2 Per-release checklist

1. **Freeze**: `main` is green in CI (macOS build & test, Linux build, live-database tests).
2. **Bump** `version` in the workspace `Cargo.toml`; commit `chore(release): vX.Y.Z`.
3. **Smoke test** on macOS from the built `.app` launched from Finder (not `cargo run`, so the working directory is `/`):
   - connect to PostgreSQL, MySQL and SQLite; restart and check the saved connections are still there;
   - check the keychain prompt appears once and the password is remembered;
   - run a large query and scroll the result grid.
4. **Tag**: `git tag -a vX.Y.Z -m "Release vX.Y.Z - <one-line summary>"` and push the tag.
5. **Build**: `./ci/build-macos.sh` locally; the `release.yml` workflow builds Linux on the tag push.
6. **Publish**: create the GitHub Release on the tag, attach the macOS artifacts and `SHA256SUMS.txt`, check the Linux artifacts from the workflow, and write the notes from `git log <prev-tag>..vX.Y.Z`. Include the Gatekeeper and keychain notices.
7. **Verify** by downloading the DMG from the release page on a clean user account and installing it.

## 4. Artifacts for v0.1.0

| Platform | File |
|---|---|
| macOS (Apple Silicon) | `FastDB-0.1.0-macos-arm64.dmg`, `FastDB-0.1.0-macos-arm64.zip` |
| Linux (x86_64) | `fastdb_0.1.0_amd64.deb`, `FastDB-0.1.0-x86_64.AppImage`, `fastdb-0.1.0-linux-x86_64.tar.gz` |
| All | `SHA256SUMS.txt` |

Intel Macs are out of scope for v0.1.0; a universal binary (`lipo` over `x86_64-apple-darwin` and `aarch64-apple-darwin`) can come later.

## 5. Later

- Developer ID signing and notarization (`notarytool`), which also fixes the keychain re-prompt and removes the `xattr` step. Requires an Apple Developer account.
- Build macOS in `release.yml` once signing secrets exist.
- [x] In-app auto-update checker and installer downloader (GitHub Releases API).
- Homebrew cask.
