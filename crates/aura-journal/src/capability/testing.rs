//! Testing utilities for capability management
//!
//! This module provides simplified mock implementations for testing Aura's
//! capability system integration with Keyhive without requiring the full
//! complexity of the real Keyhive protocol.

use aura_types::DeviceId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Simple in-memory group membership provider for testing
#[derive(Debug, Clone, Default)]
pub struct MockGroupProvider {
    /// Group membership: group_id -> set of device_ids
    memberships: BTreeMap<String, Vec<DeviceId>>,
}

impl MockGroupProvider {
    /// Create a new mock group provider
    pub fn new() -> Self {
        Self::default()
    }

    /// Add device to group
    pub fn add_member(&mut self, group_id: String, device_id: DeviceId) {
        self.memberships
            .entry(group_id)
            .or_default()
            .push(device_id);
    }

    /// Remove device from group
    pub fn remove_member(&mut self, group_id: &str, device_id: &DeviceId) {
        if let Some(members) = self.memberships.get_mut(group_id) {
            members.retain(|id| id != device_id);
        }
    }

    /// Get all groups
    pub fn groups(&self) -> Vec<String> {
        self.memberships.keys().cloned().collect()
    }
}

impl super::GroupMembershipProvider for MockGroupProvider {
    fn is_group_member(&self, device_id: &DeviceId, group_id: &str) -> bool {
        self.memberships
            .get(group_id)
            .map(|members| members.contains(device_id))
            .unwrap_or(false)
    }

