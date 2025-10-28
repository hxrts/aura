//! Device key management for authentication and message signing
//!
//! This module provides secure device key generation, storage, and signing capabilities
//! for the Aura transport layer authentication system.

use crate::{CryptoError, Effects, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Device signing key for message authentication
#[derive(Debug, Clone)]
pub struct DeviceSigningKey {
    /// Device identifier
    pub device_id: Uuid,
    /// Ed25519 signing key
    signing_key: SigningKey,
    /// Corresponding public key
    pub verifying_key: VerifyingKey,
    /// Key creation timestamp
    pub created_at: u64,
}

/// Device key manager for secure key storage and operations
pub struct DeviceKeyManager {
    /// Active device signing key
    device_key: Option<DeviceSigningKey>,
    /// Known public keys by device ID
    known_public_keys: BTreeMap<Uuid, VerifyingKey>,
    /// Injectable effects for deterministic testing
    effects: Effects,
}

impl DeviceSigningKey {
    /// Generate new device signing key
    pub fn generate(device_id: Uuid, effects: &Effects) -> Result<Self> {
        // Generate cryptographically secure random bytes for key material
        let key_bytes = effects.random_bytes::<32>();

        let signing_key = SigningKey::from_bytes(&key_bytes);
        let verifying_key = signing_key.verifying_key();
        let created_at = effects.now().unwrap_or(0);

        Ok(Self {
            device_id,
            signing_key,
            verifying_key,
            created_at,
        })
    }

    /// Sign message content with device key
    pub fn sign_message(&self, message_content: &[u8]) -> Vec<u8> {
        let signature = self.signing_key.sign(message_content);
        signature.to_bytes().to_vec()
    }

    /// Get public key bytes
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.verifying_key.to_bytes()
    }

    /// Export signing key for secure storage (in production, this would be encrypted)
    pub fn export_for_storage(&self) -> DeviceKeyStorage {
        DeviceKeyStorage {
            device_id: self.device_id,
            signing_key_bytes: self.signing_key.to_bytes(),
            public_key_bytes: self.verifying_key.to_bytes(),
            created_at: self.created_at,
        }
    }

    /// Import signing key from secure storage
    pub fn import_from_storage(storage: DeviceKeyStorage) -> Result<Self> {
        let signing_key = SigningKey::from_bytes(&storage.signing_key_bytes);
        let verifying_key = VerifyingKey::from_bytes(&storage.public_key_bytes).map_err(|e| {
            CryptoError::crypto_operation_failed(format!("Invalid public key: {}", e))
        })?;

        Ok(Self {
            device_id: storage.device_id,
            signing_key,
            verifying_key,
            created_at: storage.created_at,
        })
    }
}

/// Serializable device key storage format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceKeyStorage {
    /// Device identifier
    pub device_id: Uuid,
    /// Signing key bytes (encrypted in production)
    pub signing_key_bytes: [u8; 32],
    /// Public key bytes
    pub public_key_bytes: [u8; 32],
    /// Creation timestamp
    pub created_at: u64,
}

impl DeviceKeyManager {
    /// Create new device key manager
    pub fn new(effects: Effects) -> Self {
        Self {
            device_key: None,
            known_public_keys: BTreeMap::new(),
            effects,
        }
    }

    /// Generate and set device key for this device
    pub fn generate_device_key(&mut self, device_id: Uuid) -> Result<()> {
        let device_key = DeviceSigningKey::generate(device_id, &self.effects)?;

        // Store our own public key
        self.known_public_keys
            .insert(device_id, device_key.verifying_key);

        self.device_key = Some(device_key);
        Ok(())
    }

    /// Load device key from storage
    pub fn load_device_key(&mut self, storage: DeviceKeyStorage) -> Result<()> {
        let device_key = DeviceSigningKey::import_from_storage(storage)?;
        let device_id = device_key.device_id;

        // Store our own public key
        self.known_public_keys
            .insert(device_id, device_key.verifying_key);

        self.device_key = Some(device_key);
        Ok(())
    }

    /// Get current device key
    pub fn get_device_key(&self) -> Option<&DeviceSigningKey> {
        self.device_key.as_ref()
    }

    /// Sign message with device key
    pub fn sign_message(&self, message_content: &[u8]) -> Result<Vec<u8>> {
        let device_key = self.device_key.as_ref().ok_or_else(|| {
            CryptoError::crypto_operation_failed("No device key available".to_string())
        })?;

        Ok(device_key.sign_message(message_content))
    }

    /// Add known public key for device
    pub fn add_public_key(&mut self, device_id: Uuid, public_key_bytes: [u8; 32]) -> Result<()> {
        let verifying_key = VerifyingKey::from_bytes(&public_key_bytes).map_err(|e| {
            CryptoError::crypto_operation_failed(format!("Invalid public key: {}", e))
        })?;

        self.known_public_keys.insert(device_id, verifying_key);
        Ok(())
    }

