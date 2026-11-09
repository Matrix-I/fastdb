# Tag-triggered release workflow

Adopt litty-terminal's `release.yml`: pushing a `vX.Y.Z` (or `vX.Y.Z-rc.N`) tag
builds every asset, signs the checksum manifest with minisign and creates a
**draft** GitHub release. Publishing the draft stays manual.

## Decisions (2026-11-09)

- Sign `SHA256SUMS.txt` with minisign as litty does. The signing job runs in a
  `release` environment with required reviewers and the `MINISIGN_SECRET_KEY`
  and `MINISIGN_PASSWORD` secrets, matching `resources/minisign.pub`. The job
  fails when a secret is missing or the key does not match.
- Linux: native release build on `ubuntu-24.04` with the libraries already used
  by the `linux-app` CI job, packaged as a raw binary and a `.deb`. No zigbuild,
  `.rpm` or AppImage in the workflow.

## Jobs

1. `version`: the tag must equal the workspace `Cargo.toml` version exactly
   (an RC tag needs `X.Y.Z-rc.N` in `Cargo.toml`, so RC builds never claim the
   final version) and the tagged commit must be on `main`. Outputs the version,
   the Debian version (`X.Y.Z~rc.N` for an RC) and the prerelease flag.
2. `macos` (`macos-15`): `cargo test --workspace --locked`, then
   `./bundle_macos.sh --no-install`; uploads `FastDB.dmg`,
   `FastDB-macos-aarch64.zip` and `fastdb-macos-aarch64`. The app version comes
   from `Cargo.toml`, as the bundle script already does.
3. `linux` (`ubuntu-24.04`): `cargo test --workspace --locked`,
   `cargo build --release --locked`, then
   `ci/package-deb.sh` and `ci/test-package-deb.sh`; uploads
   `fastdb-linux-x86_64` and `fastdb_<tag version>_amd64.deb` (renamed from
   the `~rc` file name, which GitHub may rewrite).
4. `deb` (`ubuntu:24.04` container): `apt install` the package, check the
   installed binary matches, `ldd` resolves every library, and `apt remove`
   leaves nothing behind.
5. `release`: checks the exact asset set, writes and verifies
   `SHA256SUMS.txt`, signs it, verifies the signature against
   `resources/minisign.pub`, and runs `gh release create --draft --verify-tag
   --generate-notes`. A re-run updates its own draft with `--clobber` and
   refuses to touch a published release. Only this job gets `contents: write` and the secrets, and
   it uses no third-party actions.

## Script changes

- New `ci/package-deb.sh BINARY VERSION OUTDIR`: builds the `.deb` from an
  existing binary. `Depends` is computed by `dpkg-shlibdeps`, so the package
  pulls in the GPUI system libraries instead of a hand-written list.
- New `ci/test-package-deb.sh BINARY`: checks package fields, file contents,
  permissions, root ownership and the desktop entry, and that bad arguments are
  rejected.
- `ci/build-linux.sh`: read the version from `Cargo.toml`, refuse to run on a
  non-Linux host instead of silently packaging a host binary, and reuse
  `ci/package-deb.sh`. The local AppImage step stays.

## Verification

- `shellcheck` and `bash -n` on every changed script; `actionlint` on the
  workflow when available.
- Run `ci/test-package-deb.sh` in a local `ubuntu:24.04` container with a
  stand-in ELF binary.
- The workflow itself can only be exercised by pushing a tag. The first real
  run can be a `vX.Y.Z-rc.N` tag (with that version in `Cargo.toml`), which
  produces a draft prerelease. The user must create the `release` environment
  and secrets first.
- Not done: Cargo caching in release jobs (cold builds keep untrusted cache
  out of the signing pipeline), and a check that `ci.yml` passed for the tagged
  commit (step 1 of the release checklist; the environment approval is the gate).

## Docs

Update `docs/RELEASE_PLAN.md`, `docs/development/building.md` and the release
section of `CLAUDE.md`, which currently say no `release.yml` exists.
