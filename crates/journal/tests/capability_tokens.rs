#![allow(warnings, clippy::all)]
//! Unit Tests: Capability Token Creation and Verification
//!
//! Tests basic capability token functionality that both SSB and Storage depend on.
//!
//! Reference: work/pre_ssb_storage_tests.md - Category 1.1

use aura_crypto::Effects;
use aura_journal::capability::{CapabilityId, CapabilityScope, Subject};
use aura_crypto::{Ed25519SigningKey, Ed25519VerifyingKey};
use serde::{Deserialize, Serialize};

// TODO: These types will be added to aura-journal/src/capability/types.rs
// For now, we define them here to write the tests

/// Device authentication - proves "who you are"
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthentication {
    pub device_id: Vec<u8>,
    pub account_id: Vec<u8>,
    pub device_signature: Vec<u8>,
}

/// Permission scopes for different subsystems
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    Storage {
        operation: String,
        resource: String,
    },
    Communication {
        operation: String,
        relationship: String,
    },
    Relay {
        operation: String,
        trust_level: String,
    },
}

/// Capability token - combines authentication with authorization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    pub authenticated_device: Vec<u8>, // DeviceId
    pub granted_permissions: Vec<Permission>,
    pub delegation_chain: Vec<CapabilityId>,
    pub signature: Vec<u8>,
    pub issued_at: u64,
    pub expires_at: Option<u64>,
}

impl CapabilityToken {
    /// Create a new capability token
    pub fn new(
        device_id: Vec<u8>,
        permissions: Vec<Permission>,
        delegation_chain: Vec<CapabilityId>,
        signing_key: &SigningKey,
        effects: &Effects,
    ) -> Result<Self, String> {
        let issued_at = effects.now().map_err(|e| format!("Time error: {:?}", e))?;

        let mut token = Self {
            authenticated_device: device_id,
            granted_permissions: permissions,
            delegation_chain,
            signature: Vec::new(),
            issued_at,
            expires_at: None,
        };

        // Sign the token
        let message = token.signing_message()?;
        use aura_crypto::Signer;
        token.signature = signing_key.sign(&message).to_bytes().to_vec();

        Ok(token)
    }

