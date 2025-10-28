//! Platform-specific secure storage for cryptographic keys and sensitive data
//!
//! This module provides a unified interface for secure storage across different platforms,
//! with platform-specific implementations that leverage hardware security features.

use aura_coordination::KeyShare;
use aura_errors::{AuraError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Trait for secure storage of cryptographic keys and sensitive data
pub trait SecureStorage: Send + Sync {
    /// Store a key share securely
    fn store_key_share(&self, key_id: &str, key_share: &KeyShare) -> Result<()>;

    /// Load a key share from secure storage
    fn load_key_share(&self, key_id: &str) -> Result<Option<KeyShare>>;

    /// Delete a key share from secure storage
    fn delete_key_share(&self, key_id: &str) -> Result<()>;

    /// List all stored key share IDs
    fn list_key_shares(&self) -> Result<Vec<String>>;

    /// Store arbitrary secure data with a key
    fn store_secure_data(&self, key: &str, data: &[u8]) -> Result<()>;

    /// Load arbitrary secure data by key
    fn load_secure_data(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete secure data by key
    fn delete_secure_data(&self, key: &str) -> Result<()>;

    /// Get device attestation information
    fn get_device_attestation(&self) -> Result<DeviceAttestation>;
}

/// Device attestation information for security verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAttestation {
    /// Platform identifier (iOS, Android, macOS, etc.)
    pub platform: String,
    /// Device hardware identifier
    pub device_id: String,
    /// Security features available
    pub security_features: Vec<String>,
    /// Hardware security support level
    pub security_level: SecurityLevel,
    /// Platform-specific attestation data
    pub attestation_data: HashMap<String, String>,
}

/// Security level provided by the platform
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Software-only storage (not recommended for production)
    Software,
    /// Trusted Execution Environment
    TEE,
    /// Hardware Security Module / Secure Enclave
    HSM,
    /// StrongBox (Android) or equivalent highest security
    StrongBox,
}

/// Platform-specific secure storage implementation
pub struct PlatformSecureStorage {
    #[cfg(target_os = "macos")]
    inner: MacOSKeychainStorage,
    #[cfg(target_os = "ios")]
    inner: iOSKeychainStorage,
    #[cfg(target_os = "android")]
    inner: AndroidKeystoreStorage,
    #[cfg(target_os = "linux")]
    inner: LinuxSecretServiceStorage,
    #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "linux")))]
    inner: InMemoryStorage,
}

impl PlatformSecureStorage {
    /// Create a new platform-specific secure storage instance
    pub fn new() -> Result<Self> {
        Ok(Self {
            #[cfg(target_os = "macos")]
            inner: MacOSKeychainStorage::new()?,
            #[cfg(target_os = "ios")]
            inner: iOSKeychainStorage::new()?,
            #[cfg(target_os = "android")]
            inner: AndroidKeystoreStorage::new()?,
            #[cfg(target_os = "linux")]
            inner: LinuxSecretServiceStorage::new()?,
            #[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "linux")))]
            inner: InMemoryStorage::new()?,
        })
    }
}

impl SecureStorage for PlatformSecureStorage {
    fn store_key_share(&self, key_id: &str, key_share: &KeyShare) -> Result<()> {
        self.inner.store_key_share(key_id, key_share)
    }

    fn load_key_share(&self, key_id: &str) -> Result<Option<KeyShare>> {
        self.inner.load_key_share(key_id)
    }

    fn delete_key_share(&self, key_id: &str) -> Result<()> {
        self.inner.delete_key_share(key_id)
    }

    fn list_key_shares(&self) -> Result<Vec<String>> {
        self.inner.list_key_shares()
    }

    fn store_secure_data(&self, key: &str, data: &[u8]) -> Result<()> {
        self.inner.store_secure_data(key, data)
    }

    fn load_secure_data(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.inner.load_secure_data(key)
    }

    fn delete_secure_data(&self, key: &str) -> Result<()> {
        self.inner.delete_secure_data(key)
    }

    fn get_device_attestation(&self) -> Result<DeviceAttestation> {
        self.inner.get_device_attestation()
    }
}

// ========== Platform-Specific Implementations ==========

#[cfg(target_os = "macos")]
mod macos_keychain;
#[cfg(target_os = "macos")]
use macos_keychain::MacOSKeychainStorage;

#[cfg(target_os = "ios")]
mod ios_keychain;
#[cfg(target_os = "ios")]
use ios_keychain::iOSKeychainStorage;

#[cfg(target_os = "android")]
mod android_keystore;
#[cfg(target_os = "android")]
pub use android_keystore::AndroidKeystoreStorage;

#[cfg(target_os = "linux")]
mod linux_secret_service;
#[cfg(target_os = "linux")]
use linux_secret_service::LinuxSecretServiceStorage;

// Fallback in-memory storage for unsupported platforms
#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "linux")))]
mod memory_storage;
#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "linux")))]
use memory_storage::InMemoryStorage;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_platform_storage_creation() {
        let storage = PlatformSecureStorage::new();
        assert!(storage.is_ok(), "Should be able to create platform storage");
    }
}