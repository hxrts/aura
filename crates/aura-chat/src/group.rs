//! Chat Group data structures and management
//!
//! This module defines the ChatGroup struct and related functionality
//! for managing group membership, metadata, and permissions.

use crate::{
    types::{ChatMember, ChatRole},
    ChatGroupId,
};
use aura_core::identifiers::AuthorityId;
use aura_core::time::TimeStamp;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A chat group with members and metadata
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatGroup {
    /// Unique identifier for the group
    pub id: ChatGroupId,
    /// Human-readable group name
    pub name: String,
    /// Optional group description
    pub description: String,
    /// When the group was created (using unified time system)
    pub created_at: TimeStamp,
    /// Authority that created the group
    pub created_by: AuthorityId,
    /// List of group members
    pub members: Vec<ChatMember>,
    /// Additional group metadata
    pub metadata: HashMap<String, String>,
}

impl ChatGroup {
    /// Check if an authority is a member of this group
    pub fn is_member(&self, authority_id: &AuthorityId) -> bool {
        self.members.iter().any(|m| &m.authority_id == authority_id)
    }

    /// Get a member by authority ID
    pub fn get_member(&self, authority_id: &AuthorityId) -> Option<&ChatMember> {
        self.members
            .iter()
            .find(|m| &m.authority_id == authority_id)
    }

    /// Check if an authority has admin role
    pub fn is_admin(&self, authority_id: &AuthorityId) -> bool {
        self.members
            .iter()
            .any(|m| &m.authority_id == authority_id && matches!(m.role, ChatRole::Admin))
    }

    /// Get all admin members
    pub fn get_admins(&self) -> Vec<&ChatMember> {
        self.members
            .iter()
            .filter(|m| matches!(m.role, ChatRole::Admin))
            .collect()
    }

    /// Get member count
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Update group metadata
    pub fn set_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    /// Remove metadata key
    pub fn remove_metadata(&mut self, key: &str) {
        self.metadata.remove(key);
    }

    /// Get metadata value
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;
    use uuid::Uuid;

    fn create_test_group() -> ChatGroup {
        let group_id = ChatGroupId::from_uuid(Uuid::new_v4());
        let creator_id = AuthorityId::new();
        // Use deterministic time for tests instead of system time
        let now = TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        });

        ChatGroup {
            id: group_id,
            name: "Test Group".to_string(),
            description: "A test group".to_string(),
            created_at: now.clone(),
            created_by: creator_id.clone(),
            members: vec![ChatMember {
                authority_id: creator_id,
                display_name: "Creator".to_string(),
                joined_at: now,
                role: ChatRole::Admin,
            }],
            metadata: HashMap::new(),
        }
    }

    #[test]
    fn test_group_membership() {
        let group = create_test_group();
        let creator_id = &group.created_by;
        let non_member_id = AuthorityId::new();

        assert!(group.is_member(creator_id));
        assert!(!group.is_member(&non_member_id));
    }

    #[test]
    fn test_admin_permissions() {
        let group = create_test_group();
        let creator_id = &group.created_by;
        let non_member_id = AuthorityId::new();

        assert!(group.is_admin(creator_id));
        assert!(!group.is_admin(&non_member_id));
    }

    #[test]
    fn test_metadata_operations() {
        let mut group = create_test_group();

        // Add metadata
        group.set_metadata("topic".to_string(), "General Discussion".to_string());
        assert_eq!(
            group.get_metadata("topic"),
            Some(&"General Discussion".to_string())
        );

        // Remove metadata
        group.remove_metadata("topic");
        assert_eq!(group.get_metadata("topic"), None);
    }
}
