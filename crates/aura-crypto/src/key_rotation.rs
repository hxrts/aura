//! Coordinated Key Rotation
//!
//! Implements independent key rotation for identity and permission keys,
//! enabling rotation of relationship keys without affecting storage keys
//! and vice versa. Supports coordinated revocation that rotates all keys atomically.
//!
//! Reference: docs/040_storage.md Section 2.1 "KeyDerivationSpec"

use crate::key_derivation::{IdentityKeyContext, KeyDerivationSpec, PermissionKeyContext};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Key rotation event types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum KeyRotationEvent {
    /// Rotate relationship keys (K_box, K_tag, K_psk)
    RelationshipKeyRotation {
        /// Relationship identifier
        relationship_id: Vec<u8>,
        /// Previous key version
        old_version: u32,
        /// New key version
        new_version: u32,
        /// Rotation timestamp
        timestamp: u64,
    },
    /// Rotate storage access keys
    StorageKeyRotation {
        /// Device identifier
        device_id: Vec<u8>,
        /// Previous key version
        old_version: u32,
        /// New key version
        new_version: u32,
        /// Rotation timestamp
        timestamp: u64,
    },
    /// Rotate communication keys
    CommunicationKeyRotation {
        /// Relationship identifier
        relationship_id: Vec<u8>,
        /// Previous key version
        old_version: u32,
        /// New key version
        new_version: u32,
        /// Rotation timestamp
        timestamp: u64,
    },
    /// Coordinated revocation rotates all keys
    CoordinatedRevocation {
        /// Device identifier
        device_id: Vec<u8>,
        /// New version for each relationship
        relationship_versions: BTreeMap<Vec<u8>, u32>,
        /// New storage key version
        storage_version: u32,
        /// New version for each communication relationship
        communication_versions: BTreeMap<Vec<u8>, u32>,
        /// Revocation timestamp
        timestamp: u64,
    },
}

/// Key version tracker per subsystem
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyVersionTracker {
    /// Current versions for relationship keys (relationship_id -> version)
    relationship_versions: BTreeMap<Vec<u8>, u32>,

    /// Current version for storage keys
    storage_version: u32,

    /// Current versions for communication keys (relationship_id -> version)
    communication_versions: BTreeMap<Vec<u8>, u32>,

    /// History of rotation events
    rotation_history: Vec<KeyRotationEvent>,
}

impl KeyVersionTracker {
    /// Create a new key version tracker
    pub fn new() -> Self {
        Self {
            relationship_versions: BTreeMap::new(),
            storage_version: 0,
            communication_versions: BTreeMap::new(),
            rotation_history: Vec::new(),
        }
    }

    /// Get current relationship key version
    pub fn get_relationship_version(&self, relationship_id: &[u8]) -> u32 {
        self.relationship_versions
            .get(relationship_id)
            .copied()
            .unwrap_or(0)
    }

    /// Get current storage key version
    pub fn get_storage_version(&self) -> u32 {
        self.storage_version
    }

    /// Get current communication key version
    pub fn get_communication_version(&self, relationship_id: &[u8]) -> u32 {
        self.communication_versions
            .get(relationship_id)
            .copied()
            .unwrap_or(0)
    }

    /// Rotate relationship keys
    pub fn rotate_relationship_keys(
        &mut self,
        relationship_id: Vec<u8>,
        timestamp: u64,
    ) -> KeyRotationEvent {
        let old_version = self.get_relationship_version(&relationship_id);
        let new_version = old_version + 1;

        self.relationship_versions
            .insert(relationship_id.clone(), new_version);

        let event = KeyRotationEvent::RelationshipKeyRotation {
            relationship_id,
            old_version,
            new_version,
            timestamp,
        };

        self.rotation_history.push(event.clone());
        event
    }

    /// Rotate storage keys
    pub fn rotate_storage_keys(&mut self, device_id: Vec<u8>, timestamp: u64) -> KeyRotationEvent {
        let old_version = self.storage_version;
        let new_version = old_version + 1;

        self.storage_version = new_version;

        let event = KeyRotationEvent::StorageKeyRotation {
            device_id,
            old_version,
            new_version,
            timestamp,
        };

        self.rotation_history.push(event.clone());
        event
    }

    /// Rotate communication keys
    pub fn rotate_communication_keys(
        &mut self,
        relationship_id: Vec<u8>,
        timestamp: u64,
    ) -> KeyRotationEvent {
        let old_version = self.get_communication_version(&relationship_id);
        let new_version = old_version + 1;

        self.communication_versions
            .insert(relationship_id.clone(), new_version);

        let event = KeyRotationEvent::CommunicationKeyRotation {
            relationship_id,
            old_version,
            new_version,
            timestamp,
        };

        self.rotation_history.push(event.clone());
        event
    }

