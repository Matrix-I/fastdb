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

- `ci/build-macos.sh` builds `FastDB.app` and a `.dmg` with an ad-hoc signature, app icon and `/Applications` symlink. The version is still hard-coded to `0.1.0`; ZIP and checksum generation remain to be added to the script.
- `ci/build-linux.sh` builds a `.deb` and an AppImage, also with a hard-coded version. It falls back to a host build, so running it on macOS silently produces a macOS binary inside a `.deb`.
- `.github/workflows/ci.yml` builds and tests on macOS and Linux but publishes nothing.
- The workspace `Cargo.toml` has release tuning with thin LTO, one codegen unit, abort-on-panic and stripped symbols.
- `/dist` is in `.gitignore`.
- The local checkout contains tag `v0.1.0`.

Status checked against repository code on 2026-11-02; runtime smoke tests and published release state have not been verified.

### Release blockers found while reviewing

1. **Resolved: saved connection path.** Connections now use `platform_hal::paths::config_dir()/connections.json`, with migration from a legacy `./.fastdb_config.json`.
2. **Implemented in source: keychain-backed passwords.** Connection load/save uses `com.fastdb.credentials` and migrates existing plaintext config after successful secure storage. Verify access across ad-hoc signed updates and document any re-prompts before release; Developer ID signing remains a later goal. The published v0.1.0 preview predates this integration.
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
- [x] Integrate keychain-backed connection passwords and migrate plaintext credentials (blocker 2).
- [ ] Verify keychain access across ad-hoc signed updates and document any re-prompts.
- [ ] Move `Info.plist` out of the heredoc into `resources/Info.plist` (like laplace), preserving the existing `CFBundleIconFile` and adding `LSMinimumSystemVersion` and `NSPrincipalClass = NSApplication`.
- [ ] In both `ci/build-*.sh`, read the version from Cargo instead of hard-coding it:
  ```bash
  VERSION="$(cargo metadata --no-deps --format-version 1 \
    | jq -r '.packages[] | select(.name == "fastdb-app") | .version')"
  ```
  and stamp it into `Info.plist` with `plutil -replace CFBundleShortVersionString -string "$VERSION"`.
- [x] `ci/build-macos.sh`: sign ad-hoc (`codesign --force --deep --sign -`).
- [x] `ci/build-macos.sh`: stage the DMG with an `Applications` symlink.
- [ ] `ci/build-macos.sh`: produce `FastDB-$VERSION-macos-arm64.zip` and write `SHA256SUMS.txt`.
- [x] `ci/build-linux.sh`: fails when not on Linux, reads the version from `Cargo.toml` and packages the `.deb` through `ci/package-deb.sh`. It writes `fastdb-linux-x86_64` next to the `.deb` and AppImage; no tarball.
- [x] Add `/dist` to `.gitignore`.
- [x] Add `.github/workflows/release.yml` (2026-11-09, modeled on litty-terminal): a `vX.Y.Z` or `vX.Y.Z-rc.N` tag builds the macOS bundle on `macos-15` and the Linux binary and `.deb` on `ubuntu-24.04`, installs the `.deb` in a clean container, signs `SHA256SUMS.txt` with minisign in the `release` environment and creates a draft release. See [the plan](plans/2026-11-09-release-workflow.md).
- [x] Add a `LICENSE` file matching the workspace MIT declaration.
- [x] README: add macOS preview installation instructions with the `xattr -dr com.apple.quarantine /Applications/FastDB.app` notice and a link to `releases/latest`.

### 3.2 Per-release checklist

1. **Freeze**: `main` is green in CI (macOS build & test, Linux build, live-database tests).
2. **Bump** `version` in the workspace `Cargo.toml`; commit `chore(release): vX.Y.Z`.
3. **Smoke test** on macOS from the built `.app` launched from Finder (not `cargo run`, so the working directory is `/`):
   - connect to PostgreSQL, MySQL and SQLite; restart and check the saved connections are still there;
   - check the keychain prompt appears once and the password is remembered;
   - run a large query and scroll the result grid.
