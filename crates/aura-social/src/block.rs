//! Block materialized view and service
//!
//! Provides a materialized view of a block aggregated from journal facts,
//! plus validation logic for block membership operations.

use crate::error::SocialError;
use aura_core::identifiers::AuthorityId;
use aura_journal::facts::social::{
    BlockConfigFact, BlockFact, BlockId, BlockStorageBudget, ResidentFact, StewardCapabilities,
    StewardFact,
};
use serde::{Deserialize, Serialize};

/// Materialized view of a block, aggregated from journal facts.
///
/// This is a read-only view that provides efficient queries over block state.
/// All mutations go through facts in the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Block {
    /// Unique identifier for this block
    pub block_id: BlockId,
    /// Total storage limit in bytes
    pub storage_limit: u64,
    /// Maximum number of residents
    pub max_residents: u8,
    /// Maximum number of neighborhoods this block can join
    pub neighborhood_limit: u8,
    /// Current residents (authority IDs)
    pub residents: Vec<AuthorityId>,
    /// Current stewards with their capabilities
    pub stewards: Vec<(AuthorityId, StewardCapabilities)>,
    /// Current storage budget tracking
    pub storage_budget: BlockStorageBudget,
}

impl Block {
    /// Build a Block view from journal facts.
    ///
    /// # Arguments
    /// * `block_fact` - The block existence/configuration fact
    /// * `config` - The block configuration fact (optional, uses v1 defaults if None)
    /// * `residents` - All resident facts for this block
    /// * `stewards` - All steward facts for this block
    pub fn from_facts(
        block_fact: &BlockFact,
        config: Option<&BlockConfigFact>,
        residents: &[ResidentFact],
        stewards: &[StewardFact],
    ) -> Self {
        let config = config
            .cloned()
            .unwrap_or_else(|| BlockConfigFact::v1_default(block_fact.block_id));

        // Calculate storage budget from residents
        let resident_storage_spent: u64 = residents.iter().map(|r| r.storage_allocated).sum();

        let mut storage_budget = BlockStorageBudget::new(block_fact.block_id);
        storage_budget.resident_storage_spent = resident_storage_spent;

        Self {
            block_id: block_fact.block_id,
            storage_limit: block_fact.storage_limit,
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

    /// Create a new empty block with default configuration.
    pub fn new_empty(block_id: BlockId) -> Self {
        Self {
            block_id,
            storage_limit: BlockFact::DEFAULT_STORAGE_LIMIT,
            max_residents: BlockConfigFact::V1_MAX_RESIDENTS,
            neighborhood_limit: BlockConfigFact::V1_NEIGHBORHOOD_LIMIT,
            residents: Vec::new(),
            stewards: Vec::new(),
            storage_budget: BlockStorageBudget::new(block_id),
        }
    }

    /// Check if this block can accept another resident.
    pub fn can_add_resident(&self) -> bool {
        self.residents.len() < self.max_residents as usize
    }

    /// Check if an authority is a resident of this block.
    pub fn is_resident(&self, authority: &AuthorityId) -> bool {
        self.residents.contains(authority)
    }

    /// Check if an authority is a steward of this block.
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

    /// Validate that an authority can join this block.
    pub fn validate_join(&self, authority: &AuthorityId) -> Result<(), SocialError> {
        if self.is_resident(authority) {
            return Err(SocialError::AlreadyResident {
                block_id: self.block_id,
            });
        }

        if !self.can_add_resident() {
            return Err(SocialError::block_full(self.block_id, self.max_residents));
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

    /// Get all block peers (residents excluding self).
    pub fn block_peers(&self, self_authority: &AuthorityId) -> Vec<AuthorityId> {
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
    fn test_block_from_facts() {
        let block_id = BlockId::from_bytes([1u8; 32]);
        let block_fact = BlockFact::new(block_id, test_timestamp());

        let residents = vec![
            ResidentFact::new(test_authority(1), block_id, test_timestamp()),
            ResidentFact::new(test_authority(2), block_id, test_timestamp()),
        ];

        let stewards = vec![StewardFact::new(
            test_authority(1),
            block_id,
            test_timestamp(),
        )];

        let block = Block::from_facts(&block_fact, None, &residents, &stewards);

        assert_eq!(block.block_id, block_id);
        assert_eq!(block.resident_count(), 2);
        assert!(block.is_resident(&test_authority(1)));
        assert!(block.is_resident(&test_authority(2)));
        assert!(!block.is_resident(&test_authority(3)));
        assert!(block.is_steward(&test_authority(1)));
        assert!(!block.is_steward(&test_authority(2)));
    }

    #[test]
    fn test_block_capacity() {
        let block_id = BlockId::from_bytes([2u8; 32]);
        let mut block = Block::new_empty(block_id);

        // Can add residents up to max
        for i in 0..8u8 {
            assert!(block.can_add_resident());
            block.residents.push(test_authority(i));
        }

        // Now full
        assert!(!block.can_add_resident());
        assert_eq!(block.available_slots(), 0);
    }

    #[test]
    fn test_validate_join() {
        let block_id = BlockId::from_bytes([3u8; 32]);
        let mut block = Block::new_empty(block_id);
        let authority = test_authority(1);

        // Can join empty block
        assert!(block.validate_join(&authority).is_ok());

        // Can't join twice
        block.residents.push(authority);
        let result = block.validate_join(&authority);
        assert!(matches!(result, Err(SocialError::AlreadyResident { .. })));

        // Fill block
        for i in 2..9u8 {
            block.residents.push(test_authority(i));
        }

        // Can't join full block
        let new_authority = test_authority(10);
        let result = block.validate_join(&new_authority);
        assert!(matches!(result, Err(SocialError::BlockFull { .. })));
    }

    #[test]
    fn test_block_peers() {
        let block_id = BlockId::from_bytes([4u8; 32]);
        let mut block = Block::new_empty(block_id);

        let self_auth = test_authority(1);
        let peer1 = test_authority(2);
        let peer2 = test_authority(3);

        block.residents = vec![self_auth, peer1, peer2];

        let peers = block.block_peers(&self_auth);
        assert_eq!(peers.len(), 2);
        assert!(peers.contains(&peer1));
        assert!(peers.contains(&peer2));
        assert!(!peers.contains(&self_auth));
    }
}
