//! Home materialized view and service
//!
//! Provides a materialized view of a home aggregated from journal facts,
//! plus validation logic for home membership operations.

use crate::error::SocialError;
use crate::facts::{
    HomeConfigFact, HomeFact, HomeId, HomeMemberFact, HomeStorageBudget, ModeratorCapabilities,
    ModeratorCapability, ModeratorDesignation, ModeratorFact,
};
use aura_core::types::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Materialized view of a home, aggregated from journal facts.
///
/// This is a read-only view that provides efficient queries over home state.
/// All mutations go through facts in the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Home {
    /// Unique identifier for this home
    pub home_id: HomeId,
    /// Total storage limit in bytes
    pub storage_limit: u64,
    /// Maximum number of members
    pub max_members: u8,
    /// Maximum number of neighborhoods this home can join
    pub neighborhood_limit: u8,
    /// Current members (authority IDs)
    pub members: Vec<AuthorityId>,
    /// Current moderator designations with capability bundles.
    pub moderator_designations: Vec<ModeratorDesignation>,
    /// Current storage budget tracking
    pub storage_budget: HomeStorageBudget,
}

impl Home {
    /// Build a Home view from journal facts.
    ///
    /// # Arguments
    /// * `home_fact` - The home existence/configuration fact
    /// * `config` - The home configuration fact (optional, uses v1 defaults if None)
    /// * `members` - All member facts for this home
    /// * `moderators` - All moderator facts for this home
    pub fn from_facts(
        home_fact: &HomeFact,
        config: Option<&HomeConfigFact>,
        members: &[HomeMemberFact],
        moderators: &[ModeratorFact],
    ) -> Self {
        let config = config
            .cloned()
            .unwrap_or_else(|| HomeConfigFact::v1_default(home_fact.home_id));

        // Calculate storage budget from members
        let member_storage_spent: u64 = members.iter().map(|r| r.storage_allocated).sum();

        let mut storage_budget = HomeStorageBudget::new(home_fact.home_id);
        storage_budget.member_storage_spent = member_storage_spent;
        let member_set: BTreeSet<AuthorityId> = members.iter().map(|r| r.authority_id).collect();
        let mut designation_by_authority: BTreeMap<AuthorityId, ModeratorDesignation> =
            BTreeMap::new();
        for moderator in moderators {
            // Invariant: Moderator ⊆ Member. Ignore stale/non-member grants in materialized view.
            if !member_set.contains(&moderator.authority_id) {
                continue;
            }
            let designation = ModeratorDesignation::from(moderator);
            let should_replace = designation_by_authority
                .get(&designation.authority_id)
                .map(|existing| {
                    designation.designated_at.to_index_ms().value()
                        >= existing.designated_at.to_index_ms().value()
                })
                .unwrap_or(true);
            if should_replace {
                designation_by_authority.insert(designation.authority_id, designation);
            }
        }

        Self {
            home_id: home_fact.home_id,
            storage_limit: home_fact.storage_limit,
            max_members: config.max_members,
            neighborhood_limit: config.neighborhood_limit,
            members: members.iter().map(|r| r.authority_id).collect(),
            moderator_designations: designation_by_authority.into_values().collect(),
            storage_budget,
        }
    }

    /// Create a new empty home with default configuration.
    pub fn new_empty(home_id: HomeId) -> Self {
        Self {
            home_id,
            storage_limit: HomeFact::DEFAULT_STORAGE_LIMIT,
            max_members: HomeConfigFact::V1_MAX_MEMBERS,
            neighborhood_limit: HomeConfigFact::V1_NEIGHBORHOOD_LIMIT,
            members: Vec::new(),
            moderator_designations: Vec::new(),
            storage_budget: HomeStorageBudget::new(home_id),
        }
    }

    /// Check if this home can accept another member.
    pub fn can_add_member(&self) -> bool {
        self.members.len() < self.max_members as usize
    }

    /// Check if an authority is a member of this home.
    pub fn is_member(&self, authority: &AuthorityId) -> bool {
        self.members.contains(authority)
    }

