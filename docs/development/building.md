# Building and packaging

## Build from source

### Prerequisites

- **Rust** stable, with `cargo` (see `rust-toolchain.toml`). All workspace crates use Rust edition **2024**.
- **macOS** with Xcode command line tools.
- **Linux** system libraries (Debian/Ubuntu):
  ```bash
  sudo apt-get install -y build-essential pkg-config libdbus-1-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libfontconfig1-dev libasound2-dev libvulkan-dev libssl-dev
  ```

### Run

```bash
git clone https://github.com/Matrix-I/fastdb.git
cd fastdb
cargo run --release -p fastdb-app
```

## Packaging

For local macOS builds, use the root script (same workflow as Litty):

```bash
./bundle_macos.sh               # Build for this Mac and install if ~/Applications exists
./bundle_macos.sh --no-install  # Package only
```

Outputs go to `dist/`: `FastDB.app`, `FastDB.dmg` (with an Applications shortcut),
`FastDB-macos.zip`, and `fastdb-macos`, plus architecture-suffixed binary/ZIP copies.
The script also writes `SHA256SUMS.txt` for `FastDB.dmg`, the architecture-suffixed
ZIP and standalone binary. With minisign installed and a secret key available,
it signs the manifest as `SHA256SUMS.txt.minisig` and verifies the signature against
`FASTDB_MINISIGN_PUBKEY` (base64 public key), falling back to the last line of
`resources/minisign.pub`. Set `MINISIGN_SECRET_KEY` to your secret-key path;
the default is `~/.minisign/minisign.key`. Keep private keys outside the repository.
Missing minisign or secret key produces unsigned checksums with a warning.
Signing requires a public key; failed signing or a key mismatch fails packaging
and removes the signature. Upload the manifest/signature alongside those exact
artifacts. The current FastDB updater does not verify minisign signatures yet.

```bash
MINISIGN_SECRET_KEY=/path/to/fastdb.key \
FASTDB_MINISIGN_PUBKEY='<base64-public-key>' ./bundle_macos.sh --no-install
# Verify the manifest before checking the artifact hashes:
minisign -V -m dist/SHA256SUMS.txt -p /path/to/fastdb.pub
(cd dist && shasum -a 256 -c SHA256SUMS.txt)
```

The app is signed ad-hoc for local use; distribution signing and notarization are separate steps.
Both macOS packaging scripts use `diskutil image create from` when available,
with an `hdiutil` fallback for older macOS versions.

Packaging scripts live in [`ci/`](../../ci/):

```bash
./ci/build-macos.sh   # dist/macos/FastDB.app and a .dmg
```

```bash
./ci/build-linux.sh   # Linux only: dist/linux/fastdb-linux-x86_64, a .deb and an AppImage
```

`ci/package-deb.sh BINARY VERSION OUT_DIR` packages an existing binary; its
`Depends` field comes from `dpkg-shlibdeps`. `ci/test-package-deb.sh BINARY`
checks the package on Debian/Ubuntu (needs `dpkg-dev`, `binutils`,
`desktop-file-utils`).

Releases are built by [`.github/workflows/release.yml`](../../.github/workflows/release.yml)
when a `vX.Y.Z` or `vX.Y.Z-rc.N` tag is pushed; see
[`docs/RELEASE_PLAN.md`](../RELEASE_PLAN.md).
