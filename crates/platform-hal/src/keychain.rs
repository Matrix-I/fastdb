use anyhow::{Context, Result};
use keyring::Entry;
use tracing::{debug, warn};

pub const FASTDB_KEYCHAIN_SERVICE: &str = "com.fastdb.credentials";

/// Store a secret in the OS secure storage (macOS Keychain / Linux Secret Service)
pub fn save_credential(service: &str, account: &str, secret: &str) -> Result<()> {
    let entry = Entry::new(service, account)
        .with_context(|| format!("Failed to create keyring entry for account: {account}"))?;
    entry.set_password(secret)
        .with_context(|| format!("Failed to save secret in keyring for account: {account}"))?;
    debug!("Successfully saved credential in keychain for {}", account);
    Ok(())
}

/// Retrieve a secret from the OS secure storage
pub fn get_credential(service: &str, account: &str) -> Result<Option<String>> {
    let entry = Entry::new(service, account)
        .with_context(|| format!("Failed to create keyring entry for account: {account}"))?;
    match entry.get_password() {
        Ok(pw) => Ok(Some(pw)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            warn!("Keyring query returned error for {}: {}", account, e);
            Err(e).with_context(|| format!("Failed to query keyring for account: {account}"))
        }
    }
}

/// Delete a secret from the OS secure storage
pub fn delete_credential(service: &str, account: &str) -> Result<()> {
    let entry = Entry::new(service, account)
        .with_context(|| format!("Failed to create keyring entry for account: {account}"))?;
    match entry.delete_credential() {
        Ok(_) => {
            debug!("Deleted credential from keychain for {}", account);
            Ok(())
        }
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e).with_context(|| format!("Failed to delete credential for account: {account}")),
    }
}

/// Secret storage keyed by account, so callers can swap the OS keychain for a test double
pub trait CredentialStore: Send + Sync {
    fn save(&self, account: &str, secret: &str) -> Result<()>;
    fn get(&self, account: &str) -> Result<Option<String>>;
    fn delete(&self, account: &str) -> Result<()>;
}

/// OS-backed store (macOS Keychain / Linux Secret Service) under a fixed service name
pub struct KeychainStore {
    service: String,
}

impl KeychainStore {
    pub fn new(service: impl Into<String>) -> Self {
        Self { service: service.into() }
    }
}

impl Default for KeychainStore {
    fn default() -> Self {
        Self::new(FASTDB_KEYCHAIN_SERVICE)
    }
}

impl CredentialStore for KeychainStore {
    fn save(&self, account: &str, secret: &str) -> Result<()> {
        save_credential(&self.service, account, secret)
    }

    fn get(&self, account: &str) -> Result<Option<String>> {
        get_credential(&self.service, account)
    }

    fn delete(&self, account: &str) -> Result<()> {
        delete_credential(&self.service, account)
    }
}

/// Process-local store for tests and environments without a keychain
#[derive(Default)]
pub struct InMemoryCredentialStore {
    secrets: std::sync::Mutex<std::collections::HashMap<String, String>>,
}

impl CredentialStore for InMemoryCredentialStore {
    fn save(&self, account: &str, secret: &str) -> Result<()> {
        self.secrets.lock().unwrap().insert(account.to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<String>> {
        Ok(self.secrets.lock().unwrap().get(account).cloned())
    }

    fn delete(&self, account: &str) -> Result<()> {
        self.secrets.lock().unwrap().remove(account);
        Ok(())
    }
}