    /// Check if an authority is a moderator of this home.
    pub fn is_moderator(&self, authority: &AuthorityId) -> bool {
        self.moderator_designations
            .iter()
            .any(|designation| designation.authority_id == *authority)
    }

    /// Get moderator capabilities for an authority, if they are a moderator.
    pub fn moderator_capabilities(
        &self,
        authority: &AuthorityId,
    ) -> Option<&ModeratorCapabilities> {
        self.moderator_designations
            .iter()
            .find(|designation| designation.authority_id == *authority)
            .map(|designation| &designation.capabilities)
    }

    /// Check if an authority has a specific moderator capability.
    pub fn has_moderator_capability(
        &self,
        authority: &AuthorityId,
        capability: ModeratorCapability,
    ) -> bool {
        self.moderator_capabilities(authority)
            .map(|caps| caps.allows(capability))
            .unwrap_or(false)
    }

    /// Assign moderator designation to an existing home member.
    ///
    /// Returns `NotMember` if the target is not a member.
    pub fn assign_moderator_designation(
        &mut self,
        authority: AuthorityId,
        capabilities: ModeratorCapabilities,
        designated_at: aura_core::time::TimeStamp,
    ) -> Result<(), SocialError> {
        if !self.is_member(&authority) {
            return Err(SocialError::not_member(self.home_id));
        }

        if let Some(designation) = self
            .moderator_designations
            .iter_mut()
            .find(|designation| designation.authority_id == authority)
        {
            designation.capabilities = capabilities;
            designation.designated_at = designated_at;
            return Ok(());
        }

        self.moderator_designations.push(ModeratorDesignation {
            authority_id: authority,
            home_id: self.home_id,
            designated_at,
            capabilities,
        });
        Ok(())
    }

    /// Remove moderator designation from an authority.
    pub fn unassign_moderator_designation(
        &mut self,
        authority: &AuthorityId,
    ) -> Result<(), SocialError> {
        let Some(index) = self
            .moderator_designations
            .iter()
            .position(|designation| designation.authority_id == *authority)
        else {
            return Err(SocialError::not_moderator(self.home_id));
        };
        self.moderator_designations.remove(index);
        Ok(())
    }

    /// Get the number of current members.
    pub fn member_count(&self) -> usize {
        self.members.len()
    }

    /// Get available member slots.
    pub fn available_slots(&self) -> usize {
        (self.max_members as usize).saturating_sub(self.members.len())
    }

    /// Validate that an authority can join this home.
    pub fn validate_join(&self, authority: &AuthorityId) -> Result<(), SocialError> {
        if self.is_member(authority) {
            return Err(SocialError::AlreadyHomeMember {
                home_id: self.home_id,
            });
        }

        if !self.can_add_member() {
            return Err(SocialError::home_full(self.home_id, self.max_members));
        }

        Ok(())
    }

    /// Validate storage allocation request.
    pub fn validate_storage(&self, requested: u64) -> Result<(), SocialError> {
        let available = self.storage_budget.remaining_shared_storage();
        if requested > available {
            return Err(SocialError::storage_exceeded(available, requested));
        }
        Ok(())
    }

