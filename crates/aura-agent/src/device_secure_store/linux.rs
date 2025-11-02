//! Linux Secret Service implementation for secure storage (Placeholder)
//!
//! This module provides a placeholder implementation for Linux Secret Service.
//! In a real implementation, this would integrate with libsecret/keyutils.

use super::{common::PlatformKeyStore, DeviceAttestation, SecurityLevel};
use aura_types::{AuraError, AuraResult as Result};
use std::collections::HashMap;

/// Linux Secret Service implementation (placeholder with in-memory storage)
pub struct LinuxKeyring {
    /// In-memory storage for placeholder implementation
    storage: std::sync::RwLock<HashMap<String, Vec<u8>>>,
}

impl LinuxKeyring {
    /// Create a new Linux keyring instance
    pub fn new() -> Result<Self> {
        Ok(Self {
            storage: std::sync::RwLock::new(HashMap::new()),
        })
    }
}

impl PlatformKeyStore for LinuxKeyring {
    fn platform_store(&self, key: &str, data: &[u8]) -> Result<()> {
        let mut storage = self.storage.write().unwrap();
        storage.insert(key.to_string(), data.to_vec());
        Ok(())
    }

    fn platform_load(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let storage = self.storage.read().unwrap();
        Ok(storage.get(key).cloned())
    }

    fn platform_delete(&self, key: &str) -> Result<()> {
        let mut storage = self.storage.write().unwrap();
        storage.remove(key);
        Ok(())
    }

    fn platform_list(&self, prefix: &str) -> Result<Vec<String>> {
        let storage = self.storage.read().unwrap();
        let keys: Vec<String> = storage
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect();
        Ok(keys)
    }

    fn platform_attestation(&self) -> Result<DeviceAttestation> {
        Ok(DeviceAttestation {
            platform: "Linux".to_string(),
            device_id: "linux-placeholder".to_string(),
            security_features: vec!["secret-service".to_string(), "keyutils".to_string()],
            security_level: SecurityLevel::TEE,
            attestation_data: HashMap::new(),
        })
    }
}

// Factory function to create a SecureStorage implementation for Linux
pub fn create_linux_secure_storage(
    device_id: aura_types::DeviceId,
    account_id: aura_types::AccountId,
) -> Result<super::common::SecureStoreImpl<LinuxKeyring>> {
    let platform = LinuxKeyring::new()?;
    Ok(super::common::SecureStoreImpl::new(
        platform, device_id, account_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::{AccountId, DeviceId};
    use uuid::Uuid;

    #[test]
    fn test_linux_keyring_creation() {
        let keyring = LinuxKeyring::new();
        assert!(keyring.is_ok(), "Should be able to create LinuxKeyring");
    }

    #[test]
    fn test_factory_function() {
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new(Uuid::new_v4());

        let storage = create_linux_secure_storage(device_id, account_id);
        assert!(storage.is_ok(), "Should create secure storage");
    }

    #[test]
    fn test_device_attestation() {
        let keyring = LinuxKeyring::new().unwrap();
        let attestation = keyring.platform_attestation();
        assert!(
            attestation.is_ok(),
            "Should be able to get device attestation"
        );

        let attestation = attestation.unwrap();
        assert_eq!(attestation.platform, "Linux");
        assert!(
            !attestation.device_id.is_empty(),
            "Device ID should not be empty"
        );
    }

    #[test]
    fn test_platform_operations() {
        let keyring = LinuxKeyring::new().unwrap();
        let test_data = b"test data";

        // Test store and load
        keyring.platform_store("test_key", test_data).unwrap();
        let loaded = keyring.platform_load("test_key").unwrap();
        assert_eq!(loaded, Some(test_data.to_vec()));

        // Test list
        let keys = keyring.platform_list("test_").unwrap();
        assert_eq!(keys, vec!["test_key".to_string()]);

        // Test delete
        keyring.platform_delete("test_key").unwrap();
        let loaded = keyring.platform_load("test_key").unwrap();
        assert_eq!(loaded, None);
    }
}
