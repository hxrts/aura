//! In-memory secure storage implementation for unsupported platforms
//!
//! This module provides a fallback in-memory implementation for platforms
//! that don't have native secure storage support.

use super::{DeviceAttestation, SecureStorage, SecurityLevel};
use aura_protocol::KeyShare;
use aura_types::{AuraError, AuraResult as Result};
use std::collections::HashMap;

/// In-memory storage implementation (NOT SECURE - for testing/unsupported platforms only)
pub struct InMemoryStorage {
    /// In-memory storage for key shares and secure data
    storage: std::sync::RwLock<HashMap<String, Vec<u8>>>,
}

impl InMemoryStorage {
    /// Create a new in-memory storage instance
    pub fn new() -> Result<Self> {
        Ok(Self {
            storage: std::sync::RwLock::new(HashMap::new()),
        })
    }
}

impl SecureStorage for InMemoryStorage {
    fn store_key_share(&self, key_id: &str, key_share: &KeyShare) -> Result<()> {
        let serialized = bincode::serialize(key_share)
            .map_err(|e| AuraError::secure_storage_error(format!("Serialization error: {}", e)))?;

        let mut storage = self.storage.write().unwrap();
        storage.insert(format!("keyshare_{}", key_id), serialized);
        Ok(())
    }

    fn load_key_share(&self, key_id: &str) -> Result<Option<KeyShare>> {
        let storage = self.storage.read().unwrap();

        match storage.get(&format!("keyshare_{}", key_id)) {
            Some(data) => {
                let key_share = bincode::deserialize(data).map_err(|e| {
                    AuraError::secure_storage_error(format!("Deserialization error: {}", e))
                })?;
                Ok(Some(key_share))
            }
            None => Ok(None),
        }
    }

    fn delete_key_share(&self, key_id: &str) -> Result<()> {
        let mut storage = self.storage.write().unwrap();
        storage.remove(&format!("keyshare_{}", key_id));
        Ok(())
    }

    fn list_key_shares(&self) -> Result<Vec<String>> {
        let storage = self.storage.read().unwrap();
        let keys: Vec<String> = storage
            .keys()
            .filter_map(|k| {
                if k.starts_with("keyshare_") {
                    Some(k.strip_prefix("keyshare_").unwrap().to_string())
                } else {
                    None
                }
            })
            .collect();
        Ok(keys)
    }

    fn store_secure_data(&self, key: &str, data: &[u8]) -> Result<()> {
        let mut storage = self.storage.write().unwrap();
        storage.insert(format!("data_{}", key), data.to_vec());
        Ok(())
    }

    fn load_secure_data(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let storage = self.storage.read().unwrap();
        Ok(storage.get(&format!("data_{}", key)).cloned())
    }

    fn delete_secure_data(&self, key: &str) -> Result<()> {
        let mut storage = self.storage.write().unwrap();
        storage.remove(&format!("data_{}", key));
        Ok(())
    }

    fn get_device_attestation(&self) -> Result<DeviceAttestation> {
        Ok(DeviceAttestation {
            platform: "memory".to_string(),
            device_id: "memory-storage".to_string(),
            security_features: vec!["software-only".to_string()],
            security_level: SecurityLevel::Software,
            attestation_data: HashMap::new(),
        })
    }
}
