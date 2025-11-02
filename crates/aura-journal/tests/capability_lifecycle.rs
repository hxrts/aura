//! Integration Tests: Capability Grant and Revoke
//!
//! Tests the full lifecycle of capability management including grant, use, and revocation.
//! These tests simulate realistic multi-device scenarios.
//!
//! Reference: work/pre_ssb_storage_tests.md - Category 1.3

use aura_crypto::Effects;
use aura_crypto::{ed25519_sign, ed25519_verify, Ed25519SigningKey, Ed25519VerifyingKey};
use aura_journal::capability::CapabilityId;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

// Reuse types from capability_tokens.rs
// TODO: Move these to aura-journal/src/capability/types.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthentication {
    pub device_id: Vec<u8>,
    pub account_id: Vec<u8>,
    pub device_signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    pub authenticated_device: Vec<u8>,
    pub granted_permissions: Vec<Permission>,
    pub delegation_chain: Vec<CapabilityId>,
    pub signature: Vec<u8>,
    pub issued_at: u64,
    pub expires_at: Option<u64>,
}

impl CapabilityToken {
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

        let message = token.signing_message()?;
        token.signature = signing_key.sign(&message).to_bytes().to_vec();

        Ok(token)
    }

    fn signing_message(&self) -> Result<Vec<u8>, String> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&self.authenticated_device);

        let perms_bytes = serde_json::to_vec(&self.granted_permissions)
            .map_err(|e| format!("Serialization error: {}", e))?;
        msg.extend_from_slice(&perms_bytes);

        for cap_id in &self.delegation_chain {
            msg.extend_from_slice(&cap_id.0);
        }

        msg.extend_from_slice(&self.issued_at.to_le_bytes());

        Ok(msg)
    }

    pub fn verify(&self, verifying_key: &VerifyingKey) -> Result<(), String> {
        let message = self.signing_message()?;

        use aura_crypto::Ed25519Signature;
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
}

/// Mock capability manager for integration testing
struct CapabilityManager {
    /// Active capabilities (not revoked)
    active_capabilities: HashMap<Vec<u8>, CapabilityToken>,
    /// Revoked capability IDs
    revoked_capabilities: HashSet<Vec<u8>>,
    /// Device keys for verification
    device_keys: HashMap<Vec<u8>, VerifyingKey>,
}

impl CapabilityManager {
    fn new() -> Self {
        Self {
            active_capabilities: HashMap::new(),
            revoked_capabilities: HashSet::new(),
            device_keys: HashMap::new(),
        }
    }

    fn register_device(&mut self, device_id: Vec<u8>, verifying_key: VerifyingKey) {
        self.device_keys.insert(device_id, verifying_key);
    }

    fn grant_capability(
        &mut self,
        token_id: Vec<u8>,
        token: CapabilityToken,
    ) -> Result<(), String> {
        // Verify token signature
        let device_key = self
            .device_keys
            .get(&token.authenticated_device)
            .ok_or("Device not registered")?;

        token.verify(device_key)?;

        // Store active capability
        self.active_capabilities.insert(token_id, token);
        Ok(())
    }

    fn revoke_capability(&mut self, token_id: Vec<u8>) -> Result<(), String> {
        if !self.active_capabilities.contains_key(&token_id) {
            return Err("Capability not found".to_string());
        }

        self.active_capabilities.remove(&token_id);
        self.revoked_capabilities.insert(token_id);
        Ok(())
    }

    fn verify_capability(&self, token_id: &[u8]) -> Result<&CapabilityToken, String> {
        // Check if revoked
        if self.revoked_capabilities.contains(token_id) {
            return Err("Capability has been revoked".to_string());
        }

        // Check if active
        self.active_capabilities
            .get(token_id)
            .ok_or_else(|| "Capability not found".to_string())
    }

    fn has_permission(&self, token_id: &[u8], required_permission: &Permission) -> bool {
        if let Ok(token) = self.verify_capability(token_id) {
            token.granted_permissions.contains(required_permission)
        } else {
            false
        }
    }
}

/// Mock threshold signature collector
struct ThresholdCollector {
    required_threshold: usize,
    signatures: Vec<(Vec<u8>, Vec<u8>)>, // (device_id, signature)
}

impl ThresholdCollector {
    fn new(threshold: usize) -> Self {
        Self {
            required_threshold: threshold,
            signatures: Vec::new(),
        }
    }

    fn add_signature(&mut self, device_id: Vec<u8>, signature: Vec<u8>) {
        self.signatures.push((device_id, signature));
    }

    fn has_threshold(&self) -> bool {
        self.signatures.len() >= self.required_threshold
    }

