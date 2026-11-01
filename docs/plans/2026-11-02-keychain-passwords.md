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
