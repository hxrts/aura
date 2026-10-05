//! File-backed stand-in for the platform credential store in test builds.
//!
//! Test binaries are rebuilt with a new ad-hoc code signature each time, so
//! the real macOS Keychain prompts on every run and test secrets would land in
//! the developer's login keychain. With the `test-keyring` feature (enabled
//! only through dev-dependencies) every `keyring::Entry` resolves here instead.
//! Each credential is one file named by a digest of its service and user, so
//! entries persist across runtime restarts within and between test processes
//! without a shared lock. Release builds never compile this module.

use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
use std::path::PathBuf;

/// Directory holding test credentials; `AURA_TEST_KEYRING_DIR` overrides it.
fn store_dir() -> PathBuf {
    std::env::var_os("AURA_TEST_KEYRING_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("aura-test-keyring"))
}

/// Distinguishes concurrent staged writes from one process.
static STAGING: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(Debug)]
struct FileCredential {
    path: PathBuf,
}

fn platform_failure(error: std::io::Error) -> keyring::Error {
    keyring::Error::PlatformFailure(Box::new(error))
}

impl CredentialApi for FileCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        let dir = store_dir();
        std::fs::create_dir_all(&dir).map_err(platform_failure)?;
        // Write then rename so a concurrent reader never sees a partial secret.
        let staged = dir.join(format!(
            "{}.{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("credential"),
            STAGING.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::write(&staged, secret).map_err(platform_failure)?;
        std::fs::rename(&staged, &self.path).map_err(platform_failure)
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        match std::fs::read(&self.path) {
            Ok(secret) => Ok(secret),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(keyring::Error::NoEntry)
            }
            Err(error) => Err(platform_failure(error)),
        }
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(keyring::Error::NoEntry)
            }
            Err(error) => Err(platform_failure(error)),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
struct FileCredentialBuilder;

impl CredentialBuilderApi for FileCredentialBuilder {
    fn build(
        &self,
        target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        let mut encoded = Vec::new();
        for part in [target.unwrap_or(""), service, user] {
            encoded.extend_from_slice(&(part.len() as u64).to_le_bytes());
            encoded.extend_from_slice(part.as_bytes());
        }
        let name = hex::encode(aura_core::hash::hash(&encoded));
        Ok(Box::new(FileCredential {
            path: store_dir().join(name),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Route every subsequent `keyring::Entry` to the file-backed test store.
pub(super) fn install() {
    static INSTALL: std::sync::Once = std::sync::Once::new();
    INSTALL.call_once(|| keyring::set_default_credential_builder(Box::new(FileCredentialBuilder)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builds_never_resolve_entries_to_the_platform_keychain() {
        install();
        let entry = keyring::Entry::new("aura-test-keyring-regression", "round-trip")
            .expect("test credential entry");
        assert!(
            entry.get_credential().is::<FileCredential>(),
            "test builds must use the file-backed store, not the login keychain"
        );
        let _ = entry.delete_credential();
        assert!(matches!(entry.get_secret(), Err(keyring::Error::NoEntry)));
        entry.set_secret(b"secret").expect("store");
        let reopened = keyring::Entry::new("aura-test-keyring-regression", "round-trip")
            .expect("reopened entry");
        assert_eq!(reopened.get_secret().expect("retrieve"), b"secret");
        reopened.delete_credential().expect("delete");
        assert!(matches!(entry.get_secret(), Err(keyring::Error::NoEntry)));
    }
}
