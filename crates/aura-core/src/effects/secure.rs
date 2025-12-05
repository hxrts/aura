//! Secure Storage Effects Trait Definitions
//!
//! This module defines trait interfaces for secure storage operations that require
//! hardware-backed security features like secure enclaves, TPMs, or hardware security modules.
//! These operations provide stronger security guarantees than regular storage.
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Usage**: Secure enclave/TPM/HSM integration for cryptographic material storage
//!
//! This is an infrastructure effect providing hardware security module interfaces
//! with no Aura-specific semantics. Implementations should interface with platform
//! secure storage APIs (Intel SGX, ARM TrustZone, Apple Secure Enclave, TPM) and
//! provide software fallback for testing environments.
//!
//! ## Security Model
//!
//! Secure storage provides:
//! - Hardware-backed encryption and integrity protection
//! - Access control enforced at the hardware level
//! - Protection against physical attacks and privilege escalation
//! - Key derivation tied to device identity/attestation
//!
//! ## Use Cases
//!
//! - FROST nonce storage (prevent reuse attacks)
//! - Cryptographic signing shares (threshold cryptography)
//! - Device attestation certificates
//! - Critical configuration data

use crate::time::PhysicalTime;
use crate::AuraError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Secure storage operation error
pub type SecureStorageError = AuraError;

/// Location within secure storage
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SecureStorageLocation {
    /// Namespace for organizing secure data
    pub namespace: String,
    /// Unique key within the namespace
    pub key: String,
    /// Optional sub-key for hierarchical organization
    pub sub_key: Option<String>,
}

impl SecureStorageLocation {
    /// Create a new secure storage location
    pub fn new(namespace: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            key: key.into(),
            sub_key: None,
        }
    }

    /// Create with a sub-key for hierarchical organization
    pub fn with_sub_key(
        namespace: impl Into<String>,
        key: impl Into<String>,
        sub_key: impl Into<String>,
    ) -> Self {
        Self {
            namespace: namespace.into(),
            key: key.into(),
            sub_key: Some(sub_key.into()),
        }
    }

    /// Get the full key path as a string
    pub fn full_path(&self) -> String {
        if let Some(sub_key) = &self.sub_key {
            format!("{}/{}/{}", self.namespace, self.key, sub_key)
        } else {
            format!("{}/{}", self.namespace, self.key)
        }
    }
}

/// Capabilities required for secure storage operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecureStorageCapability {
    /// Read access to secure storage
    Read,
    /// Write access to secure storage
    Write,
    /// Delete access to secure storage
    Delete,
    /// Ability to list keys in secure storage
    List,
    /// Access to device attestation for key binding
    DeviceAttestation,
    /// Time-bound access control (uses unified time system)
    TimeBound { expires_at: PhysicalTime },
}

