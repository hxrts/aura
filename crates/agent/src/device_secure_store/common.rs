//! Common platform-agnostic secure storage implementation
//!
//! This module provides a generic implementation of SecureStorage that handles
//! serialization, key naming conventions, and error handling. Platform-specific
//! implementations only need to provide low-level storage operations.

use crate::{
    device_secure_store::{DeviceAttestation, SecureStorage, SecurityLevel},
    error::{AuraError, Result},
    utils::{storage_keys, ResultExt},
};
use aura_protocol::KeyShare;
use aura_types::{AccountId, DeviceId};
use std::marker::PhantomData;

/// Platform-specific storage operations trait
///
/// This trait defines the minimal interface that each platform must implement.
/// All high-level logic is handled by the generic SecureStoreImpl.
pub trait PlatformKeyStore: Send + Sync {
    /// Store raw bytes with the given key
    fn platform_store(&self, key: &str, data: &[u8]) -> Result<()>;

    /// Load raw bytes for the given key, returning None if not found
    fn platform_load(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete data for the given key
    fn platform_delete(&self, key: &str) -> Result<()>;

    /// List all keys with the given prefix
    fn platform_list(&self, prefix: &str) -> Result<Vec<String>>;

    /// Get platform-specific device attestation
    fn platform_attestation(&self) -> Result<DeviceAttestation>;
}

/// Generic secure storage implementation
///
/// This wraps a platform-specific implementation and provides all the high-level
/// SecureStorage functionality including serialization, key naming, and error handling.
pub struct SecureStoreImpl<P: PlatformKeyStore> {
    platform: P,
    device_id: DeviceId,
    account_id: AccountId,
    _phantom: PhantomData<P>,
}

impl<P: PlatformKeyStore> SecureStoreImpl<P> {
    /// Create a new secure store implementation
    pub fn new(platform: P, device_id: DeviceId, account_id: AccountId) -> Self {
        Self {
            platform,
            device_id,
            account_id,
            _phantom: PhantomData,
        }
    }

    /// Generate storage key for key shares
    fn key_share_key(&self, key_id: &str) -> String {
        format!("keyshare_{}_{}", self.device_id.0, key_id)
    }

    /// Generate storage key for secure data
    fn secure_data_key(&self, key: &str) -> String {
        format!("data_{}_{}", self.device_id.0, key)
    }

    /// Serialize a key share for storage
    fn serialize_key_share(&self, key_share: &KeyShare) -> Result<Vec<u8>> {
        bincode::serialize(key_share).serialize_context("Failed to serialize key share")
    }

    /// Deserialize a key share from storage
    fn deserialize_key_share(&self, data: &[u8]) -> Result<KeyShare> {
        bincode::deserialize(data).deserialize_context("Failed to deserialize key share")
    }
}

impl<P: PlatformKeyStore> SecureStorage for SecureStoreImpl<P> {
    fn store_key_share(&self, key_id: &str, key_share: &KeyShare) -> Result<()> {
        let storage_key = self.key_share_key(key_id);
        let serialized_data = self.serialize_key_share(key_share)?;

        self.platform
            .platform_store(&storage_key, &serialized_data)
            .storage_context(&format!("Failed to store key share {}", key_id))?;

        tracing::info!(
            device_id = %self.device_id,
            key_id = key_id,
            "Key share stored successfully"
        );

        Ok(())
    }

    fn load_key_share(&self, key_id: &str) -> Result<Option<KeyShare>> {
        let storage_key = self.key_share_key(key_id);

        let data = self.platform.platform_load(&storage_key).map_err(|e| {
            AuraError::storage_failed(format!("Failed to load key share {}: {}", key_id, e))
        })?;

        match data {
            Some(bytes) => {
                let key_share = self.deserialize_key_share(&bytes)?;
                tracing::info!(
                    device_id = %self.device_id,
                    key_id = key_id,
                    "Key share loaded successfully"
                );
                Ok(Some(key_share))
            }
            None => {
                tracing::debug!(
                    device_id = %self.device_id,
                    key_id = key_id,
                    "Key share not found"
                );
                Ok(None)
            }
        }
    }

    fn delete_key_share(&self, key_id: &str) -> Result<()> {
        let storage_key = self.key_share_key(key_id);

        self.platform.platform_delete(&storage_key).map_err(|e| {
            AuraError::storage_failed(format!("Failed to delete key share {}: {}", key_id, e))
        })?;

        tracing::info!(
            device_id = %self.device_id,
            key_id = key_id,
            "Key share deleted successfully"
        );

        Ok(())
    }

    fn list_key_shares(&self) -> Result<Vec<String>> {
        let prefix = format!("keyshare_{}_", self.device_id.0);

        let keys = self
            .platform
            .platform_list(&prefix)
            .map_err(|e| AuraError::storage_failed(format!("Failed to list key shares: {}", e)))?;

        // Extract key_id from storage keys
        let key_ids: Vec<String> = keys
            .into_iter()
            .filter_map(|key| {
                // Extract the key_id portion from the storage key
                key.strip_prefix(&prefix).map(|suffix| suffix.to_string())
            })
            .collect();

        tracing::info!(
            device_id = %self.device_id,
            count = key_ids.len(),
            "Listed key shares successfully"
        );

        Ok(key_ids)
    }

    fn store_secure_data(&self, key: &str, data: &[u8]) -> Result<()> {
        let storage_key = self.secure_data_key(key);

        self.platform
            .platform_store(&storage_key, data)
            .map_err(|e| {
                AuraError::storage_failed(format!("Failed to store secure data {}: {}", key, e))
            })?;

        tracing::info!(
            device_id = %self.device_id,
            key = key,
            data_len = data.len(),
            "Secure data stored successfully"
        );

        Ok(())
    }

    fn load_secure_data(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let storage_key = self.secure_data_key(key);

        let data = self.platform.platform_load(&storage_key).map_err(|e| {
            AuraError::storage_failed(format!("Failed to load secure data {}: {}", key, e))
        })?;

        match data {
            Some(bytes) => {
                tracing::info!(
                    device_id = %self.device_id,
                    key = key,
                    data_len = bytes.len(),
                    "Secure data loaded successfully"
                );
                Ok(Some(bytes))
            }
            None => {
                tracing::debug!(
                    device_id = %self.device_id,
                    key = key,
                    "Secure data not found"
                );
                Ok(None)
            }
        }
    }

    fn delete_secure_data(&self, key: &str) -> Result<()> {
        let storage_key = self.secure_data_key(key);

        self.platform.platform_delete(&storage_key).map_err(|e| {
            AuraError::storage_failed(format!("Failed to delete secure data {}: {}", key, e))
        })?;

        tracing::info!(
            device_id = %self.device_id,
            key = key,
            "Secure data deleted successfully"
        );

        Ok(())
    }

    fn get_device_attestation(&self) -> Result<DeviceAttestation> {
        self.platform.platform_attestation()
    }
}