    /// Coordinated revocation - rotate all keys atomically
    pub fn coordinated_revocation(
        &mut self,
        device_id: Vec<u8>,
        timestamp: u64,
    ) -> KeyRotationEvent {
        // Collect relationship IDs first to avoid borrowing issues
        let rel_ids: Vec<Vec<u8>> = self.relationship_versions.keys().cloned().collect();

        // Rotate all relationship keys
        let mut relationship_versions = BTreeMap::new();
        for id in rel_ids {
            let old_version = *self.relationship_versions.get(&id).unwrap_or(&0);
            let new_version = old_version + 1;
            self.relationship_versions.insert(id.clone(), new_version);
            relationship_versions.insert(id, new_version);
        }

        // Rotate storage keys
        let old_storage_version = self.storage_version;
        let new_storage_version = old_storage_version + 1;
        self.storage_version = new_storage_version;

        // Collect communication IDs first to avoid borrowing issues
        let comm_ids: Vec<Vec<u8>> = self.communication_versions.keys().cloned().collect();

        // Rotate all communication keys
        let mut communication_versions = BTreeMap::new();
        for id in comm_ids {
            let old_version = *self.communication_versions.get(&id).unwrap_or(&0);
            let new_version = old_version + 1;
            self.communication_versions.insert(id.clone(), new_version);
            communication_versions.insert(id, new_version);
        }

        let event = KeyRotationEvent::CoordinatedRevocation {
            device_id,
            relationship_versions,
            storage_version: new_storage_version,
            communication_versions,
            timestamp,
        };

        self.rotation_history.push(event.clone());
        event
    }

    /// Get rotation history
    pub fn get_history(&self) -> &[KeyRotationEvent] {
        &self.rotation_history
    }

    /// Check if a key version is current
    pub fn is_current_relationship_version(&self, relationship_id: &[u8], version: u32) -> bool {
        self.get_relationship_version(relationship_id) == version
    }

    /// Check if storage version is current
    pub fn is_current_storage_version(&self, version: u32) -> bool {
        self.storage_version == version
    }

    /// Check if communication version is current
    pub fn is_current_communication_version(&self, relationship_id: &[u8], version: u32) -> bool {
        self.get_communication_version(relationship_id) == version
    }
}

impl Default for KeyVersionTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Key rotation coordinator
#[derive(Debug, Clone)]
pub struct KeyRotationCoordinator {
    /// Version tracker
    tracker: KeyVersionTracker,
}

impl KeyRotationCoordinator {
    /// Create a new coordinator
    pub fn new() -> Self {
        Self {
            tracker: KeyVersionTracker::new(),
        }
    }

    /// Create a new coordinator with existing tracker
    pub fn with_tracker(tracker: KeyVersionTracker) -> Self {
        Self { tracker }
    }

    /// Get the version tracker
    pub fn tracker(&self) -> &KeyVersionTracker {
        &self.tracker
    }

    /// Get mutable version tracker
    pub fn tracker_mut(&mut self) -> &mut KeyVersionTracker {
        &mut self.tracker
    }

    /// Rotate relationship keys and return new key specs
    pub fn rotate_relationship_keys(
        &mut self,
        relationship_id: Vec<u8>,
        timestamp: u64,
    ) -> (KeyRotationEvent, KeyDerivationSpec) {
        let event = self
            .tracker
            .rotate_relationship_keys(relationship_id.clone(), timestamp);

        let new_version = self.tracker.get_relationship_version(&relationship_id);

        let key_spec = KeyDerivationSpec::identity_only(IdentityKeyContext::RelationshipKeys {
            relationship_id,
        })
        .with_version(new_version);

        (event, key_spec)
    }

    /// Rotate storage keys and return new key spec
    pub fn rotate_storage_keys(
        &mut self,
        device_id: Vec<u8>,
        timestamp: u64,
    ) -> (KeyRotationEvent, KeyDerivationSpec) {
        let event = self
            .tracker
            .rotate_storage_keys(device_id.clone(), timestamp);

        let new_version = self.tracker.get_storage_version();

        let key_spec =
            KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption { device_id })
                .with_version(new_version);