impl SecureStorageCapability {
    /// Create a time-bound capability with millisecond expiration (backward compatibility)
    pub fn time_bound_ms(expires_at_ms: u64) -> Self {
        Self::TimeBound {
            expires_at: PhysicalTime {
                ts_ms: expires_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Get expiration in milliseconds if this is a TimeBound capability
    pub fn expires_at_ms(&self) -> Option<u64> {
        match self {
            Self::TimeBound { expires_at } => Some(expires_at.ts_ms),
            _ => None,
        }
    }
}

/// Secure storage effects interface
///
/// This trait defines operations for storing cryptographic material and sensitive data
/// in hardware-backed secure storage. Unlike regular storage, secure storage provides:
/// - Hardware-level encryption and integrity protection
/// - Access control enforced by secure hardware
/// - Protection against physical and privilege escalation attacks
/// - Optional time-bound access controls
///
/// # Implementation Notes
///
/// - Production: Interface with platform secure enclaves (Intel SGX, ARM TrustZone, Apple Secure Enclave, etc.)
/// - Testing: In-memory mock with simulated security properties
/// - Simulation: Deterministic mock for reproducible testing
///
/// # Stability: EXPERIMENTAL
/// This API is under development and may change in future versions.
#[async_trait]
pub trait SecureStorageEffects: Send + Sync {
    /// Store data securely with optional capabilities
    ///
    /// Stores sensitive data in hardware-backed secure storage. The data is encrypted
    /// and bound to the device identity to prevent unauthorized access.
    ///
    /// # Parameters
    /// - `location`: Where to store the data within secure storage
    /// - `data`: Sensitive data to store (will be encrypted)
    /// - `capabilities`: Required capabilities for accessing this data
    ///
    /// # Security Properties
    /// - Data is encrypted using hardware-derived keys
    /// - Access is controlled by the specified capabilities
    /// - Storage is tamper-resistant and integrity-protected
    async fn secure_store(
        &self,
        location: &SecureStorageLocation,
        data: &[u8],
        capabilities: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError>;

    /// Retrieve data from secure storage
    ///
    /// Retrieves and decrypts data from secure storage, verifying that the caller
    /// has the required capabilities.
    ///
    /// # Parameters
    /// - `location`: Where to retrieve the data from
    /// - `required_capabilities`: Capabilities needed to access this data
    ///
    /// # Returns
    /// The decrypted data if access is authorized and data exists
    async fn secure_retrieve(
        &self,
        location: &SecureStorageLocation,
        required_capabilities: &[SecureStorageCapability],
    ) -> Result<Vec<u8>, SecureStorageError>;

    /// Delete data from secure storage
    ///
    /// Securely deletes data from storage, ensuring it cannot be recovered.
    ///
    /// # Parameters
    /// - `location`: Location of data to delete
    /// - `required_capabilities`: Capabilities needed to delete this data
    async fn secure_delete(
        &self,
        location: &SecureStorageLocation,
        required_capabilities: &[SecureStorageCapability],
    ) -> Result<(), SecureStorageError>;

    /// Check if data exists at the given location
    ///
    /// Checks for existence without retrieving the actual data.
    ///
    /// # Parameters
    /// - `location`: Location to check
    async fn secure_exists(
        &self,
        location: &SecureStorageLocation,
    ) -> Result<bool, SecureStorageError>;

    /// List available keys in a namespace
    ///
    /// Lists keys available in the specified namespace, subject to access controls.
    ///
    /// # Parameters
    /// - `namespace`: Namespace to list
    /// - `required_capabilities`: Capabilities needed to list keys
    async fn secure_list_keys(
        &self,
        namespace: &str,
        required_capabilities: &[SecureStorageCapability],
    ) -> Result<Vec<String>, SecureStorageError>;

    /// Generate and store a new cryptographic key
    ///
    /// Generates a new cryptographic key within the secure storage, ensuring it
    /// never leaves the secure environment in plaintext.
    ///
    /// # Parameters
    /// - `location`: Where to store the generated key
    /// - `key_type`: Type of key to generate (e.g., "ed25519", "frost-share")
    /// - `capabilities`: Required capabilities for accessing this key
    ///
    /// # Returns
    /// The public key material (if applicable), while private key stays secure
    async fn secure_generate_key(
        &self,
        location: &SecureStorageLocation,
        key_type: &str,
        capabilities: &[SecureStorageCapability],
    ) -> Result<Option<Vec<u8>>, SecureStorageError>;

    /// Create a time-bound access token
    ///
    /// Creates an access token that allows operations within a specific time window.
    /// Used for implementing time-bound nonce access and preventing replay attacks.
    ///
    /// # Parameters
    /// - `location`: Location the token grants access to
    /// - `capabilities`: Capabilities granted by this token
    /// - `expires_at`: Timestamp when the token expires (uses unified time system)
    ///
    /// # Returns
    /// An opaque token that can be used for time-bound access
    async fn secure_create_time_bound_token(
        &self,
        location: &SecureStorageLocation,
        capabilities: &[SecureStorageCapability],
        expires_at: &PhysicalTime,
    ) -> Result<Vec<u8>, SecureStorageError>;

    /// Use a time-bound token to access data
    ///
    /// Retrieves data using a previously created time-bound token, automatically
    /// checking expiration and capabilities.
    ///
    /// # Parameters
    /// - `token`: Time-bound access token
    /// - `location`: Location to access
    ///
    /// # Returns
    /// The data if the token is valid and not expired
    async fn secure_access_with_token(
        &self,
        token: &[u8],
        location: &SecureStorageLocation,
    ) -> Result<Vec<u8>, SecureStorageError>;

    /// Get device attestation for secure operations
    ///
    /// Provides device attestation that can be used to verify the integrity
    /// of the secure storage environment and bind operations to a specific device.
    ///
    /// # Returns
    /// Device attestation certificate or proof
    async fn get_device_attestation(&self) -> Result<Vec<u8>, SecureStorageError>;

    /// Check if secure storage is available
    ///
    /// Verifies that the underlying secure storage hardware is available and functional.
    async fn is_secure_storage_available(&self) -> bool;

    /// Get secure storage capabilities
    ///
    /// Returns information about what the secure storage implementation supports.
    fn get_secure_storage_capabilities(&self) -> Vec<String>;
}

/// Helper functions for common secure storage operations
impl SecureStorageLocation {
    /// Create a location for storing FROST nonces
    pub fn frost_nonce(session_id: &str, participant_id: u16) -> Self {
        Self::new("frost_nonces", format!("{}_{}", session_id, participant_id))
    }

    /// Create a location for storing signing shares
    pub fn signing_share(account_id: &str, epoch: u64, participant_id: u16) -> Self {
        Self::with_sub_key(
            "signing_shares",
            account_id,
            format!("{}_{}", epoch, participant_id),
        )
    }

    /// Create a location for device attestation certificates
    pub fn device_attestation(device_id: &str) -> Self {
        Self::new("device_attestation", device_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_storage_location_creation() {
        let location = SecureStorageLocation::new("test", "key1");
        assert_eq!(location.namespace, "test");
        assert_eq!(location.key, "key1");
        assert_eq!(location.sub_key, None);
        assert_eq!(location.full_path(), "test/key1");
    }

    #[test]
    fn test_secure_storage_location_with_sub_key() {
        let location = SecureStorageLocation::with_sub_key("test", "key1", "subkey1");
        assert_eq!(location.namespace, "test");
        assert_eq!(location.key, "key1");
        assert_eq!(location.sub_key, Some("subkey1".to_string()));
        assert_eq!(location.full_path(), "test/key1/subkey1");
    }

    #[test]
    fn test_frost_nonce_location() {
        let location = SecureStorageLocation::frost_nonce("session123", 1);
        assert_eq!(location.namespace, "frost_nonces");
        assert_eq!(location.key, "session123_1");
        assert_eq!(location.full_path(), "frost_nonces/session123_1");
    }

    #[test]
    fn test_signing_share_location() {
        let location = SecureStorageLocation::signing_share("account456", 42, 2);
        assert_eq!(location.namespace, "signing_shares");
        assert_eq!(location.key, "account456");
        assert_eq!(location.sub_key, Some("42_2".to_string()));
        assert_eq!(location.full_path(), "signing_shares/account456/42_2");
    }

    #[test]
    fn test_device_attestation_location() {
        let location = SecureStorageLocation::device_attestation("device789");
        assert_eq!(location.namespace, "device_attestation");
        assert_eq!(location.key, "device789");
        assert_eq!(location.full_path(), "device_attestation/device789");
    }
}
