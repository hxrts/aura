//! Relationship Key Derivation for SBB
//!
//! This module provides bidirectional key derivation for Social Bulletin Board (SBB)
//! envelope encryption. It extends the existing DKD system to create deterministic
//! relationship keys that Alice and Bob can derive independently.

use aura_core::hash::hasher;
use aura_core::{derive_encryption_key, IdentityKeyContext, KeyDerivationSpec};
use aura_core::{AuraError, AuraResult, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 32-byte relationship encryption key
pub type RelationshipKey = [u8; 32];

/// Relationship key context for deterministic derivation
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RelationshipContext {
    /// Lower device ID (lexicographically)
    device_a: DeviceId,
    /// Higher device ID (lexicographically)
    device_b: DeviceId,
    /// Application context (e.g., "sbb-envelope", "direct-message")
    app_context: String,
    /// Key rotation epoch
    epoch: u64,
}

/// Relationship key manager for SBB envelope encryption
#[derive(Debug)]
pub struct RelationshipKeyManager {
    /// This device's ID
    device_id: DeviceId,
    /// Root key material for derivation (would come from DKD)
    root_key: [u8; 32],
    /// Cached relationship keys
    key_cache: HashMap<RelationshipContext, RelationshipKey>,
    /// Current epoch for key rotation
    current_epoch: u64,
}

impl RelationshipContext {
    /// Create new relationship context with deterministic device ordering
    pub fn new(device_a: DeviceId, device_b: DeviceId, app_context: String, epoch: u64) -> Self {
        // Ensure deterministic ordering (Alice and Bob get same context)
        let (device_a, device_b) = if device_a.0 < device_b.0 {
            (device_a, device_b)
        } else {
            (device_b, device_a)
        };

        Self {
            device_a,
            device_b,
            app_context,
            epoch,
        }
    }

    /// Build relationship identifier for key derivation
    fn build_relationship_id(&self) -> Vec<u8> {
        let mut h = hasher();
        h.update(b"aura-sbb-relationship-v1");
        h.update(self.device_a.0.as_bytes());
        h.update(self.device_b.0.as_bytes());
        h.update(self.app_context.as_bytes());
        h.update(&self.epoch.to_le_bytes());

        let hash = h.finalize();
        hash.to_vec()
    }
}

impl RelationshipKeyManager {
    /// Create new relationship key manager
    pub fn new(device_id: DeviceId, root_key: [u8; 32]) -> Self {
        Self {
            device_id,
            root_key,
            key_cache: HashMap::new(),
            current_epoch: 0, // Would sync from time effects in real implementation
        }
    }

    /// Derive relationship key for SBB envelope encryption
    ///
    /// This is deterministic - Alice and Bob will derive the same key
    /// when given the same parameters and root key material.
    pub fn derive_relationship_key(
        &mut self,
        peer_id: DeviceId,
        app_context: &str,
    ) -> AuraResult<RelationshipKey> {
        let context = RelationshipContext::new(
            self.device_id,
            peer_id,
            app_context.to_string(),
            self.current_epoch,
        );

        // Check cache first
        if let Some(key) = self.key_cache.get(&context) {
            return Ok(*key);
        }

        // Derive new key using existing DKD infrastructure
        let relationship_id = context.build_relationship_id();
        let identity_context = IdentityKeyContext::RelationshipKeys { relationship_id };

        let spec = KeyDerivationSpec {
            identity_context,
            permission_context: None,
            key_version: 1,
        };

        let key = derive_encryption_key(&self.root_key, &spec)
            .map_err(|e| AuraError::crypto(format!("Relationship key derivation failed: {}", e)))?;

        // Cache the derived key
        self.key_cache.insert(context, key);
        Ok(key)
    }

    /// Derive relationship key for specific epoch (for key rotation)
    pub fn derive_relationship_key_for_epoch(
        &mut self,
        peer_id: DeviceId,
        app_context: &str,
        epoch: u64,
    ) -> AuraResult<RelationshipKey> {
        let context =
            RelationshipContext::new(self.device_id, peer_id, app_context.to_string(), epoch);

        // Check cache first
        if let Some(key) = self.key_cache.get(&context) {
            return Ok(*key);
        }

        let relationship_id = context.build_relationship_id();
        let identity_context = IdentityKeyContext::RelationshipKeys { relationship_id };

        let spec = KeyDerivationSpec {
            identity_context,
            permission_context: None,
            key_version: 1,
        };

        let key = derive_encryption_key(&self.root_key, &spec)
            .map_err(|e| AuraError::crypto(format!("Relationship key derivation failed: {}", e)))?;

        self.key_cache.insert(context, key);
        Ok(key)
    }

    /// Rotate to next epoch (for periodic key rotation)
    pub fn rotate_epoch(&mut self) {
        self.current_epoch += 1;
        // In real implementation, would clear expired keys from cache
    }

    /// Get current epoch
    pub fn current_epoch(&self) -> u64 {
        self.current_epoch
    }

    /// Clear key cache (for memory management)
    pub fn clear_cache(&mut self) {
        self.key_cache.clear();
    }

    /// Get cache size (for monitoring)
    pub fn cache_size(&self) -> usize {
        self.key_cache.len()
    }
}

/// Generate deterministic root key from device ID for testing
/// In production, this would come from the distributed DKD protocol
pub fn derive_test_root_key(device_id: DeviceId) -> [u8; 32] {
    use aura_core::hash::hasher;

    let mut h = hasher();
    h.update(b"aura-test-root-key-v1");
    h.update(device_id.0.as_bytes());
    h.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relationship_context_ordering() {
        let alice_id = DeviceId::new();
        let bob_id = DeviceId::new();

        // Context should be same regardless of parameter order
        let context1 = RelationshipContext::new(alice_id, bob_id, "sbb-envelope".to_string(), 0);
        let context2 = RelationshipContext::new(bob_id, alice_id, "sbb-envelope".to_string(), 0);

        assert_eq!(context1, context2);
        assert_eq!(
            context1.build_relationship_id(),
            context2.build_relationship_id()
        );
    }

    #[test]
    fn test_relationship_context_uniqueness() {
        let alice_id = DeviceId::new();
        let bob_id = DeviceId::new();
        let charlie_id = DeviceId::new();

        let context_ab = RelationshipContext::new(alice_id, bob_id, "sbb-envelope".to_string(), 0);
        let context_ac =
            RelationshipContext::new(alice_id, charlie_id, "sbb-envelope".to_string(), 0);
        let context_ab_dm =
            RelationshipContext::new(alice_id, bob_id, "direct-message".to_string(), 0);
        let context_ab_epoch1 =
            RelationshipContext::new(alice_id, bob_id, "sbb-envelope".to_string(), 1);

        // Different relationships should produce different contexts
        assert_ne!(context_ab, context_ac);
        assert_ne!(context_ab, context_ab_dm);
        assert_ne!(context_ab, context_ab_epoch1);

        // Different contexts should produce different IDs
        assert_ne!(
            context_ab.build_relationship_id(),
            context_ac.build_relationship_id()
        );
        assert_ne!(
            context_ab.build_relationship_id(),
            context_ab_dm.build_relationship_id()
        );
        assert_ne!(
            context_ab.build_relationship_id(),
            context_ab_epoch1.build_relationship_id()
        );
    }

    #[test]
    fn test_key_derivation_deterministic() {
        let alice_id = DeviceId::new();
        let bob_id = DeviceId::new();

        // Alice's perspective
        let alice_root = derive_test_root_key(alice_id);
        let mut alice_manager = RelationshipKeyManager::new(alice_id, alice_root);

        // Bob's perspective
        let bob_root = derive_test_root_key(bob_id);
        let mut bob_manager = RelationshipKeyManager::new(bob_id, bob_root);

        // If Alice and Bob have same root key, they should derive same relationship key
        let shared_root = [0x42; 32]; // Simulating shared DKD output
        alice_manager.root_key = shared_root;
        bob_manager.root_key = shared_root;

        let alice_key = alice_manager
            .derive_relationship_key(bob_id, "sbb-envelope")
            .unwrap();
        let bob_key = bob_manager
            .derive_relationship_key(alice_id, "sbb-envelope")
            .unwrap();

        assert_eq!(
            alice_key, bob_key,
            "Alice and Bob should derive identical relationship keys"
        );
    }

    #[test]
    fn test_key_rotation() {
        let alice_id = DeviceId::new();
        let bob_id = DeviceId::new();
        let root_key = [0x42; 32];

        let mut manager = RelationshipKeyManager::new(alice_id, root_key);

        let key_epoch0 = manager
            .derive_relationship_key(bob_id, "sbb-envelope")
            .unwrap();

        manager.rotate_epoch();
        let key_epoch1 = manager
            .derive_relationship_key(bob_id, "sbb-envelope")
            .unwrap();

        assert_ne!(key_epoch0, key_epoch1, "Keys should differ across epochs");
        assert_eq!(manager.current_epoch(), 1);
    }

    #[test]
    fn test_key_caching() {
        let alice_id = DeviceId::new();
        let bob_id = DeviceId::new();
        let root_key = [0x42; 32];

        let mut manager = RelationshipKeyManager::new(alice_id, root_key);

        assert_eq!(manager.cache_size(), 0);

        let _key1 = manager
            .derive_relationship_key(bob_id, "sbb-envelope")
            .unwrap();
        assert_eq!(manager.cache_size(), 1);

        let _key2 = manager
            .derive_relationship_key(bob_id, "sbb-envelope")
            .unwrap(); // From cache
        assert_eq!(manager.cache_size(), 1);

        let _key3 = manager
            .derive_relationship_key(bob_id, "direct-message")
            .unwrap(); // New context
        assert_eq!(manager.cache_size(), 2);

        manager.clear_cache();
        assert_eq!(manager.cache_size(), 0);
    }

    #[test]
    fn test_app_context_isolation() {
        let alice_id = DeviceId::new();
        let bob_id = DeviceId::new();
        let root_key = [0x42; 32];

        let mut manager = RelationshipKeyManager::new(alice_id, root_key);

        let sbb_key = manager
            .derive_relationship_key(bob_id, "sbb-envelope")
            .unwrap();
        let dm_key = manager
            .derive_relationship_key(bob_id, "direct-message")
            .unwrap();

        assert_ne!(
            sbb_key, dm_key,
            "Different app contexts should produce different keys"
        );
    }
}
