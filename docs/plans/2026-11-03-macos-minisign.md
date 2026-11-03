# macOS bundle checksums and minisign

Match Litty's local packaging workflow: write SHA256SUMS.txt for the DMG and
architecture-specific ZIP/binary, optionally sign with minisign, verify against
FastDB's public key, and remove stale signatures. Preserve existing bundle,
architecture aliases and installation behavior. No updater behavior changes.

Use MINISIGN_SECRET_KEY (default ~/.minisign/minisign.key) and
FASTDB_MINISIGN_PUBKEY (fallback resources/minisign.pub). Public key configuration
is required when signing; never create or copy production private keys.
Missing minisign/secret key leaves checksums unsigned with explicit feedback,
as in Litty. Failed signing/verification must fail packaging and remove signatures.

Test the actual bundle script in an isolated fixture with mocked native build/
packaging boundaries, real checksums, and real minisign temporary keys. Cover
unsigned output, stale cleanup, successful verification and mismatched keys.
Run shell syntax checks, formatter, Clippy and workspace tests, and obtain an
independent review. Native release packaging validation will be recorded separately.
No commits are authorized for this change.

## Results

- `python3 ci/test_bundle_macos.py`: 7 passed. The actual bundle script runs in
  isolated fixtures with real SHA-256 and temporary minisign keypairs. Tests
  cover signed/unsigned output, missing executable/public/secret keys, key
  mismatch, partial signing failure, stale metadata and build failure.
- `bash -n bundle_macos.sh` and `shellcheck bundle_macos.sh`: passed.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked`
  and `cargo test --workspace --locked`: passed (335 harness passes; live
  provider/keychain checks remain unconfigured, not validated coverage).
- `MINISIGN_SECRET_KEY=/tmp/fastdb-minisign-validation-no-secret CARGO_NET_OFFLINE=true
  ./bundle_macos.sh --no-install`: actual release build and native packaging passed
  outside the sandbox. Initial sandbox DMG creation failed with Device not
  configured; retry outside the sandbox succeeded without source changes.
- `cd dist && shasum -a 256 -c SHA256SUMS.txt`: DMG, aarch64 ZIP and aarch64
  binary all OK. Native run was deliberately unsigned; real signing/verification
  is covered with isolated temporary keys. No production key was used or created.
- Independent review found two fixture issues (Cargo PATH precedence and the
  diskutil help argument); both fixed and re-reviewed with no remaining findings.
- `git diff --check`: passed. No commit/push or installation performed.

## Self-review

Follow-up (2026-11-03): after the user supplied `resources/minisign.pub`,
`CARGO_NET_OFFLINE=true ./bundle_macos.sh --no-install` completed with real
minisign signing using the configured default secret key. Independent
`minisign -V -m dist/SHA256SUMS.txt -p resources/minisign.pub` verified both
signatures; `shasum -a 256 -c SHA256SUMS.txt` verified all three artifacts.
`codesign --verify --deep --strict dist/FastDB.app` passed. The seven packaging
tests passed again. No private key was read into chat, copied or modified.
The bundle remains ad-hoc signed rather than notarized; updater integration is
outside this packaging change. No install, commit or push was performed.

1. Correctness: manifest names match Litty's distribution pattern and actual
   outputs; key mismatch and signing failure remove signatures and fail.
2. Performance: one sequential hash pass over three artifacts, only at packaging.
3. Security: signing is explicit and verified against configured public key;
   private material stays outside the repository and command output.
4. Robustness: stale metadata is removed before build, partial signatures cleaned
   on failure; no network calls, dependencies or persistent key generation added.
5. Syntax/naming: Bash syntax, ShellCheck and formatting checks pass.
6. Modern APIs: uses installed minisign's documented sign/verify flags and existing
   native packaging commands; no Rust API changes.
7. Business clarity: follows Litty's optional signing workflow; requires a public
   key when signing; README states updater verification is separate.
8. Coverage: seven executable fixture tests plus actual native package/checksum
   run; production-key signing is not claimed. No app UI changed.
9. Design: short packaging step added to existing script; tests isolate external
   build/platform boundaries while exercising real signing/checksum tools.