    /// Get the message that should be signed
    fn signing_message(&self) -> Result<Vec<u8>, String> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&self.authenticated_device);

        // Serialize permissions
        let perms_bytes = serde_json::to_vec(&self.granted_permissions)
            .map_err(|e| format!("Serialization error: {}", e))?;
        msg.extend_from_slice(&perms_bytes);

        // Add delegation chain
        for cap_id in &self.delegation_chain {
            msg.extend_from_slice(&cap_id.0);
        }

        // Add timestamp
        msg.extend_from_slice(&self.issued_at.to_le_bytes());

        Ok(msg)
    }

    /// Verify the token signature
    pub fn verify(&self, verifying_key: &VerifyingKey) -> Result<(), String> {
        let message = self.signing_message()?;

        use aura_crypto::{Ed25519Signature, ed25519_verify};
        let sig = Signature::from_bytes(
            self.signature
                .as_slice()
                .try_into()
                .map_err(|_| "Invalid signature length")?,
        );

        verifying_key
            .verify(&message, &sig)
            .map_err(|e| format!("Signature verification failed: {}", e))
    }

    /// Check if token has expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        if let Some(expires_at) = self.expires_at {
            current_time >= expires_at
        } else {
            false
        }
    }

    /// Check if token has a specific permission
    pub fn has_permission(&self, permission: &Permission) -> bool {
        self.granted_permissions.contains(permission)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_crypto::Ed25519SigningKey;

    fn create_test_device_key() -> SigningKey {
        let effects = Effects::deterministic(42, 0);
        let key_bytes = effects.random_bytes::<32>();
        SigningKey::from_bytes(&key_bytes)
    }

    #[test]
    fn test_capability_token_creation_with_device_auth() {
        // Setup
        let effects = Effects::test();
        let device_key = create_test_device_key();
        let device_id = b"device_123".to_vec();

        let permissions = vec![Permission::Storage {
            operation: "write".to_string(),
            resource: "/docs".to_string(),
        }];

        // Create token
        let token = CapabilityToken::new(
            device_id.clone(),
            permissions.clone(),
            vec![],
            &device_key,
            &effects,
        )
        .expect("Token creation should succeed");

        // Verify token structure
        assert_eq!(token.authenticated_device, device_id);
        assert_eq!(token.granted_permissions.len(), 1);
        assert_eq!(token.granted_permissions[0], permissions[0]);
        assert!(!token.signature.is_empty());

        // Verify signature
        let verifying_key = device_key.verifying_key();
        assert!(
            token.verify(&verifying_key).is_ok(),
            "Token signature should verify"
        );

        println!("[OK] test_capability_token_creation_with_device_auth PASSED");
    }

    #[test]
    fn test_capability_token_multiple_permission_scopes() {
        // Setup
        let effects = Effects::test();
        let device_key = create_test_device_key();
        let device_id = b"device_456".to_vec();

        let permissions = vec![
            Permission::Storage {
                operation: "read".to_string(),
                resource: "/public".to_string(),
            },
            Permission::Communication {
                operation: "relay".to_string(),
                relationship: "friend".to_string(),
            },
            Permission::Relay {
                operation: "forward".to_string(),
                trust_level: "high".to_string(),
            },
        ];

        // Create token with mixed permissions
        let token = CapabilityToken::new(
            device_id,
            permissions.clone(),
            vec![],
            &device_key,
            &effects,
        )
        .expect("Token creation should succeed");

        // Verify each permission scope
        assert!(
            token.has_permission(&permissions[0]),
            "Should have storage permission"
        );
        assert!(
            token.has_permission(&permissions[1]),
            "Should have communication permission"
        );
        assert!(
            token.has_permission(&permissions[2]),
            "Should have relay permission"
        );

        // Verify signature still works with multiple permissions
        let verifying_key = device_key.verifying_key();
        assert!(
            token.verify(&verifying_key).is_ok(),
            "Mixed permissions token should verify"
        );

        println!("[OK] test_capability_token_multiple_permission_scopes PASSED");
    }

    #[test]
    fn test_capability_token_delegation_chain() {
        // Setup
        let effects = Effects::test();
        let device_key = create_test_device_key();
        let device_id = b"device_789".to_vec();

        // Create delegation chain A → B → C
        let cap_a = CapabilityId::from_chain(
            None,
            &Subject::new("delegator_a"),
            &CapabilityScope::simple("storage", "admin"),
        );

        let cap_b = CapabilityId::from_chain(
            Some(&cap_a),
            &Subject::new("delegator_b"),
            &CapabilityScope::simple("storage", "write"),
        );

        let cap_c = CapabilityId::from_chain(
            Some(&cap_b),
            &Subject::new("delegator_c"),
            &CapabilityScope::simple("storage", "read"),
        );

        let delegation_chain = vec![cap_a.clone(), cap_b.clone(), cap_c.clone()];

        let permissions = vec![Permission::Storage {
            operation: "read".to_string(),
            resource: "/data".to_string(),
        }];

        // Create token with delegation chain
        let token = CapabilityToken::new(
            device_id,
            permissions,
            delegation_chain.clone(),
            &device_key,
            &effects,
        )
        .expect("Token creation should succeed");

        // Verify delegation chain is preserved
        assert_eq!(token.delegation_chain.len(), 3);
        assert_eq!(token.delegation_chain[0], cap_a);
        assert_eq!(token.delegation_chain[1], cap_b);
        assert_eq!(token.delegation_chain[2], cap_c);

        // Verify signature includes delegation chain
        let verifying_key = device_key.verifying_key();
        assert!(
            token.verify(&verifying_key).is_ok(),
            "Delegation chain token should verify"
        );

        println!("[OK] test_capability_token_delegation_chain PASSED");
    }

    #[test]
    fn test_capability_token_expiration() {
        // Setup
        let effects = Effects::test();
        let device_key = create_test_device_key();
        let device_id = b"device_exp".to_vec();

        let permissions = vec![Permission::Storage {
            operation: "write".to_string(),
            resource: "/temp".to_string(),
        }];

        // Create token
        let mut token = CapabilityToken::new(device_id, permissions, vec![], &device_key, &effects)
            .expect("Token creation should succeed");

        // Set expiration 3600 seconds in future
        let issued_at = token.issued_at;
        token.expires_at = Some(issued_at + 3600);

        // Token valid before expiration
        assert!(
            !token.is_expired(issued_at + 1800),
            "Token should be valid before expiration"
        );
        assert!(
            !token.is_expired(issued_at + 3599),
            "Token should be valid just before expiration"
        );

        // Token invalid after expiration
        assert!(
            token.is_expired(issued_at + 3600),
            "Token should be invalid at expiration"
        );
        assert!(
            token.is_expired(issued_at + 7200),
            "Token should be invalid after expiration"
        );

        println!("[OK] test_capability_token_expiration PASSED");
    }

    #[test]
    fn test_capability_token_signature_verification_fails_with_wrong_key() {
        // Setup
        let effects = Effects::test();
        let device_key = create_test_device_key();
        let device_id = b"device_wrong".to_vec();

        let permissions = vec![Permission::Storage {
            operation: "read".to_string(),
            resource: "/data".to_string(),
        }];

        // Create token with device_key
        let token = CapabilityToken::new(device_id, permissions, vec![], &device_key, &effects)
            .expect("Token creation should succeed");

        // Try to verify with different key
        let effects_wrong = Effects::deterministic(999, 0);
        let wrong_key_bytes = effects_wrong.random_bytes::<32>();
        let wrong_key = SigningKey::from_bytes(&wrong_key_bytes);
        let wrong_verifying_key = wrong_key.verifying_key();

        // Verification should fail
        assert!(
            token.verify(&wrong_verifying_key).is_err(),
            "Verification with wrong key should fail"
        );

        // Verification should succeed with correct key
        let correct_verifying_key = device_key.verifying_key();
        assert!(
            token.verify(&correct_verifying_key).is_ok(),
            "Verification with correct key should succeed"
        );

        println!("[OK] test_capability_token_signature_verification_fails_with_wrong_key PASSED");
    }
}
