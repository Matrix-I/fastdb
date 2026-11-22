# Verify In-App Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: executing-plans-strict. Tasks are `### Task N: <name>` and steps use `- [ ]`.

**Goal:** Complete P3-05 by authenticating every downloaded update before FastDB opens or installs it.
**Architecture:** Bind the selected asset to the exact filename and SHA-256 in the same release's authenticated checksum manifest. Stream the package into a temporary file. Only publish a file whose digest matches. Carry the verified digest into the installer state and check the file again on each launch.
**Tech stack:** Rust, Reqwest/Tokio, tempfile, Minisign, RustCrypto SHA-256, GPUI.
**Branch:** `fix/verify-updates` from `6952025` on `origin/main`.

## Confirmed Requirements

- P3-05 requires the published manifest and signature, verification with FastDB's trusted public key, and exact digest matching before every auto-install and local manual-open fallback. Missing or invalid metadata or mismatched bytes fail closed, with retry available.
- The user approved adding `minisign-verify` and declaring the already locked `sha2` directly. The user approved Codex models as substitutes for the Claude model tiers in the project skills.

## Decision Log

- Use the two approved libraries for in-app verification. This avoids an operating-system package prerequisite and leverages the existing release signature format. Approved by the user in chat. Not posted because no issue or PR comment was authorized.
- Apply the existing P3-05 release contract to older releases too. Missing signed metadata blocks in-app installation. Not posted because the behavior is already specified in the architecture roadmap.

## Scope Map

**Invariant:** FastDB hands bytes to a local update installer or opener only after the selected filename and the current file bytes match a manifest authenticated by the embedded release key.