        (event, key_spec)
    }

    /// Rotate communication keys and return new key spec
    pub fn rotate_communication_keys(
        &mut self,
        relationship_id: Vec<u8>,
        timestamp: u64,
    ) -> (KeyRotationEvent, KeyDerivationSpec) {
        let event = self
            .tracker
            .rotate_communication_keys(relationship_id.clone(), timestamp);

        let new_version = self.tracker.get_communication_version(&relationship_id);

        let key_spec = KeyDerivationSpec::with_permission(
            IdentityKeyContext::RelationshipKeys {
                relationship_id: relationship_id.clone(),
            },
            PermissionKeyContext::CommunicationScope {
                operation: "send".to_string(),
                relationship: hex::encode(&relationship_id),
            },
        )
        .with_version(new_version);

        (event, key_spec)
    }

    /// Coordinated revocation - rotate all keys atomically
    pub fn coordinated_revocation(
        &mut self,
        device_id: Vec<u8>,
        timestamp: u64,
    ) -> (KeyRotationEvent, Vec<KeyDerivationSpec>) {
        let event = self
            .tracker
            .coordinated_revocation(device_id.clone(), timestamp);

        let mut specs = Vec::new();

        // Create new specs for all rotated keys
        if let KeyRotationEvent::CoordinatedRevocation {
            relationship_versions,
            storage_version,
            communication_versions,
            ..
        } = &event
        {
            // Storage key spec
            specs.push(
                KeyDerivationSpec::identity_only(IdentityKeyContext::DeviceEncryption {
                    device_id: device_id.clone(),
                })
                .with_version(*storage_version),
            );

            // Relationship key specs
            for (rel_id, &version) in relationship_versions {
                specs.push(
                    KeyDerivationSpec::identity_only(IdentityKeyContext::RelationshipKeys {
                        relationship_id: rel_id.clone(),
                    })
                    .with_version(version),
                );
            }

            // Communication key specs
            for (rel_id, &version) in communication_versions {
                specs.push(
                    KeyDerivationSpec::with_permission(
                        IdentityKeyContext::RelationshipKeys {
                            relationship_id: rel_id.clone(),
                        },
                        PermissionKeyContext::CommunicationScope {
                            operation: "send".to_string(),
                            relationship: hex::encode(rel_id),
                        },
                    )
                    .with_version(version),
                );
            }
        }

        (event, specs)
    }

    /// Verify that an operation uses the current key version
    pub fn verify_relationship_version(&self, relationship_id: &[u8], version: u32) -> bool {
        self.tracker
            .is_current_relationship_version(relationship_id, version)
    }

    /// Verify storage version
    pub fn verify_storage_version(&self, version: u32) -> bool {
        self.tracker.is_current_storage_version(version)
    }

    /// Verify communication version
    pub fn verify_communication_version(&self, relationship_id: &[u8], version: u32) -> bool {
        self.tracker
            .is_current_communication_version(relationship_id, version)
    }
}

impl Default for KeyRotationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(warnings, clippy::all)]
mod tests {
    use super::*;

    #[test]
    fn test_key_version_tracker_creation() {
        let tracker = KeyVersionTracker::new();
        assert_eq!(tracker.get_storage_version(), 0);
        assert_eq!(tracker.get_relationship_version(b"rel1"), 0);
        assert_eq!(tracker.get_communication_version(b"rel1"), 0);
    }

    #[test]
    fn test_rotate_relationship_keys() {
        let mut tracker = KeyVersionTracker::new();
        let rel_id = b"relationship1".to_vec();

        let event = tracker.rotate_relationship_keys(rel_id.clone(), 1000);

        match event {
            KeyRotationEvent::RelationshipKeyRotation {
                relationship_id,
                old_version,
                new_version,
                timestamp,
            } => {
                assert_eq!(relationship_id, rel_id);
                assert_eq!(old_version, 0);
                assert_eq!(new_version, 1);
                assert_eq!(timestamp, 1000);
            }
            _ => panic!("Wrong event type"),
        }

        assert_eq!(tracker.get_relationship_version(&rel_id), 1);
    }

    #[test]
    fn test_rotate_storage_keys() {
        let mut tracker = KeyVersionTracker::new();
        let device_id = b"device1".to_vec();

        let event = tracker.rotate_storage_keys(device_id.clone(), 2000);

        match event {
            KeyRotationEvent::StorageKeyRotation {
                device_id: dev_id,
                old_version,
                new_version,
                timestamp,
            } => {
                assert_eq!(dev_id, device_id);
                assert_eq!(old_version, 0);
                assert_eq!(new_version, 1);
                assert_eq!(timestamp, 2000);
            }
            _ => panic!("Wrong event type"),
        }

        assert_eq!(tracker.get_storage_version(), 1);
    }

