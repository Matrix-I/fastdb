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