    /// Verify message signature against known public key
    pub fn verify_message_signature(
        &self,
        device_id: Uuid,
        message_content: &[u8],
        signature_bytes: &[u8],
    ) -> Result<bool> {
        // Get public key for device
        let public_key = self.known_public_keys.get(&device_id).ok_or_else(|| {
            CryptoError::crypto_operation_failed(format!("Unknown device: {}", device_id))
        })?;

        // Parse signature
        if signature_bytes.len() != 64 {
            return Ok(false);
        }

        let signature = Signature::from_bytes(
            signature_bytes
                .try_into()
                .map_err(|_| CryptoError::invalid_signature("Invalid signature format"))?,
        );

        // Verify signature
        match public_key.verify(message_content, &signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Get public key for device
    pub fn get_public_key(&self, device_id: Uuid) -> Option<[u8; 32]> {
        self.known_public_keys
            .get(&device_id)
            .map(|key| key.to_bytes())
    }

    /// Export device key for secure storage
    pub fn export_device_key(&self) -> Result<DeviceKeyStorage> {
        let device_key = self.device_key.as_ref().ok_or_else(|| {
            CryptoError::crypto_operation_failed("No device key to export".to_string())
        })?;

        Ok(device_key.export_for_storage())
    }

    /// Get device ID for current device key
    pub fn get_device_id(&self) -> Option<Uuid> {
        self.device_key.as_ref().map(|key| key.device_id)
    }

    /// Get raw signing key for protocol contexts (legacy compatibility)
    ///
    /// WARNING: This exposes the raw signing key and should only be used
    /// for legacy protocol contexts that haven't been updated to use
    /// the DeviceKeyManager directly.
    pub fn get_raw_signing_key(&self) -> Result<ed25519_dalek::SigningKey> {
        let device_key = self.device_key.as_ref().ok_or_else(|| {
            CryptoError::crypto_operation_failed("No device key available".to_string())
        })?;

        Ok(device_key.signing_key.clone())
    }

    /// Check if we have a device key configured
    pub fn has_device_key(&self) -> bool {
        self.device_key.is_some()
    }

    /// Remove device key (for testing or key rotation)
    pub fn clear_device_key(&mut self) {
        if let Some(device_key) = &self.device_key {
            self.known_public_keys.remove(&device_key.device_id);
        }
        self.device_key = None;
    }

    /// Get all known device IDs
    pub fn get_known_devices(&self) -> Vec<Uuid> {
        self.known_public_keys.keys().cloned().collect()
    }
}

impl Default for DeviceKeyManager {
    fn default() -> Self {
        Self::new(Effects::test())
    }
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;

    #[test]
    fn test_device_key_generation() {
        let effects = Effects::test();
        let device_id = effects.gen_uuid();

        let key = DeviceSigningKey::generate(device_id, &effects).unwrap();
        assert_eq!(key.device_id, device_id);
        assert!(key.created_at > 0);
    }

    #[test]
    fn test_message_signing_and_verification() {
        let effects = Effects::test();
        let mut manager = DeviceKeyManager::new(effects.clone());
        let device_id = effects.gen_uuid();

        // Generate device key
        manager.generate_device_key(device_id).unwrap();

        // Sign message
        let message = b"test message";
        let signature = manager.sign_message(message).unwrap();

        // Verify signature
        let is_valid = manager
            .verify_message_signature(device_id, message, &signature)
            .unwrap();
        assert!(is_valid);

        // Verify with wrong message should fail
        let wrong_message = b"wrong message";
        let is_valid = manager
            .verify_message_signature(device_id, wrong_message, &signature)
            .unwrap();
        assert!(!is_valid);
    }

    #[test]
    fn test_key_storage_export_import() {
        let effects = Effects::test();
        let device_id = effects.gen_uuid();

        // Generate original key
        let original_key = DeviceSigningKey::generate(device_id, &effects).unwrap();
        let original_public_key = original_key.public_key_bytes();

        // Export and import
        let storage = original_key.export_for_storage();
        let imported_key = DeviceSigningKey::import_from_storage(storage).unwrap();

        // Keys should be identical
        assert_eq!(imported_key.device_id, original_key.device_id);
        assert_eq!(imported_key.public_key_bytes(), original_public_key);
        assert_eq!(imported_key.created_at, original_key.created_at);

        // Should be able to sign with imported key
        let message = b"test message";
        let signature = imported_key.sign_message(message);

        // Original key should be able to verify signature from imported key
        let mut manager = DeviceKeyManager::new(effects);
        manager
            .add_public_key(device_id, original_public_key)
            .unwrap();
        let is_valid = manager
            .verify_message_signature(device_id, message, &signature)
            .unwrap();
        assert!(is_valid);
    }
}