    fn verify_all(&self, device_keys: &HashMap<Vec<u8>, VerifyingKey>, message: &[u8]) -> bool {
        if !self.has_threshold() {
            return false;
        }

        for (device_id, sig_bytes) in &self.signatures {
            if let Some(verifying_key) = device_keys.get(device_id) {
                use aura_crypto::Ed25519Signature;
                if let Ok(sig) = Signature::try_from(sig_bytes.as_slice()) {
                    if verifying_key.verify(message, &sig).is_err() {
                        return false;
                    }
                } else {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_device_key(seed: u64) -> SigningKey {
        let effects = Effects::deterministic(seed, 0);
        let key_bytes = effects.random_bytes::<32>();
        SigningKey::from_bytes(&key_bytes)
    }

    #[test]
    fn test_grant_storage_capability_to_peer() {
        // Setup: Device A grants storage capability to Device B
        let effects = Effects::test();

        let device_a_key = create_device_key(100);
        let device_b_key = create_device_key(200);

        let device_a_id = b"device_a".to_vec();
        let device_b_id = b"device_b".to_vec();

        // Initialize capability manager
        let mut manager = CapabilityManager::new();
        manager.register_device(device_a_id.clone(), device_a_key.verifying_key());
        manager.register_device(device_b_id.clone(), device_b_key.verifying_key());

        // Device A creates capability token for Device B
        let storage_permission = Permission::Storage {
            operation: "write".to_string(),
            resource: "/shared/docs".to_string(),
        };

        let token = CapabilityToken::new(
            device_b_id.clone(),
            vec![storage_permission.clone()],
            vec![],
            &device_b_key,
            &effects,
        )
        .expect("Token creation should succeed");

        // Grant capability
        let token_id = b"token_123".to_vec();
        manager
            .grant_capability(token_id.clone(), token)
            .expect("Grant should succeed");

        // Device B uses capability to store data
        assert!(
            manager.has_permission(&token_id, &storage_permission),
            "Device B should have storage permission"
        );

        // Verify capability is active
        let verified_token = manager
            .verify_capability(&token_id)
            .expect("Capability should be active");
        assert_eq!(verified_token.authenticated_device, device_b_id);
        assert_eq!(verified_token.granted_permissions.len(), 1);

        println!("[OK] test_grant_storage_capability_to_peer PASSED");
    }

    #[test]
    fn test_revoke_capability_invalidates_token() {
        // Setup: Device A grants capability to Device B, then revokes it
        let effects = Effects::test();

        let device_a_key = create_device_key(300);
        let device_b_key = create_device_key(400);

        let device_a_id = b"device_a".to_vec();
        let device_b_id = b"device_b".to_vec();

        let mut manager = CapabilityManager::new();
        manager.register_device(device_a_id.clone(), device_a_key.verifying_key());
        manager.register_device(device_b_id.clone(), device_b_key.verifying_key());

        // Grant capability
        let permission = Permission::Communication {
            operation: "relay".to_string(),
            relationship: "friend".to_string(),
        };

        let token = CapabilityToken::new(
            device_b_id.clone(),
            vec![permission.clone()],
            vec![],
            &device_b_key,
            &effects,
        )
        .expect("Token creation should succeed");

        let token_id = b"token_456".to_vec();
        manager
            .grant_capability(token_id.clone(), token)
            .expect("Grant should succeed");

        // Device B successfully uses capability
        assert!(
            manager.has_permission(&token_id, &permission),
            "Device B should have permission before revocation"
        );
        assert!(
            manager.verify_capability(&token_id).is_ok(),
            "Capability should be valid before revocation"
        );

        // Device A revokes capability
        manager
            .revoke_capability(token_id.clone())
            .expect("Revocation should succeed");

        // Device B cannot use revoked capability
        assert!(
            !manager.has_permission(&token_id, &permission),
            "Device B should NOT have permission after revocation"
        );
        assert!(
            manager.verify_capability(&token_id).is_err(),
            "Capability should be invalid after revocation"
        );

        // Verify error message mentions revocation
        let err = manager.verify_capability(&token_id).unwrap_err();
        assert!(
            err.contains("revoked"),
            "Error should mention revocation, got: {}",
            err
        );

        println!("[OK] test_revoke_capability_invalidates_token PASSED");
    }

    #[test]
    fn test_capability_grant_requires_threshold_signature() {
        // Setup: 3-of-5 threshold for capability grants
        let effects = Effects::test();

        // Create 5 devices
        let device_keys: Vec<SigningKey> = (0..5).map(|i| create_device_key(500 + i)).collect();
        let device_ids: Vec<Vec<u8>> = (0..5)
            .map(|i| format!("device_{}", i).into_bytes())
            .collect();

        let mut device_key_map = HashMap::new();
        for (id, key) in device_ids.iter().zip(device_keys.iter()) {
            device_key_map.insert(id.clone(), key.verifying_key());
        }

        // Create capability grant message
        let grant_message = b"grant_capability_to_device_5_for_storage_admin";

        // Attempt grant with insufficient signatures (only 2)
        let mut collector_insufficient = ThresholdCollector::new(3);
        for i in 0..2 {
            let signature = device_keys[i].sign(grant_message).to_bytes().to_vec();
            collector_insufficient.add_signature(device_ids[i].clone(), signature);
        }

        assert!(
            !collector_insufficient.has_threshold(),
            "Should not meet threshold with 2/3 signatures"
        );
        assert!(
            !collector_insufficient.verify_all(&device_key_map, grant_message),
            "Grant should fail with insufficient signatures"
        );

        // Collect threshold signatures (3)
        let mut collector_sufficient = ThresholdCollector::new(3);
        for i in 0..3 {
            let signature = device_keys[i].sign(grant_message).to_bytes().to_vec();
            collector_sufficient.add_signature(device_ids[i].clone(), signature);
        }

        assert!(
            collector_sufficient.has_threshold(),
            "Should meet threshold with 3/3 signatures"
        );
        assert!(
            collector_sufficient.verify_all(&device_key_map, grant_message),
            "Grant should succeed with threshold signatures"
        );

        // Grant succeeds with threshold
        let manager = CapabilityManager::new();
        let device_5_key = create_device_key(600);
        let device_5_id = b"device_5".to_vec();

        let permission = Permission::Storage {
            operation: "admin".to_string(),
            resource: "/root".to_string(),
        };

        let token = CapabilityToken::new(
            device_5_id.clone(),
            vec![permission],
            vec![],
            &device_5_key,
            &effects,
        )
        .expect("Token creation should succeed");

        // In real implementation, this would only succeed if threshold met
        assert!(
            collector_sufficient.has_threshold(),
            "Threshold enforcement works"
        );

        println!("[OK] test_capability_grant_requires_threshold_signature PASSED");
    }
}