    fn get_device_groups(&self, device_id: &DeviceId) -> Vec<String> {
        self.memberships
            .iter()
            .filter_map(|(group_id, members)| {
                if members.contains(device_id) {
                    Some(group_id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn get_group_members(&self, group_id: &str) -> Vec<DeviceId> {
        self.memberships.get(group_id).cloned().unwrap_or_default()
    }
}

/// Simplified mock capability for testing
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MockCapability {
    /// Unique identifier for this capability
    pub capability_id: String,
    /// Subject being granted the capability
    pub subject_id: String,
    /// Scope of the capability (e.g., "storage/read", "group/member")
    pub scope: String,
    /// Parent capability this is delegated from (None for root)
    pub parent_id: Option<String>,
    /// Expiration timestamp (None for no expiration)
    pub expiry: Option<u64>,
    /// Timestamp when this capability was created
    pub created_at: u64,
    /// Device that created this capability
    pub author_device_id: String,
}

impl MockCapability {
    /// Create a new mock capability
    pub fn new(subject_id: String, scope: String, author_device_id: String) -> Self {
        let capability_id = Self::generate_capability_id(&None, &subject_id, &scope);

        Self {
            capability_id,
            subject_id,
            scope,
            parent_id: None,
            expiry: None,
            created_at: 0, // In real implementation, use current timestamp
            author_device_id,
        }
    }

    /// Create a delegated capability
    pub fn delegate(
        parent_id: String,
        subject_id: String,
        scope: String,
        author_device_id: String,
    ) -> Self {
        let capability_id =
            Self::generate_capability_id(&Some(parent_id.clone()), &subject_id, &scope);

        Self {
            capability_id,
            subject_id,
            scope,
            parent_id: Some(parent_id),
            expiry: None,
            created_at: 0,
            author_device_id,
        }
    }

    /// Generate deterministic capability ID
    fn generate_capability_id(parent_id: &Option<String>, subject_id: &str, scope: &str) -> String {
        let mut hasher = aura_crypto::blake3_hasher();
        if let Some(parent) = parent_id {
            hasher.update(parent.as_bytes());
        }
        hasher.update(subject_id.as_bytes());
        hasher.update(scope.as_bytes());
        hex::encode(hasher.finalize().as_bytes())
    }

    /// Check if this capability is valid (basic validation)
    pub fn is_valid(&self) -> bool {
        !self.subject_id.is_empty() && !self.scope.is_empty()
    }

    /// Check if this capability has expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        self.expiry.map(|exp| current_time > exp).unwrap_or(false)
    }
}

/// Mock BeeKEM group state for testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockGroupState {
    /// Group identifier
    pub group_id: String,
    /// Current epoch
    pub epoch: u64,
    /// Current group members
    pub members: Vec<String>,
    /// Last operation timestamp
    pub last_updated: u64,
}

impl MockGroupState {
    /// Create a new mock group state
    pub fn new(group_id: String, initial_members: Vec<String>) -> Self {
        Self {
            group_id,
            epoch: 0,
            members: initial_members,
            last_updated: 0,
        }
    }

    /// Check if a member is in the group
    pub fn has_member(&self, member_id: &str) -> bool {
        self.members.contains(&member_id.to_string())
    }

    /// Get group size
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Add member to group (for testing)
    pub fn add_member(&mut self, member_id: String) {
        if !self.members.contains(&member_id) {
            self.members.push(member_id);
            self.epoch += 1;
        }
    }

    /// Remove member from group (for testing)
    pub fn remove_member(&mut self, member_id: &str) -> bool {
        let initial_len = self.members.len();
        self.members.retain(|m| m != member_id);
        if self.members.len() < initial_len {
            self.epoch += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::keyhive_manager::GroupMembershipProvider;

    #[test]
    fn test_mock_group_provider() {
        let mut provider = MockGroupProvider::new();
        let device1 = DeviceId::new();
        let device2 = DeviceId::new();

        // Add devices to groups
        provider.add_member("group1".to_string(), device1.clone());
        provider.add_member("group1".to_string(), device2.clone());
        provider.add_member("group2".to_string(), device1.clone());

        // Test membership
        assert!(provider.is_group_member(&device1, "group1"));
        assert!(provider.is_group_member(&device2, "group1"));
        assert!(provider.is_group_member(&device1, "group2"));
        assert!(!provider.is_group_member(&device2, "group2"));

        // Test group listing
        let device1_groups = provider.get_device_groups(&device1);
        assert_eq!(device1_groups.len(), 2);
        assert!(device1_groups.contains(&"group1".to_string()));
        assert!(device1_groups.contains(&"group2".to_string()));

        let device2_groups = provider.get_device_groups(&device2);
        assert_eq!(device2_groups.len(), 1);
        assert!(device2_groups.contains(&"group1".to_string()));
    }

    #[test]
    fn test_mock_capability() {
        let cap = MockCapability::new(
            "alice".to_string(),
            "storage/read".to_string(),
            "device1".to_string(),
        );

        assert!(cap.is_valid());
        assert!(!cap.is_expired(1000));
        assert_eq!(cap.parent_id, None);

        let delegated = MockCapability::delegate(
            cap.capability_id.clone(),
            "bob".to_string(),
            "storage/read".to_string(),
            "device2".to_string(),
        );

        assert!(delegated.is_valid());
        assert_eq!(delegated.parent_id, Some(cap.capability_id));
    }

    #[test]
    fn test_mock_group_state() {
        let mut group = MockGroupState::new(
            "test_group".to_string(),
            vec!["alice".to_string(), "bob".to_string()],
        );

        assert_eq!(group.member_count(), 2);
        assert!(group.has_member("alice"));
        assert!(group.has_member("bob"));
        assert!(!group.has_member("charlie"));

        group.add_member("charlie".to_string());
        assert_eq!(group.member_count(), 3);
        assert!(group.has_member("charlie"));
        assert_eq!(group.epoch, 1);

        assert!(group.remove_member("bob"));
        assert_eq!(group.member_count(), 2);
        assert!(!group.has_member("bob"));
        assert_eq!(group.epoch, 2);

        // Removing non-existent member should not change epoch
        assert!(!group.remove_member("non_existent"));
        assert_eq!(group.epoch, 2);
    }
}
