//! Home materialized view and service
//!
//! Provides a materialized view of a home aggregated from journal facts,
//! plus validation logic for home membership operations.

use crate::error::SocialError;
use crate::facts::{
    HomeConfigFact, HomeFact, HomeId, HomeStorageBudget, ResidentFact, StewardCapabilities,
    StewardFact,
};
use aura_core::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

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
    /// Maximum number of residents
    pub max_residents: u8,
    /// Maximum number of neighborhoods this home can join
    pub neighborhood_limit: u8,
    /// Current residents (authority IDs)
    pub residents: Vec<AuthorityId>,
    /// Current stewards with their capabilities
    pub stewards: Vec<(AuthorityId, StewardCapabilities)>,
    /// Current storage budget tracking
    pub storage_budget: HomeStorageBudget,
}

impl Home {
    /// Build a Home view from journal facts.
    ///
    /// # Arguments
    /// * `home_fact` - The home existence/configuration fact
    /// * `config` - The home configuration fact (optional, uses v1 defaults if None)
    /// * `residents` - All resident facts for this home
    /// * `stewards` - All steward facts for this home
    pub fn from_facts(
        home_fact: &HomeFact,
        config: Option<&HomeConfigFact>,
        residents: &[ResidentFact],
        stewards: &[StewardFact],
    ) -> Self {
        let config = config
            .cloned()
            .unwrap_or_else(|| HomeConfigFact::v1_default(home_fact.home_id));

        // Calculate storage budget from residents
        let resident_storage_spent: u64 = residents.iter().map(|r| r.storage_allocated).sum();

        let mut storage_budget = HomeStorageBudget::new(home_fact.home_id);
        storage_budget.resident_storage_spent = resident_storage_spent;

        Self {
            home_id: home_fact.home_id,
            storage_limit: home_fact.storage_limit,
            max_residents: config.max_residents,
            neighborhood_limit: config.neighborhood_limit,
            residents: residents.iter().map(|r| r.authority_id).collect(),
            stewards: stewards
                .iter()
                .map(|s| (s.authority_id, s.capabilities.clone()))
                .collect(),
            storage_budget,
        }
    }

    /// Create a new empty home with default configuration.
    pub fn new_empty(home_id: HomeId) -> Self {
        Self {
            home_id,
            storage_limit: HomeFact::DEFAULT_STORAGE_LIMIT,
            max_residents: HomeConfigFact::V1_MAX_RESIDENTS,
            neighborhood_limit: HomeConfigFact::V1_NEIGHBORHOOD_LIMIT,
            residents: Vec::new(),
            stewards: Vec::new(),
            storage_budget: HomeStorageBudget::new(home_id),
        }
    }

    /// Check if this home can accept another resident.
    pub fn can_add_resident(&self) -> bool {
        self.residents.len() < self.max_residents as usize
    }

    /// Check if an authority is a resident of this home.
    pub fn is_resident(&self, authority: &AuthorityId) -> bool {
        self.residents.contains(authority)
    }

    /// Check if an authority is a steward of this home.
    pub fn is_steward(&self, authority: &AuthorityId) -> bool {
        self.stewards.iter().any(|(a, _)| a == authority)
    }

    /// Get steward capabilities for an authority, if they are a steward.
    pub fn steward_capabilities(&self, authority: &AuthorityId) -> Option<&StewardCapabilities> {
        self.stewards
            .iter()
            .find(|(a, _)| a == authority)
            .map(|(_, caps)| caps)
    }

    /// Get the number of current residents.
    pub fn resident_count(&self) -> usize {
        self.residents.len()
    }

    /// Get available resident slots.
    pub fn available_slots(&self) -> usize {
        (self.max_residents as usize).saturating_sub(self.residents.len())
    }

    /// Validate that an authority can join this home.
    pub fn validate_join(&self, authority: &AuthorityId) -> Result<(), SocialError> {
        if self.is_resident(authority) {
            return Err(SocialError::AlreadyResident {
                home_id: self.home_id,
            });
        }

        if !self.can_add_resident() {
            return Err(SocialError::home_full(self.home_id, self.max_residents));
        }

        Ok(())
    }

    /// Validate storage allocation request.
    pub fn validate_storage(&self, requested: u64) -> Result<(), SocialError> {
        let available = self.storage_budget.remaining_public_good_space();
        if requested > available {
            return Err(SocialError::storage_exceeded(available, requested));
        }
        Ok(())
    }

    /// Get all home peers (residents excluding self).
    pub fn home_peers(&self, self_authority: &AuthorityId) -> Vec<AuthorityId> {
        self.residents
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

        let residents = vec![
            ResidentFact::new(test_authority(1), home_id, test_timestamp()),
            ResidentFact::new(test_authority(2), home_id, test_timestamp()),
        ];

        let stewards = vec![StewardFact::new(
            test_authority(1),
            home_id,
            test_timestamp(),
        )];

        let home_instance = Home::from_facts(&home_fact, None, &residents, &stewards);

        assert_eq!(home_instance.home_id, home_id);
        assert_eq!(home_instance.resident_count(), 2);
        assert!(home_instance.is_resident(&test_authority(1)));
        assert!(home_instance.is_resident(&test_authority(2)));
        assert!(!home_instance.is_resident(&test_authority(3)));
        assert!(home_instance.is_steward(&test_authority(1)));
        assert!(!home_instance.is_steward(&test_authority(2)));
    }

    #[test]
    fn test_home_capacity() {
        let home_id = HomeId::from_bytes([2u8; 32]);
        let mut home_instance = Home::new_empty(home_id);

        // Can add residents up to max
        for i in 0..8u8 {
            assert!(home_instance.can_add_resident());
            home_instance.residents.push(test_authority(i));
        }

        // Now full
        assert!(!home_instance.can_add_resident());
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
        home_instance.residents.push(authority);
        let result = home_instance.validate_join(&authority);
        assert!(matches!(result, Err(SocialError::AlreadyResident { .. })));

        // Fill home
        for i in 2..9u8 {
            home_instance.residents.push(test_authority(i));
        }

        // Can't join full home
        let new_authority = test_authority(10);
        let result = home_instance.validate_join(&new_authority);
        assert!(matches!(result, Err(SocialError::HomeFull { .. })));
    }

    #[test]
    fn test_home_peers() {
        let home_id = HomeId::from_bytes([4u8; 32]);
        let mut home_instance = Home::new_empty(home_id);

        let self_auth = test_authority(1);
        let peer1 = test_authority(2);
        let peer2 = test_authority(3);

        home_instance.residents = vec![self_auth, peer1, peer2];

        let peers = home_instance.home_peers(&self_auth);
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));
        assert!(!peers.contains(&self_auth));
    }
}
