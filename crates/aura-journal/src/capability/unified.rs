//! Unified capability system for Storage, Communication, and Relay permissions
//!
//! This module implements the clean capability model from docs/040_storage.md and
//! docs/041_rendezvous.md with clear separation between authentication (who you are)
//! and authorization (what you can do).

use super::CapabilityId;
use aura_crypto::Effects;
use aura_crypto::{Ed25519Signature, Ed25519SigningKey, Ed25519VerifyingKey};
use aura_types::DeviceId;
use serde::{Deserialize, Serialize};

/// Permission enum with four scopes: Storage, Communication, Relay, and DeviceAuth
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    /// Storage access permission
    Storage {
        operation: StorageOperation,
        resource: String,
    },
    /// Communication scope permission
    Communication {
        operation: CommunicationOperation,
        relationship: String,
    },
    /// Relay permission with trust level
    Relay {
        operation: RelayOperation,
        trust_level: String,
    },
    /// Device authentication permission
    DeviceAuth(DeviceAuthentication),
}

/// Storage operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageOperation {
    Read,
    Write,
    Delete,
    Replicate,
}

/// Communication operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommunicationOperation {
    Send,
    Receive,
    Subscribe,
}

/// Relay operations
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelayOperation {
    Forward,
    Store,
    Announce,
}

/// DeviceAuthentication separates identity proof from permissions
///
/// This struct answers "who you are" without granting any permissions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAuthentication {
    /// Device identifier
    pub device_id: DeviceId,
    /// Account identifier (the device belongs to this account)
    pub account_id: Vec<u8>,
    /// Device signature proving control of device key
    pub device_signature: Vec<u8>,
}

impl DeviceAuthentication {
    /// Create a new device authentication
    pub fn new(
        device_id: DeviceId,
        account_id: Vec<u8>,
        signing_key: &Ed25519SigningKey,
    ) -> Result<Self, String> {
        // Create authentication payload
        let mut payload = Vec::new();
        payload.extend_from_slice(device_id.0.as_bytes());
        payload.extend_from_slice(&account_id);

        // Sign the payload
        let signature: Ed25519Signature = aura_crypto::ed25519_sign(signing_key, &payload);

        Ok(Self {
            device_id,
            account_id,
            device_signature: signature.to_vec(),
        })
    }

    /// Verify device authentication
    pub fn verify(&self, verifying_key: &Ed25519VerifyingKey) -> Result<(), String> {
        // Reconstruct authentication payload
        let mut payload = Vec::new();
        payload.extend_from_slice(self.device_id.0.as_bytes());
        payload.extend_from_slice(&self.account_id);

        // Parse signature
        let signature = aura_crypto::Ed25519Signature::from_slice(&self.device_signature)
            .map_err(|e| format!("Invalid signature format: {:?}", e))?;

        // Verify signature
        aura_crypto::ed25519_verify(verifying_key, &payload, &signature)
            .map_err(|e| format!("Invalid device signature: {:?}", e))
    }
}

/// CapabilityToken combines device authentication with granted permissions
///
/// This answers both "who you are" (authentication) and "what you can do" (authorization).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Authenticated device
    pub authenticated_device: DeviceId,
    /// Granted permissions
    pub granted_permissions: Vec<Permission>,
    /// Delegation chain for traceability
    pub delegation_chain: Vec<CapabilityId>,
    /// Token signature from granting authority
    pub signature: Vec<u8>,
    /// Issuance timestamp
    pub issued_at: u64,
    /// Optional expiration timestamp
    pub expires_at: Option<u64>,
}

impl CapabilityToken {
    /// Create a new capability token
    pub fn new(
        device_id: DeviceId,
        permissions: Vec<Permission>,
        delegation_chain: Vec<CapabilityId>,
        signing_key: &Ed25519SigningKey,
        effects: &Effects,
    ) -> Result<Self, String> {
        let issued_at = effects
            .now()
            .map_err(|e| format!("Failed to get time: {:?}", e))?;

        // Serialize permissions for signing
        let permissions_bytes = bincode::serialize(&permissions)
            .map_err(|e| format!("Failed to serialize permissions: {}", e))?;

        // Create signing payload
        let mut payload = Vec::new();
        payload.extend_from_slice(device_id.0.as_bytes());
        payload.extend_from_slice(&permissions_bytes);
        payload.extend_from_slice(&issued_at.to_le_bytes());

        // Sign the token
        let signature: Ed25519Signature = aura_crypto::ed25519_sign(signing_key, &payload);

        Ok(Self {
            authenticated_device: device_id,
            granted_permissions: permissions,
            delegation_chain,
            signature: signature.to_vec(),
            issued_at,
            expires_at: None,
        })
    }