| Path | Scope |
| --- | --- |
| [Release workflow](../../.github/workflows/release.yml) and [published key](../../resources/minisign.pub) | Input contract. Consume the two existing release assets by exact name. Embed the repository key. No format or signing changes. |
| [Updater](../../crates/app/src/updater.rs), `download_asset_file`, `apply_update_file`, `open_installer_file`, all DMG/ZIP/Linux fallbacks | Task 1. Authenticate metadata, stage and hash bytes, and gate all native actions. |
| [Update tasks](../../crates/app/src/workspace/software_update/tasks.rs), `state.rs`, `mod.rs` and `download_view.rs` | Task 1. Pass verified proof through download completion and installation. Gate both Open Installer and Show in Folder, which currently also invokes the opener. |
| [Update status views](../../crates/app/src/workspace/software_update/status_views.rs) | Task 1. Keep View on GitHub as browser navigation by deriving the destination from the configured repository and validated tag. Never pass a release API's unchecked URL to the operating system. |
| [Update guide](../guide/updates.md), [acceptance gaps](../guide/not-built-yet.md), [architecture roadmap](2026-11-03-codebase-architecture-improvements.md#phase-3--architecture-and-updater-verification) | Task 2. Document verification, errors, and outstanding desktop acceptance. |

## Existing Data & States

- A legacy release can appear in version checks and remain viewable via GitHub. Upgrade fails clearly when the manifest or signature is absent. Download Retry repeats verification from scratch.
- Existing files in Downloads carry no verified proof. An invalid update leaves an earlier file untouched. A file changed or removed after a valid download fails re-verification on either automatic or manual action.
- Browser-only release metadata remains viewable. Update preferences, skipped-version state, semantic version comparison, platform asset selection, and native installer behavior retain their existing contracts.
- Concurrent FastDB processes can hold separate verified macOS downloads. Each install gets its own extraction directory or mount point, and automatic bundle writes are exclusive. A release can contain multiple platform assets. Exactly one entry must match the selected filename. A digest belonging to a different filename, ambiguous metadata, and a traversal-like asset name are rejected.

## Global Constraints

- Follow the project AGENTS.md and CLAUDE.md. Keep UI work off the render path. Commit or push only when asked. Never force-push main.
- Do not use Computer Use or request Computer Use permissions. The user verifies the GUI manually. Complete automated checks and report manual GUI acceptance as pending.
- Use test-first regression evidence. Use a real loopback HTTP server and temporary files, fixture public material only, and fake only the native launch boundary. Run the full workspace formatter, Clippy, build, typecheck and test suite. Run a Linux app smoke launch. Keep the runnable binary and reusable build dependencies.

## Technical Defaults

- Pin `minisign-verify = "=0.3.0"` (MIT, no transitive dependencies) and `sha2 = "=0.10.9"` (MIT OR Apache-2.0, already locked). Use upstream signature verification and SHA-256. No runtime executable prerequisite.
- Authenticate the original manifest bytes before parsing. Require exact filenames with a single basename component. Accept `sha256sum`'s text and binary markers. Reject duplicate matching lines and malformed digests. Use `PublicKey::decode` and `Signature::decode` with the repository key and `verify(..., false)` for the release workflow's prehashed signatures.
- Use explicit request timeouts and bounded signature, manifest and package sizes. Stream the verified package instead of holding it in memory. Stage with a unique same-directory `NamedTempFile` and publish atomically only on success. Do not surface a partial file as an installer.
- Treat the real operating-system file/user boundary as the same as the existing updater. Recheck regular-file contents on the background executor immediately before handing off a path. Do not claim protection against an attacker with write access to the user's files racing the operating-system process.

## Risks & Rollback

- Unsigned or incompletely published releases will fail closed. The error identifies unavailable or invalid verification material and the existing browser release link remains available.
- No persisted configuration or release format changes. Revert the isolated patch if necessary. Live Linux and macOS installer acceptance is separate from loopback and clean launch verification.

### Task 1: Authenticate the download and gate every installer action

**Files:** `crates/app/Cargo.toml:11`, `Cargo.lock`, `crates/app/src/updater.rs:120`, `:477`, `:606`, `:667`, `:1014`; new child module under `crates/app/src/updater/` for verification and fixture(s); `crates/app/src/workspace/software_update/{state.rs,mod.rs,tasks.rs,download_view.rs,status_views.rs}`.

**Consumes:** `ReleaseAsset`, `ReleaseInfo.assets`, matched asset, download directory, trusted public key, Reqwest client, progress sender. **Produces:** `DownloadProgressEvent::Finished { installer: VerifiedInstaller }`, an opaque cloneable proof carrying the final path and digest, and an explicit verification error on failure. **Test levels:** unit (manifest/selection), integration (plain executor, loopback HTTP, real signatures and filesystem), and launch-boundary regression.

- [x] Run baseline `cargo test --offline --locked --workspace` before changing app code. Use native loopback/spill access if sandboxing prevents the existing integrations.
- [x] Create a deterministic referenced fixture for tests: an ephemeral Minisign keypair, artifact bytes, valid signed manifest for the fixture asset, and a second valid signed manifest without the selected entry. Keep only public key, signature and manifest bytes. Reuse the existing loopback test in `updater.rs`, adapting it to serve the manifest, signature, and platform-independent artifact by URL path. Exercise the same background download entry point without a caller Tokio runtime, with fixture-key injection restricted to the internal test seam. Assert exact persisted bytes on success and a verified event. For missing manifest or signature (HTTP 404), invalid signature, mismatched key, absent exact filename and mismatched binary, assert `Failed` with the relevant verification error, no verified event, no staged-file residue, and an existing destination file left byte-for-byte unchanged. Add a real-file launch test that proves a valid downloaded file can reach the injected native action and a replaced, symlinked, or deleted file cannot. Run `cargo test -p fastdb-app updater::` and record the regression failure on the unverified implementation.
- [x] Add these executable regressions to the existing updater tests. Run `cargo test -p fastdb-app updater::` before introducing the three missing helpers. Expect E0425 for `parse_asset_checksum`, `release_metadata_asset` and `validate_asset_name`; the browser regression also expects the new `release_page_url` helper. Use `use super::verification::{parse_asset_checksum, release_metadata_asset, validate_asset_name};` after adding the verification module. The HTTP integration in the preceding step must also cover rejected metadata at the download boundary and preserve a pre-existing destination.

  ```rust
  #[test]
  fn signed_checksum_uses_one_exact_safe_filename() {
      let zeros = "00".repeat(32);
      let lines = format!("{zeros}  other.dmg\n{zeros} *FastDB.dmg\n");
      assert_eq!(parse_asset_checksum(lines.as_bytes(), "FastDB.dmg").unwrap(), [0; 32]);
      assert!(parse_asset_checksum(lines.as_bytes(), "fastdb.dmg").is_err());

      let duplicate = format!("{zeros}  FastDB.dmg\n{zeros} *FastDB.dmg\n");
      assert!(parse_asset_checksum(duplicate.as_bytes(), "FastDB.dmg").is_err());
      assert!(parse_asset_checksum(b"invalid  FastDB.dmg\n", "FastDB.dmg").is_err());

      for name in ["", ".", "..", "../FastDB.dmg", "/tmp/file.dmg", "a\\file.dmg"] {
          assert!(validate_asset_name(name).is_err(), "{name:?}");
      }
      assert!(validate_asset_name("FastDB.dmg").is_ok());
  }

  #[test]
  fn release_metadata_lookup_rejects_ambiguity() {
      let manifest = ReleaseAsset {
          name: "SHA256SUMS.txt".into(),
          browser_download_url: "https://example.invalid/manifest".into(),
          size: 16,
      };
      assert!(release_metadata_asset(&[], "SHA256SUMS.txt").is_err());
      assert_eq!(
          release_metadata_asset(std::slice::from_ref(&manifest), "SHA256SUMS.txt").unwrap(),
          &manifest,
      );
      assert!(
          release_metadata_asset(&[manifest.clone(), manifest], "SHA256SUMS.txt").is_err()
      );
  }

  #[test]
  fn release_link_opens_only_the_configured_github_release_page() {
      let releases = "https://github.com/Matrix-I/fastdb/releases";
      assert_eq!(
          release_page_url("v0.3.0-rc.1"),
          format!("{releases}/tag/v0.3.0-rc.1"),
      );
      for tag in ["file:///tmp/update.dmg", "open-installer:payload", "../../download", "v1.2.3?x=1"] {
          assert_eq!(release_page_url(tag), releases);
      }
  }
  ```

- [x] Resolve exact approved versions and update the lockfile. Split verification out of `updater.rs`. Add manifest and signature constants, unique same-release metadata lookup, bounded byte reads with status checks, key/signature authentication, and exact checksum extraction. The decisive authentication is:

  ```rust
  let key = minisign_verify::PublicKey::decode(trusted_key)
      .context("Invalid update verification key")?;
  let signature = minisign_verify::Signature::decode(signature_text)
      .context("Invalid update manifest signature")?;
  key.verify(manifest_bytes, &signature, false)
      .context("Update manifest signature verification failed")?;
  ```

- [x] Make the existing async download entry consume both release metadata and the selected asset. Validate the basename before any write. Download the two metadata files on the shared Tokio runtime and authenticate them before downloading the package. Stream with finite time and size bounds into the existing buffered writer over a unique temporary file; update `Sha256` with the bytes written, flush and compare the digest before persisting. Emit one terminal event. On any failure, drop staging and emit `Failed`. Preserve download progress. Pass the path and fixed-size digest only through a private `VerifiedInstaller` constructor.
- [x] Route public-to-app install and manual-open calls through verified installer wrappers. Use one shared execution gate that checks the path is a regular file and hashes it with a bounded buffer before passing it to the platform action. Keep raw native opener and installer helpers private; native fallback calls must return through the same gate before launching. Distinguish verification errors from native launch errors so verification failure always produces the retry/error state and cannot produce the local fallback state. Keep native behavior for verified packages.
- [x] Carry `VerifiedInstaller` through `DownloadUiState::Finished`. Point both manual UI actions at the same controller method, run file checks and the local opener off the foreground thread, and surface errors. Preserve the existing successful close behavior for Open Installer and existing open-dialog behavior for Show in Folder. Guard against repeated actions while an update operation is busy. Construct the View on GitHub destination with `release_page_url(&release.tag_name)` at the UI boundary. Use `DEFAULT_REPO`; use the tag only when it satisfies the existing `release_tag_from_url` validation rules, and otherwise open that repository's release list. The API's `html_url` must never reach `cx.open_url`.
- [x] Exercise the automatic and manual production entry points with an injected native operation. After replacing the file, both paths must return the typed verification error before invoking that operation. Verify the controller maps verification failure to Retry and native failure to the verified manual fallback. Use real filesystem tests to prove macOS staging directories are unique and automatic installers cannot simultaneously hold the bundle-write lock. The macOS commands themselves require native acceptance.
- [x] Run `cargo test -p fastdb-app updater::` and `cargo test -p fastdb-app workspace::software_update`. Record process-boundary and loopback outcomes. Review all native call sites, exact release matching, timeout and byte-limit checks, retry, and staging cleanup. Ready to commit the app sources and lockfile as `fix(app): verify downloaded updates before launch` if authorized.

### Task 2: Verification & Docs

**Files:** `docs/guide/updates.md`, `docs/guide/not-built-yet.md`, this plan and the P3-05 entry in the architecture roadmap. **Consumes:** Task 1. **Produces:** final verification record, independent review, documentation. **Test levels:** workspace unit/integration, Linux startup smoke, user desktop acceptance.

- [x] Document the signed release requirement, what happens when verification fails and availability of the release page. Record GUI acceptance as pending where applicable. RED: n/a for prose, verified by reading the rendered Markdown and checking links.
- [x] Run `cargo fmt --all`, inspect the full patch including every new file, run `git diff --check`, then run `cargo fmt --all -- --check`, `cargo check --workspace --all-targets --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo build --workspace --all-targets --locked`, `cargo test --workspace --locked`, and `cargo audit`. Record exact outcomes, any pre-existing advisories, and skipped live provider paths.
- [x] Smoke-launch `cargo run -p fastdb-app --locked` on Linux with isolated state. Do not install a real update in the user's running app. Exercise the production download entry point end to end via the signed fixture and verify native launch gating via the injected boundary. Leave interactive Linux/macOS acceptance to the user.
- [x] Run `code-review-strict` for the final patch with the plan as requirements. Address findings, rerun the final gate after edits, and clean temporary verification artifacts. Retain the app binary and reusable dependencies. Update the roadmap and plan with the verified outcome. Ready to commit documentation separately if authorized.


## Execution checkpoint

- Baseline: `cargo test --offline --locked --workspace` passed on Linux at `6952025`. The original loopback download test expected and received a successful Finished event for unsigned bytes. The original updater from `6974b4c` published files immediately after rename without signature or digest verification.
- Release compatibility: downloaded the v0.3.0 manifest and signature. `minisign -Vm SHA256SUMS.txt -p resources/minisign.pub` verified the signature and trusted comment. The manifest lists the actual macOS DMG/ZIP, Linux DEB, and standalone binary filenames. Temporary downloads were removed.
- Plan review: approved on round 2 after adding the guarded GitHub release URL and explicit rejection tests for ambiguous metadata, unsafe paths, and malformed or duplicate checksums.
- Dependency baseline: `cargo audit --no-fetch --json` reported 14 vulnerabilities and 32 informational warnings in the original lockfile. Recheck exact advisory IDs and package versions after updating the lockfile.
- Observable RED: on the original download path, a loopback server returned 17 unsigned bytes and the test failed with a Finished event instead of Failed. The new helper regressions also failed compilation with the planned missing-symbol errors before implementation.
- Implementation dispatch: the Claude Code sub-session tool is unavailable in this app. The user-approved Codex substitution used one security implementer subagent for the batch. Focused updater (18 tests) and software-update controller checks passed on Linux. The implementation also bounds progress buffering and prevents Skip This Version from replacing an active update dialog.
- Comparable dependency audit with registry access: the original and proposed lockfiles have identical affected package versions and advisory IDs (14 vulnerability findings and 33 informational warnings). No advisory was added for either approved dependency.
- Code review round 1 identified the shared macOS install staging paths and missing wrapper/controller regression coverage. Both findings are in scope for correction before the final gate.
- Code review round 2 approved both fixes. Automatic macOS installers use per-operation staging and an advisory OS lock. The loopback fixture drives the production automatic and manual adapters with a recording process boundary. Replacing the file during the automatic attempt blocks the fallback. A native error for an intact file retains the retryable manual action and its error message. Tests also confirm lock contention, stage isolation, and release of the lock on close. The existing macOS ZIP bundle-copy status behavior was reported outside this change.
- Final Linux verification: `cargo fmt --all -- --check`, `git diff --check`, `cargo check --offline --workspace --all-targets --locked`, `cargo clippy --offline --workspace --all-targets --locked -- -D warnings`, and `cargo build --offline --workspace --all-targets --locked` exited 0. The existing unused-dependency notices in the mem-engine, platform-hal, sql-intel and ui-editor manifests remain.
- `cargo test --offline --workspace --locked`: 833 passed across 42 suites. Focused `cargo test --offline --locked -p fastdb-app updater::` passed 19 tests. Focused `cargo test --offline --locked -p fastdb-app workspace::software_update` passed 2 tests. Live provider cases took their configured skip paths because the FASTDB_TEST variables were unset.
- `timeout --kill-after=3s 15s cargo run --offline --locked -p fastdb-app` launched the Linux app with an isolated profile. Exit 124 was the expected smoke timeout. Native macOS execution and interactive installer acceptance on both platforms remain pending.
- Self-review: metadata lookup rejects duplicates before writes, the signature binds the exact package name, streaming bounds memory and disk, failed verification preserves the previous download, every native handoff uses the guarded adapter, and busy-state guards and the OS lock protect active operations. No stored configuration or release format changes. Temporary verification files are removed after the gate while the runnable binary and build dependencies are kept.
- Ready to commit the application and lockfile as `fix(app): verify downloaded updates before launch`, and the plan and guide files as `docs: record signed updater verification`.
