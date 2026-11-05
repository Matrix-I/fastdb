use anyhow::Result;
use platform_hal::keychain::CredentialStore;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_SECRET_BYTES: usize = 1024 * 1024;

/// Plaintext storage explicitly enabled for a development profile.
pub(super) struct LocalCredentialStore {
    directory: PathBuf,
}

impl CredentialStore for LocalCredentialStore {
    fn save(&self, account: &str, secret: &str) -> Result<()> {
        anyhow::ensure!(
            secret.len() <= MAX_SECRET_BYTES,
            "Local credential is too large"
        );
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&self.directory)?;
        // NamedTempFile creates owner-only files and removes abandoned writes on error.
        let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
        temporary.write_all(secret.as_bytes())?;
        temporary.as_file().sync_all()?;
        temporary.persist(self.path(account))?;
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<String>> {
        let file = match std::fs::File::open(self.path(account)) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut secret = String::new();
        file.take((MAX_SECRET_BYTES + 1) as u64)
            .read_to_string(&mut secret)?;
        anyhow::ensure!(
            secret.len() <= MAX_SECRET_BYTES,
            "Local credential is too large"
        );
        Ok(Some(secret))
    }

    fn delete(&self, account: &str) -> Result<()> {
        match std::fs::remove_file(self.path(account)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

impl LocalCredentialStore {
    pub(super) fn new(profile: &Path, service: &str) -> Self {
        Self {
            directory: profile.join("local-credentials").join(encode(service)),
        }
    }

    fn path(&self, account: &str) -> PathBuf {
        self.directory.join(encode(account))
    }
}

// Encode identifiers so even legacy account names cannot escape the profile.
fn encode(value: &str) -> String {
    use std::fmt::Write;
    let mut encoded = String::from("secret-");
    for byte in value.bytes() {
        write!(encoded, "{byte:02x}").expect("writing to a String");
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_updates_and_deletes_across_instances_without_namespace_collisions() {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalCredentialStore::new(temp.path(), "database");
        let reopened = LocalCredentialStore::new(temp.path(), "database");
        let ssh = LocalCredentialStore::new(temp.path(), "ssh");
        let account = "../../connection/é";
        assert_eq!(store.get(account).unwrap(), None);
        store.delete(account).unwrap();
        store.save(account, "first\n秘密").unwrap();
        ssh.save(account, "ssh-secret").unwrap();
        assert_eq!(
            reopened.get(account).unwrap().as_deref(),
            Some("first\n秘密")
        );
        reopened.save(account, "replacement").unwrap();
        assert_eq!(store.get(account).unwrap().as_deref(), Some("replacement"));
        store.delete(account).unwrap();
        assert_eq!(reopened.get(account).unwrap(), None);
        assert_eq!(ssh.get(account).unwrap().as_deref(), Some("ssh-secret"));
        assert!(store.path(account).starts_with(&store.directory));
    }

    #[test]
    fn reports_io_and_invalid_utf8_errors() {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalCredentialStore::new(temp.path(), "database");
        store.save("a", "secret").unwrap();
        std::fs::write(store.path("a"), [0xff]).unwrap();
        assert!(store.get("a").is_err());
        std::fs::create_dir(store.path("b")).unwrap();
        assert!(store.save("b", "secret").is_err());
        assert!(store.get("b").is_err());
        assert!(store.delete("b").is_err());
    }

    #[test]
    fn accepts_empty_and_limit_sized_secrets_but_rejects_oversized_values() {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalCredentialStore::new(temp.path(), "database");
        store.save("a", "").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some(""));
        let maximum = "x".repeat(MAX_SECRET_BYTES);
        store.save("a", &maximum).unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some(maximum.as_str()));
        let oversized = format!("{maximum}x");
        assert!(store.save("a", &oversized).is_err());
        assert_eq!(store.get("a").unwrap().as_deref(), Some(maximum.as_str()));
        std::fs::write(store.path("a"), oversized).unwrap();
        assert!(store.get("a").is_err());
    }

    #[test]
    fn concurrent_readers_observe_only_complete_replacements() {
        let temp = tempfile::tempdir().unwrap();
        let store = LocalCredentialStore::new(temp.path(), "database");
        let first = "a".repeat(8192);
        let second = "b".repeat(16384);
        store.save("a", &first).unwrap();
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                barrier.wait();
                for _ in 0..32 {
                    store.save("a", &second).unwrap();
                    store.save("a", &first).unwrap();
                }
            });
            barrier.wait();
            for _ in 0..64 {
                let loaded = store.get("a").unwrap().unwrap();
                assert!(loaded == first || loaded == second);
            }
        });
        assert_eq!(store.get("a").unwrap(), Some(first));
    }

    #[cfg(unix)]
    #[test]
    fn creates_owner_only_files_and_directories() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let store = LocalCredentialStore::new(temp.path(), "database");
        for secret in ["first", "updated"] {
            store.save("a", secret).unwrap();
            assert_eq!(
                std::fs::metadata(store.path("a"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
            for directory in [&store.directory, &temp.path().join("local-credentials")] {
                assert_eq!(
                    std::fs::metadata(directory).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
        }
    }
}
