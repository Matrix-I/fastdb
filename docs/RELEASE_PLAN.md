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

- `ci/build-macos.sh` builds `FastDB.app` and a `.dmg`, but it has **no ad-hoc signature, no app icon, no `/Applications` symlink** in the DMG, and the version is hard-coded to `0.1.0`.
- `ci/build-linux.sh` builds a `.deb` and an AppImage, also with a hard-coded version. It falls back to a host build, so running it on macOS silently produces a macOS binary inside a `.deb`.
- `.github/workflows/ci.yml` builds and tests on macOS and Linux but publishes nothing.
- There is no `[profile.release]` tuning in the workspace `Cargo.toml`.
- `dist/` is not in `.gitignore`.
- No tags exist yet.

### Release blockers found while reviewing

1. **Saved connections use a relative path.** `crates/app/src/workspace/connections.rs:163` and `:199` read and write `.fastdb_config.json` in the current directory. An app launched from Finder or Spotlight starts with `/` as its working directory, so the write fails (the error is discarded with `let _ =`) and connections are lost on every launch. Move the file under `platform_hal::paths::project_dirs()` (`~/Library/Application Support/...` on macOS, `~/.config/...` on Linux), and migrate an existing `./.fastdb_config.json` once.
2. **Keychain access with an ad-hoc signature.** Passwords live in the macOS keychain under `com.fastdb.credentials`. Keychain item access is tied to the app's code signature; an ad-hoc signature changes with every build, so after each update macOS will ask the user again to allow access. Acceptable for v0.1.0, but say so in the release notes, and plan for a Developer ID certificate later.
3. **No app icon.** The bundle copies `assets/icons/` (UI icons embedded via `rust-embed` anyway) instead of a `FastDB.icns`. Add `resources/FastDB.icns` and `CFBundleIconFile`.

## 3. Target process for FastDB

Keep laplace-terminal's simple manual flow, with two changes: one version source, and Linux built on a real Linux runner instead of a host fallback.

### 3.1 One-time setup

- [ ] Fix blocker 1 (config path).
- [x] Add the icon (blocker 3): `resources/icon.svg` is the source; `resources/FastDB.icns` and `resources/icon.png` are rendered from it.
- [ ] Add to the workspace `Cargo.toml`:
  ```toml
  [profile.release]
  opt-level = 3
  lto = "fat"
  codegen-units = 1
  panic = "abort"
  strip = true
  ```
  Check that `panic = "abort"` does not break any `catch_unwind` in the tokio/GPUI paths before keeping it; drop it if it does.
- [ ] Move `Info.plist` out of the heredoc into `resources/Info.plist` (like laplace), adding `CFBundleIconFile`, `LSMinimumSystemVersion` and `NSPrincipalClass = NSApplication`.
- [ ] In both `ci/build-*.sh`, read the version from Cargo instead of hard-coding it:
  ```bash
  VERSION="$(cargo metadata --no-deps --format-version 1 \
    | jq -r '.packages[] | select(.name == "fastdb-app") | .version')"
  ```
  and stamp it into `Info.plist` with `plutil -replace CFBundleShortVersionString -string "$VERSION"`.
- [ ] `ci/build-macos.sh`: sign ad-hoc (`codesign --force --deep --sign -`), stage the DMG with an `Applications` symlink, also produce `FastDB-$VERSION-macos-arm64.zip`, and write `SHA256SUMS.txt`.
- [ ] `ci/build-linux.sh`: fail instead of falling back when not on Linux; add `fastdb-$VERSION-linux-x86_64.tar.gz` next to the `.deb` and AppImage.
- [ ] Add `/dist` to `.gitignore`.
- [ ] Add a `.github/workflows/release.yml` triggered on `v*` tags that runs the Linux script on `ubuntu-latest` (same apt packages as `linux-app` in `ci.yml`) and attaches its artifacts to the draft release. macOS stays a local build for now, as in laplace, since there is no signing certificate to put in CI.
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
- Homebrew cask and an in-app update check.