    /// Set expiration time
    pub fn with_expiration(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Get capability ID (hash of token)
    pub fn capability_id(&self) -> CapabilityId {
        let mut hasher = aura_crypto::blake3_hasher();
        hasher.update(self.authenticated_device.0.as_bytes());
        hasher.update(&self.issued_at.to_le_bytes());
        hasher.update(&self.signature);
        CapabilityId(hasher.finalize().into())
    }

    /// Verify capability token signature
    pub fn verify(&self, verifying_key: &Ed25519VerifyingKey) -> Result<(), String> {
        // Serialize permissions
        let permissions_bytes = bincode::serialize(&self.granted_permissions)
            .map_err(|e| format!("Failed to serialize permissions: {}", e))?;

        // Reconstruct signing payload
        let mut payload = Vec::new();
        payload.extend_from_slice(self.authenticated_device.0.as_bytes());
        payload.extend_from_slice(&permissions_bytes);
        payload.extend_from_slice(&self.issued_at.to_le_bytes());

        // Parse signature
        let signature = aura_crypto::Ed25519Signature::from_slice(&self.signature)
            .map_err(|e| format!("Invalid signature format: {:?}", e))?;

        // Verify signature
        aura_crypto::ed25519_verify(verifying_key, &payload, &signature)
            .map_err(|e| format!("Invalid capability token signature: {:?}", e))
    }

    /// Check if token is expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        if let Some(expires_at) = self.expires_at {
            current_time >= expires_at
        } else {
            false
        }
    }

    /// Check if token has specific permission
    pub fn has_permission(&self, permission: &Permission) -> bool {
        self.granted_permissions.iter().any(|p| p == permission)
    }

    /// Check if token permissions are a subset of given permissions
    pub fn is_subset_of(&self, superset: &[Permission]) -> bool {
        self.granted_permissions
            .iter()
            .all(|p| superset.contains(p))
    }
}

/// Capability verification functions with clear authentication/authorization separation
pub mod verification {
    use super::*;

    /// Verify device authentication only (answers "who are you?")
    pub fn verify_authentication(
        auth: &DeviceAuthentication,
        verifying_key: &Ed25519VerifyingKey,
    ) -> Result<(), String> {
        auth.verify(verifying_key)
    }

    /// Verify capability token (answers "who are you?" and "what can you do?")
    pub fn verify_capability(
        token: &CapabilityToken,
        verifying_key: &Ed25519VerifyingKey,
        required_permissions: &[Permission],
        current_time: u64,
    ) -> Result<(), String> {
        // First verify signature (authentication)
        token.verify(verifying_key)?;

        // Check expiration
        if token.is_expired(current_time) {
            return Err("Capability token expired".to_string());
        }

        // Check authorization - verify token has all required permissions
        for required in required_permissions {
            if !token.has_permission(required) {
                return Err(format!("Missing required permission: {:?}", required));
            }
        }

        Ok(())
    }

    /// Check if token is authorized for a specific operation (authorization only)
    ///
    /// Assumes authentication already verified.
    pub fn check_authorization(
        token: &CapabilityToken,
        required_permissions: &[Permission],
    ) -> Result<(), String> {
        for required in required_permissions {
            if !token.has_permission(required) {
                return Err(format!("Missing required permission: {:?}", required));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods, clippy::clone_on_copy, clippy::len_zero)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_effects() -> Effects {
        Effects::deterministic(42, 1000)
    }

    #[test]
    fn test_device_authentication() {
        let effects = test_effects();
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = b"test_account".to_vec();

        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let verifying_key = signing_key.verifying_key();

        let auth = DeviceAuthentication::new(device_id, account_id, &signing_key).unwrap();

        assert!(auth.verify(&verifying_key).is_ok());
    }

    #[test]
    fn test_capability_token() {
        let effects = test_effects();
        let device_id = DeviceId(Uuid::new_v4());

        let permissions = vec![
            Permission::Storage {
                operation: StorageOperation::Read,
                resource: "file1".to_string(),
            },
            Permission::Communication {
                operation: CommunicationOperation::Send,
                relationship: "alice-bob".to_string(),
            },
        ];

        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let verifying_key = signing_key.verifying_key();

        let token = CapabilityToken::new(
            device_id,
            permissions.clone(),
            vec![],
            &signing_key,
            &effects,
        )
        .unwrap();

        assert!(token.verify(&verifying_key).is_ok());
        assert!(!token.is_expired(effects.now().unwrap()));

        assert!(token.has_permission(&permissions[0]));
        assert!(token.has_permission(&permissions[1]));
    }

    #[test]
    fn test_capability_expiration() {
        let effects = test_effects();
        let device_id = DeviceId(Uuid::new_v4());

        let permissions = vec![Permission::Storage {
            operation: StorageOperation::Read,
            resource: "file1".to_string(),
        }];

        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());

        let current_time = effects.now().unwrap();
        let token = CapabilityToken::new(device_id, permissions, vec![], &signing_key, &effects)
            .unwrap()
            .with_expiration(current_time + 1000);

        assert!(!token.is_expired(current_time));
        assert!(!token.is_expired(current_time + 500));
        assert!(token.is_expired(current_time + 1000));
        assert!(token.is_expired(current_time + 2000));
    }

    #[test]
    fn test_permission_checking() {
        let effects = test_effects();
        let device_id = DeviceId(Uuid::new_v4());

        let granted = vec![Permission::Storage {
            operation: StorageOperation::Read,
            resource: "file1".to_string(),
        }];

        let signing_key = aura_crypto::Ed25519SigningKey::from_bytes(&effects.random_bytes::<32>());
        let verifying_key = signing_key.verifying_key();

        let token =
            CapabilityToken::new(device_id, granted.clone(), vec![], &signing_key, &effects)
                .unwrap();

        // Should succeed with granted permission
        let result = verification::verify_capability(
            &token,
            &verifying_key,
            &granted,
            effects.now().unwrap(),
        );
        assert!(result.is_ok());

        // Should fail with different permission
        let different = vec![Permission::Storage {
            operation: StorageOperation::Write,
            resource: "file1".to_string(),
        }];

        let result = verification::verify_capability(
            &token,
            &verifying_key,
            &different,
            effects.now().unwrap(),
        );
        assert!(result.is_err());
    }
}
