//! Platform-specific secure storage for cryptographic keys and sensitive data
//!
//! This module provides a unified interface for secure storage across different platforms,
//! with platform-specific implementations that leverage hardware security features.

use aura_crypto::KeyShare;
use aura_types::{time_utils::current_unix_timestamp, AuraResult as Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Platform-specific imports
#[cfg(target_os = "android")]
use super::android::AndroidKeystore;
#[cfg(target_os = "ios")]
use super::ios::IOSKeychain;
#[cfg(target_os = "linux")]
use super::linux::LinuxKeyring;
#[cfg(target_os = "macos")]
use super::macos::MacOSKeychain;
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_os = "linux"
)))]
use super::memory::InMemoryStorage;

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

    /// Store data with security level (legacy compatibility)
    fn store_data(&self, key: &str, data: &[u8], _security_level: SecurityLevel) -> Result<()> {
        self.store_secure_data(key, data)
    }

    /// Retrieve data (legacy compatibility)
    fn retrieve_data(&self, key: &str) -> Result<Vec<u8>> {
        self.load_secure_data(key)?
            .ok_or_else(|| aura_types::AuraError::data_not_found(format!("Key not found: {}", key)))
    }

    /// Get device attestation information
    fn get_device_attestation(&self) -> Result<DeviceAttestation> {
        DeviceAttestation::new()
    }
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

/// Attestation statement for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationStatement {
    pub challenge: Vec<u8>,
    pub device_id: String,
    pub timestamp: u64,
    pub platform_properties: HashMap<String, String>,
    pub signature: Option<Vec<u8>>,
}

impl DeviceAttestation {
    /// Create a new device attestation instance
    pub fn new() -> Result<Self> {
        Ok(Self {
            platform: "macos".to_string(),
            device_id: format!("apple_device_{}", aura_crypto::generate_uuid()),
            security_features: vec!["keychain".to_string(), "secure_enclave".to_string()],
            security_level: SecurityLevel::HSM,
            attestation_data: HashMap::new(),
        })
    }

    /// Create an attestation statement for the given challenge
    pub fn create_attestation(&self, challenge: &[u8]) -> Result<AttestationStatement> {
        let mut platform_properties = HashMap::new();
        platform_properties.insert("sip_enabled".to_string(), "true".to_string());
        platform_properties.insert("platform".to_string(), self.platform.clone());

        Ok(AttestationStatement {
            challenge: challenge.to_vec(),
            device_id: self.device_id.clone(),
            timestamp: current_unix_timestamp(),
            platform_properties,
            signature: Some(vec![0u8; 64]), // Stub signature
        })
    }

    /// Get the public key for this attestation
    pub fn public_key(&self) -> Vec<u8> {
        vec![0u8; 32] // Stub public key
    }

    /// Verify an attestation statement
    pub fn verify_attestation(
        _statement: &AttestationStatement,
        _public_key: &[u8],
    ) -> Result<bool> {
        Ok(true) // Stub verification - always passes
    }
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
    inner: super::common::SecureStoreImpl<MacOSKeychain>,
    #[cfg(target_os = "ios")]
    inner: super::common::SecureStoreImpl<IOSKeychain>,
    #[cfg(target_os = "android")]
    inner: super::common::SecureStoreImpl<AndroidKeystore>,
    #[cfg(target_os = "linux")]
    inner: super::common::SecureStoreImpl<LinuxKeyring>,
    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "android",
        target_os = "linux"
    )))]
    inner: InMemoryStorage,
}

impl PlatformSecureStorage {
    /// Create a new platform-specific secure storage instance
    pub fn new(device_id: aura_types::DeviceId, account_id: aura_types::AccountId) -> Result<Self> {
        Ok(Self {
            #[cfg(target_os = "macos")]
            inner: super::macos::create_macos_secure_storage(device_id, account_id)?,
            #[cfg(target_os = "ios")]
            inner: super::ios::create_ios_secure_storage(device_id, account_id)?,
            #[cfg(target_os = "android")]
            inner: super::android::create_android_secure_storage(device_id, account_id)?,
            #[cfg(target_os = "linux")]
            inner: super::linux::create_linux_secure_storage(device_id, account_id)?,
            #[cfg(not(any(
                target_os = "macos",
                target_os = "ios",
                target_os = "android",
                target_os = "linux"
            )))]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_platform_storage_creation() {
        use aura_types::{AccountId, DeviceId};

        let device_id = DeviceId(aura_crypto::generate_uuid());
        let account_id = AccountId::new();
        let storage = PlatformSecureStorage::new(device_id, account_id);
        assert!(storage.is_ok(), "Should be able to create platform storage");
    }
}
