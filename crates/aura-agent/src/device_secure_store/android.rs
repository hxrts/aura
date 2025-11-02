//! Android Keystore secure storage implementation
//!
//! This module provides a placeholder implementation for Android Keystore.
//! In a real implementation, this would integrate with Android's KeyStore API via JNI.

use super::{common::PlatformKeyStore, DeviceAttestation, SecurityLevel};
use aura_types::{AuraError, AuraResult as Result};
use std::collections::HashMap;
use tracing::{debug, warn};

/// Android Keystore implementation (placeholder with in-memory storage)
pub struct AndroidKeystore {
    /// In-memory storage for placeholder implementation
    storage: std::sync::RwLock<HashMap<String, Vec<u8>>>,
}

impl AndroidKeystore {
    /// Create a new Android keystore instance
    pub fn new() -> Result<Self> {
        warn!("Using placeholder Android Keystore implementation. Real Android deployment requires JNI integration.");
        Ok(Self {
            storage: std::sync::RwLock::new(HashMap::new()),
        })
    }
}

impl PlatformKeyStore for AndroidKeystore {
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
            platform: "Android".to_string(),
            device_id: "android-placeholder".to_string(),
            security_features: vec![
                "android-keystore".to_string(),
                "strongbox".to_string(),
                "tee".to_string(),
            ],
            security_level: SecurityLevel::StrongBox,
            attestation_data: HashMap::new(),
        })
    }
}

// Factory function to create a SecureStorage implementation for Android
pub fn create_android_secure_storage(
    device_id: aura_types::DeviceId,
    account_id: aura_types::AccountId,
) -> Result<super::common::SecureStoreImpl<AndroidKeystore>> {
    let platform = AndroidKeystore::new()?;
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
    fn test_android_keystore_creation() {
        let keystore = AndroidKeystore::new();
        assert!(keystore.is_ok(), "Should be able to create AndroidKeystore");
    }

    #[test]
    fn test_factory_function() {
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new(Uuid::new_v4());

        let storage = create_android_secure_storage(device_id, account_id);
        assert!(storage.is_ok(), "Should create secure storage");
    }

    #[test]
    fn test_device_attestation() {
        let keystore = AndroidKeystore::new().unwrap();
        let attestation = keystore.platform_attestation();
        assert!(
            attestation.is_ok(),
            "Should be able to get device attestation"
        );

        let attestation = attestation.unwrap();
        assert_eq!(attestation.platform, "Android");
        assert!(
            !attestation.device_id.is_empty(),
            "Device ID should not be empty"
        );
    }

    #[test]
    fn test_platform_operations() {
        let keystore = AndroidKeystore::new().unwrap();
        let test_data = b"test data";

        // Test store and load
        keystore.platform_store("test_key", test_data).unwrap();
        let loaded = keystore.platform_load("test_key").unwrap();
        assert_eq!(loaded, Some(test_data.to_vec()));

        // Test list
        let keys = keystore.platform_list("test_").unwrap();
        assert_eq!(keys, vec!["test_key".to_string()]);

        // Test delete
        keystore.platform_delete("test_key").unwrap();
        let loaded = keystore.platform_load("test_key").unwrap();
        assert_eq!(loaded, None);
    }
}