    #[test]
    fn test_rotate_communication_keys() {
        let mut tracker = KeyVersionTracker::new();
        let rel_id = b"relationship1".to_vec();

        let event = tracker.rotate_communication_keys(rel_id.clone(), 3000);

        match event {
            KeyRotationEvent::CommunicationKeyRotation {
                relationship_id,
                old_version,
                new_version,
                timestamp,
            } => {
                assert_eq!(relationship_id, rel_id);
                assert_eq!(old_version, 0);
                assert_eq!(new_version, 1);
                assert_eq!(timestamp, 3000);
            }
            _ => panic!("Wrong event type"),
        }

        assert_eq!(tracker.get_communication_version(&rel_id), 1);
    }

    #[test]
    fn test_independent_rotation() {
        let mut tracker = KeyVersionTracker::new();
        let rel_id = b"relationship1".to_vec();
        let device_id = b"device1".to_vec();

        // Rotate relationship keys
        tracker.rotate_relationship_keys(rel_id.clone(), 1000);
        assert_eq!(tracker.get_relationship_version(&rel_id), 1);
        assert_eq!(tracker.get_storage_version(), 0);
        assert_eq!(tracker.get_communication_version(&rel_id), 0);

        // Rotate storage keys
        tracker.rotate_storage_keys(device_id.clone(), 2000);
        assert_eq!(tracker.get_relationship_version(&rel_id), 1);
        assert_eq!(tracker.get_storage_version(), 1);
        assert_eq!(tracker.get_communication_version(&rel_id), 0);

        // Rotate communication keys
        tracker.rotate_communication_keys(rel_id.clone(), 3000);
        assert_eq!(tracker.get_relationship_version(&rel_id), 1);
        assert_eq!(tracker.get_storage_version(), 1);
        assert_eq!(tracker.get_communication_version(&rel_id), 1);
    }

    #[test]
    fn test_coordinated_revocation() {
        let mut tracker = KeyVersionTracker::new();
        let device_id = b"device1".to_vec();
        let rel_id1 = b"relationship1".to_vec();
        let rel_id2 = b"relationship2".to_vec();

        // Initialize some keys
        tracker.rotate_relationship_keys(rel_id1.clone(), 1000);
        tracker.rotate_relationship_keys(rel_id2.clone(), 1000);
        tracker.rotate_communication_keys(rel_id1.clone(), 1000);

        assert_eq!(tracker.get_relationship_version(&rel_id1), 1);
        assert_eq!(tracker.get_relationship_version(&rel_id2), 1);
        assert_eq!(tracker.get_storage_version(), 0);
        assert_eq!(tracker.get_communication_version(&rel_id1), 1);

        // Coordinated revocation
        let event = tracker.coordinated_revocation(device_id.clone(), 5000);

        match event {
            KeyRotationEvent::CoordinatedRevocation {
                device_id: dev_id,
                relationship_versions,
                storage_version,
                communication_versions,
                timestamp,
            } => {
                assert_eq!(dev_id, device_id);
                assert_eq!(*relationship_versions.get(&rel_id1).unwrap(), 2);
                assert_eq!(*relationship_versions.get(&rel_id2).unwrap(), 2);
                assert_eq!(storage_version, 1);
                assert_eq!(*communication_versions.get(&rel_id1).unwrap(), 2);
                assert_eq!(timestamp, 5000);
            }
            _ => panic!("Wrong event type"),
        }

        // All versions should be incremented
        assert_eq!(tracker.get_relationship_version(&rel_id1), 2);
        assert_eq!(tracker.get_relationship_version(&rel_id2), 2);
        assert_eq!(tracker.get_storage_version(), 1);
        assert_eq!(tracker.get_communication_version(&rel_id1), 2);
    }

