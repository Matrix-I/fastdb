# Keychain-backed connection passwords

## Scope

Store connection passwords in the OS credential store under com.fastdb.credentials,
keyed by connection ID. Persist only credential references in connections.json.
Migrate both legacy config formats without discarding plaintext until keychain
storage and atomic config replacement succeed. Remove the legacy source after
successful migration. Do not fall back to saving new passwords in plaintext.

## Implementation

1. Inject CredentialStore into ConnectionRepository; load and migrate credentials,
   preserve unresolved references on keychain errors, and synchronize password
   updates, clearing and connection deletion with config saves.
2. Run saves in the existing serialized background writer. Surface errors in the
   workspace and prevent Quit from silently ignoring failed credential saves.
3. Add regression tests using an in-memory/failing store for migration, restart,
   updates, clearing, deletion, missing credentials and failures.
4. Update README and release checklist, run formatting, workspace clippy/tests,
   build and launch with an isolated profile. Record any limits of native keychain
   and interactive UI verification.

The user requested committing the completed implementation. Keep this plan in its
own documentation commit before implementation.

## Implementation and validation record (2026-11-02)

Implemented keychain references, plaintext migration for both config formats,
password updates/clearing/deletion, preservation of unreadable references, and
best-effort rollback of credential updates when a config write fails. Secret
cleanup is recorded in the config for retry after failure or restart. Connection
saves run through the serialized background writer; failures have a visible retry
banner and prevent Quit/Restart from ignoring a failed save.

Validation:

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: passed.
- `cargo test --workspace --offline`: passed, including 103 app tests and the new
  migration/failure/cleanup/queue regressions. External provider tests skipped
  without their environment variables; no live database verification is claimed.
- `FASTDB_TEST_KEYCHAIN=1 cargo test -p platform-hal --offline native_keychain_round_trip_when_enabled`:
  passed against macOS Keychain using an isolated temporary credential; save,
  read, update, delete and missing-entry behavior were verified.
- `cargo build -p fastdb-app --offline`: passed.
- Launched the built app from `/` with an isolated empty FASTDB_CONFIG_DIR;
  startup remained running, then the test instance was terminated. Interactive
  banner/dialog behavior and access across ad-hoc signed updates remain unverified.
- Linux Secret Service was not exercised on this macOS host.
