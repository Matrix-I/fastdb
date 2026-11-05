# Local development credentials

Add opt-in `FASTDB_LOCAL_CREDENTIALS=1` to replace all three OS credential
stores with plaintext files under the selected profile's `local-credentials/`.
Keep Keychain as the default and do not automatically copy secrets between modes.
Use existing dependencies, private directories, owner-only files and atomic
replacement so readers never observe partially written passwords.

1. Test file persistence, namespaces, updates, deletion, permissions and errors.
2. Wire the repository to the selected backend; test save/load across repository
   recreation for database, SSH and TLS secrets without OS keychain access.
3. Document launch commands, plaintext implications and switching modes.
4. Run formatting, workspace tests, Clippy, build and independent diff review.
   GUI acceptance is manual per project preference.

No commits are created: the user has not requested a commit.

## Verification and self-review

- `cargo fmt --all`: passed; `git diff --check`: passed.
- `cargo test --workspace --offline`: passed with local environment access,
  including 218 app tests and six new local-storage/repository tests. The first
  sandboxed run failed because an SSH loopback bind was denied; the unrestricted
  full suite passed. Live provider tests retain their environment-based skips.
- `cargo clippy --workspace --all-targets --offline`: passed.
- `cargo build -p fastdb-app --offline`: passed; runnable binary retained.
- Real binary smoke: launched with `FASTDB_LOCAL_CREDENTIALS=1` and a temporary
  profile containing a synthetic legacy password. After startup, plaintext was
  removed from connections.json and the exact fixture existed in the file store.
  Process was terminated after observation (exit -15); temporary profile removed.
  Restricted startup could not reach macOS app services; the same smoke passed
  with local environment access. GUI interaction remains pending user acceptance.
- Independent review and re-review: no actionable findings.
- Full diff and untracked files reviewed; existing diagram changes belong to
  earlier work and are preserved. No dependencies, public APIs or connection
  configuration formats changed. No commit was created.

1. Requirements/correctness: explicit opt-in covers DB, SSH and TLS; tests verify
   restart persistence, updates, deletion, missing values, errors and boundaries.
2. Performance: individual files avoid whole-store loading; reads are limited to
   1 MiB; credential I/O stays in the existing repository flow outside rendering.
3. Security: plaintext is explicitly authorized; filenames encode identifiers,
   new directories/files use 0700/0600 on Unix, and secrets are never logged.
4. Robustness/dependencies: atomic replacement preserves complete reads and
   failed writes; I/O errors propagate; temporary writes clean up via RAII;
   no dependencies, external calls, clocks or retries were added.
5. Syntax/naming: formatter, Clippy and build pass with existing Rust conventions.
6. Modern APIs: existing tempfile atomic persistence and stable Rust scoped
   threads/Unix directory permissions are used without deprecated APIs.
7. Business clarity: user selected persistent plaintext storage; opt-in environment
   selection and separate development profile were stated before implementation.
8. Test coverage: six new tests assert observable persisted data, namespace
   isolation, permissions, errors, size limits and complete concurrent reads;
   real startup verifies production environment wiring. Manual GUI is pending.
9. Design: the private file store implements the existing CredentialStore trait;
   repository selection changes no driver, credential trait or persisted schema.

Exceptions: no plan commit because commits require an explicit request. No
Computer Use or automated GUI acceptance under the user's project preference;
manual desktop acceptance remains unverified. SQL/query-plan rules do not apply
because no queries or database schema were added or changed.