4. **Tag**: `git tag -a vX.Y.Z -m "Release vX.Y.Z - <one-line summary>"` and push the tag. The tag must equal the `Cargo.toml` version and point at a commit on `main`. For a release candidate, set `X.Y.Z-rc.N` in `Cargo.toml` and tag `vX.Y.Z-rc.N`; it becomes a draft prerelease.
5. **Build**: the `release.yml` workflow runs on the tag push. It checks that the tag matches `Cargo.toml`, builds and tests the macOS and Linux assets, signs `SHA256SUMS.txt` after a reviewer approves the `release` environment, and creates a draft release with all assets.
6. **Publish**: review the draft release and its assets, and replace the generated notes with the template in 3.3, changes taken from `git log <prev-tag>..vX.Y.Z`. The Gatekeeper note is required in every release until the app is notarized. Then publish the draft.
7. **Verify** by downloading the DMG from the release page on a clean user account and installing it.

### 3.3 Release notes template

Replace `X.Y.Z` and fill in the changes; keep the Gatekeeper section verbatim.

````markdown
## What's new

- <user-visible change, from `git log <prev-tag>..vX.Y.Z`>

## Downloads

| Platform | File |
|---|---|
| macOS (Apple Silicon) | `FastDB-X.Y.Z-macos-arm64.dmg` or `FastDB-X.Y.Z-macos-arm64.zip` |
| Linux (x86_64) | `fastdb_X.Y.Z_amd64.deb`, `FastDB-X.Y.Z-x86_64.AppImage` or `fastdb-X.Y.Z-linux-x86_64.tar.gz` |

Verify downloads against `SHA256SUMS.txt`.

## ⚠️ macOS Gatekeeper Note (Apple Silicon)

Because FastDB is an independent open-source project and not yet notarized with an Apple Developer Account, macOS Gatekeeper will block the application on first launch.

After copying `FastDB.app` into `/Applications`, open your terminal and run:

```bash
xattr -dr com.apple.quarantine /Applications/FastDB.app
```

Then launch FastDB normally from Spotlight, Launchpad, or the Applications folder.
````

For releases containing keychain integration, include a keychain note: ad-hoc signed updates may make macOS ask again for access to the saved passwords.

## 4. Artifacts for v0.1.0

| Platform | File |
|---|---|
| macOS (Apple Silicon) | `FastDB-0.1.0-macos-arm64.dmg`, `FastDB-0.1.0-macos-arm64.zip` |
| Linux (x86_64) | `fastdb_0.1.0_amd64.deb`, `FastDB-0.1.0-x86_64.AppImage`, `fastdb-0.1.0-linux-x86_64.tar.gz` |
| All | `SHA256SUMS.txt` |

Intel Macs are out of scope for v0.1.0; a universal binary (`lipo` over `x86_64-apple-darwin` and `aarch64-apple-darwin`) can come later.

## 5. Later

- Developer ID signing and notarization (`notarytool`), which also fixes the keychain re-prompt and removes the `xattr` step. Requires an Apple Developer account.
- Developer ID signing in `release.yml` once an Apple certificate exists; the workflow currently signs ad-hoc.
- [x] In-app auto-update checker and installer downloader (GitHub Releases API).
- Homebrew cask.

## v0.1.0 preview release decision (2026-11-01)

The first release is an early preview for macOS Apple Silicon. In that release, keychain integration
was deferred with an explicit plaintext-password disclosure in installation instructions
and release notes. It is now implemented in source; cross-update keychain access still needs release validation.
Linux artifacts and release automation are deferred until built and verified on Linux.
The local `bundle_macos.sh --no-install` flow supplies the app, DMG with Applications
shortcut, and ZIP. Release assets are copied to versioned filenames and checksummed.
The remaining one-time setup checklist above tracks future packaging work, not a
claim that every item has shipped in this preview.