    #[test]
    fn test_rotation_history() {
        let mut tracker = KeyVersionTracker::new();
        let rel_id = b"relationship1".to_vec();
        let device_id = b"device1".to_vec();

        tracker.rotate_relationship_keys(rel_id.clone(), 1000);
        tracker.rotate_storage_keys(device_id.clone(), 2000);
        tracker.rotate_communication_keys(rel_id.clone(), 3000);

        let history = tracker.get_history();
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn test_version_verification() {
        let mut tracker = KeyVersionTracker::new();
        let rel_id = b"relationship1".to_vec();

        assert!(tracker.is_current_relationship_version(&rel_id, 0));
        assert!(!tracker.is_current_relationship_version(&rel_id, 1));

        tracker.rotate_relationship_keys(rel_id.clone(), 1000);

        assert!(!tracker.is_current_relationship_version(&rel_id, 0));
        assert!(tracker.is_current_relationship_version(&rel_id, 1));
    }

    #[test]
    fn test_coordinator_rotate_relationship_keys() {
        let mut coordinator = KeyRotationCoordinator::new();
        let rel_id = b"relationship1".to_vec();

        let (event, key_spec) = coordinator.rotate_relationship_keys(rel_id.clone(), 1000);

        match event {
            KeyRotationEvent::RelationshipKeyRotation { new_version, .. } => {
                assert_eq!(new_version, 1);
            }
            _ => panic!("Wrong event type"),
        }

        assert_eq!(key_spec.key_version, 1);
        match key_spec.identity_context {
            IdentityKeyContext::RelationshipKeys { relationship_id } => {
                assert_eq!(relationship_id, rel_id);
            }
            _ => panic!("Wrong identity context"),
        }
    }

    #[test]
    fn test_coordinator_rotate_storage_keys() {
        let mut coordinator = KeyRotationCoordinator::new();
        let device_id = b"device1".to_vec();

        let (event, key_spec) = coordinator.rotate_storage_keys(device_id.clone(), 2000);

        match event {
            KeyRotationEvent::StorageKeyRotation { new_version, .. } => {
                assert_eq!(new_version, 1);
            }
            _ => panic!("Wrong event type"),
        }

        assert_eq!(key_spec.key_version, 1);
        match key_spec.identity_context {
            IdentityKeyContext::DeviceEncryption { device_id: dev_id } => {
                assert_eq!(dev_id, device_id);
            }
            _ => panic!("Wrong identity context"),
        }
    }

    #[test]
    fn test_coordinator_coordinated_revocation() {
        let mut coordinator = KeyRotationCoordinator::new();
        let device_id = b"device1".to_vec();
        let rel_id = b"relationship1".to_vec();

        // Initialize a relationship
        coordinator.rotate_relationship_keys(rel_id.clone(), 1000);

        let (event, specs) = coordinator.coordinated_revocation(device_id.clone(), 5000);

        match event {
            KeyRotationEvent::CoordinatedRevocation {
                storage_version, ..
            } => {
                assert_eq!(storage_version, 1);
            }
            _ => panic!("Wrong event type"),
        }

        // Should have at least storage and relationship specs
        assert!(specs.len() >= 2);

        // All specs should have incremented versions
        for spec in &specs {
            assert!(spec.key_version >= 1);
        }
    }

    #[test]
    fn test_coordinator_version_verification() {
        let mut coordinator = KeyRotationCoordinator::new();
        let rel_id = b"relationship1".to_vec();

        assert!(coordinator.verify_relationship_version(&rel_id, 0));
        assert!(!coordinator.verify_relationship_version(&rel_id, 1));

        coordinator.rotate_relationship_keys(rel_id.clone(), 1000);

        assert!(!coordinator.verify_relationship_version(&rel_id, 0));
        assert!(coordinator.verify_relationship_version(&rel_id, 1));
    }

    #[test]
    fn test_storage_rotation_does_not_affect_relationships() {
        let mut coordinator = KeyRotationCoordinator::new();
        let device_id = b"device1".to_vec();
        let rel_id = b"relationship1".to_vec();

        // Set up initial state
        coordinator.rotate_relationship_keys(rel_id.clone(), 1000);
        let initial_rel_version = coordinator.tracker().get_relationship_version(&rel_id);

        // Rotate storage keys
        coordinator.rotate_storage_keys(device_id.clone(), 2000);

        // Relationship version should be unchanged
        let final_rel_version = coordinator.tracker().get_relationship_version(&rel_id);
        assert_eq!(initial_rel_version, final_rel_version);
    }

    #[test]
    fn test_relationship_rotation_does_not_affect_storage() {
        let mut coordinator = KeyRotationCoordinator::new();
        let device_id = b"device1".to_vec();
        let rel_id = b"relationship1".to_vec();

        // Set up initial state
        coordinator.rotate_storage_keys(device_id.clone(), 1000);
        let initial_storage_version = coordinator.tracker().get_storage_version();

        // Rotate relationship keys
        coordinator.rotate_relationship_keys(rel_id.clone(), 2000);

        // Storage version should be unchanged
        let final_storage_version = coordinator.tracker().get_storage_version();
        assert_eq!(initial_storage_version, final_storage_version);
    }
}