    /// Get all home peers (members excluding self).
    pub fn same_home_members(&self, self_authority: &AuthorityId) -> Vec<AuthorityId> {
        self.members
            .iter()
            .filter(|a| *a != self_authority)
            .copied()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::{PhysicalTime, TimeStamp};

    fn test_timestamp() -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1700000000000,
            uncertainty: None,
        })
    }

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_home_from_facts() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let home_fact = HomeFact::new(home_id, test_timestamp());

        let members = vec![
            HomeMemberFact::new(test_authority(1), home_id, test_timestamp()),
            HomeMemberFact::new(test_authority(2), home_id, test_timestamp()),
        ];

        let moderators = vec![ModeratorFact::new(
            test_authority(1),
            home_id,
            test_timestamp(),
        )];

        let home_instance = Home::from_facts(&home_fact, None, &members, &moderators);

        assert_eq!(home_instance.home_id, home_id);
        assert_eq!(home_instance.member_count(), 2);
        assert!(home_instance.is_member(&test_authority(1)));
        assert!(home_instance.is_member(&test_authority(2)));
        assert!(!home_instance.is_member(&test_authority(3)));
        assert!(home_instance.is_moderator(&test_authority(1)));
        assert!(!home_instance.is_moderator(&test_authority(2)));
    }

    #[test]
    fn test_moderator_designation_requires_member_membership() {
        let home_id = HomeId::from_bytes([9u8; 32]);
        let mut home_instance = Home::new_empty(home_id);
        let member = test_authority(1);
        let outsider = test_authority(2);

        home_instance.members.push(member);
        assert!(home_instance
            .assign_moderator_designation(
                member,
                ModeratorCapabilities::default(),
                test_timestamp()
            )
            .is_ok());
        assert!(home_instance.is_moderator(&member));

        let result = home_instance.assign_moderator_designation(
            outsider,
            ModeratorCapabilities::default(),
            test_timestamp(),
        );
        assert!(matches!(result, Err(SocialError::NotMember { .. })));
    }

    #[test]
    fn test_unassign_moderator_designation() {
        let home_id = HomeId::from_bytes([10u8; 32]);
        let mut home_instance = Home::new_empty(home_id);
        let member = test_authority(1);

        home_instance.members.push(member);
        let assigned = home_instance.assign_moderator_designation(
            member,
            ModeratorCapabilities::default(),
            test_timestamp(),
        );
        assert!(assigned.is_ok());
        assert!(home_instance.is_moderator(&member));

        let revoked = home_instance.unassign_moderator_designation(&member);
        assert!(revoked.is_ok());
        assert!(!home_instance.is_moderator(&member));
    }

    #[test]
    fn test_has_moderator_capability() {
        let home_id = HomeId::from_bytes([11u8; 32]);
        let mut home_instance = Home::new_empty(home_id);
        let member = test_authority(1);

        home_instance.members.push(member);
        let assigned = home_instance.assign_moderator_designation(
            member,
            ModeratorCapabilities::default(),
            test_timestamp(),
        );
        assert!(assigned.is_ok());

        assert!(home_instance.has_moderator_capability(&member, ModeratorCapability::Kick));
        assert!(home_instance.has_moderator_capability(&member, ModeratorCapability::Ban));
        assert!(home_instance.has_moderator_capability(&member, ModeratorCapability::Mute));
        assert!(
            !home_instance.has_moderator_capability(&member, ModeratorCapability::GrantModerator)
        );
    }

    #[test]
    fn test_home_capacity() {
        let home_id = HomeId::from_bytes([2u8; 32]);
        let mut home_instance = Home::new_empty(home_id);

        // Can add members up to max
        for i in 0..8u8 {
            assert!(home_instance.can_add_member());
            home_instance.members.push(test_authority(i));
        }

        // Now full
        assert!(!home_instance.can_add_member());
        assert_eq!(home_instance.available_slots(), 0);
    }

    #[test]
    fn test_validate_join() {
        let home_id = HomeId::from_bytes([3u8; 32]);
        let mut home_instance = Home::new_empty(home_id);
        let authority = test_authority(1);

        // Can join empty home
        assert!(home_instance.validate_join(&authority).is_ok());

        // Can't join twice
        home_instance.members.push(authority);
        let result = home_instance.validate_join(&authority);
        assert!(matches!(result, Err(SocialError::AlreadyHomeMember { .. })));

        // Fill home
        for i in 2..9u8 {
            home_instance.members.push(test_authority(i));
        }

        // Can't join full home
        let new_authority = test_authority(10);
        let result = home_instance.validate_join(&new_authority);
        assert!(matches!(result, Err(SocialError::HomeFull { .. })));
    }

    #[test]
    fn test_same_home_members() {
        let home_id = HomeId::from_bytes([4u8; 32]);
        let mut home_instance = Home::new_empty(home_id);

        let self_auth = test_authority(1);
        let peer1 = test_authority(2);
        let peer2 = test_authority(3);

        home_instance.members = vec![self_auth, peer1, peer2];

        let peers = home_instance.same_home_members(&self_auth);
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));
        assert!(!peers.contains(&self_auth));
    }
}
