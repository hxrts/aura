//! Traversal service
//!
//! Provides traversal rules and capability checks for moving through
//! the social topology.

use crate::{error::SocialError, Block, Neighborhood};
use crate::facts::{
    BlockId, TraversalAllowedFact, TraversalDepth, TraversalPosition,
};
use std::collections::HashMap;

/// Service for checking traversal permissions.
pub struct TraversalService {
    /// Traversal rules indexed by (from_block, to_block)
    rules: HashMap<(BlockId, BlockId), TraversalAllowedFact>,
}

impl TraversalService {
    /// Create a new traversal service from traversal facts.
    pub fn from_facts(facts: &[TraversalAllowedFact]) -> Self {
        let rules = facts
            .iter()
            .map(|f| ((f.from_block, f.to_block), f.clone()))
            .collect();
        Self { rules }
    }

    /// Create an empty traversal service.
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
        }
    }

    /// Check if traversal is allowed from one block to another.
    ///
    /// # Arguments
    /// * `from` - The current traversal position
    /// * `to_block` - The target block
    /// * `capabilities` - The capabilities the traversing authority has
    pub fn can_traverse(
        &self,
        from: &TraversalPosition,
        to_block: BlockId,
        capabilities: &[String],
    ) -> bool {
        let from_block = match from.block {
            Some(b) => b,
            None => return false, // Can't traverse from nowhere
        };

        // Check if there's a rule for this traversal
        if let Some(rule) = self.rules.get(&(from_block, to_block)) {
            // Check if the required capability is present
            capabilities.contains(&rule.capability_requirement)
        } else {
            // No explicit rule means traversal is not allowed
            false
        }
    }

    /// Get the required capability for traversal between two blocks.
    pub fn required_capability(&self, from: BlockId, to: BlockId) -> Option<&str> {
        self.rules
            .get(&(from, to))
            .map(|r| r.capability_requirement.as_str())
    }

    /// Add a traversal rule.
    pub fn add_rule(&mut self, fact: TraversalAllowedFact) {
        self.rules.insert((fact.from_block, fact.to_block), fact);
    }

    /// Validate a traversal request.
    pub fn validate_traversal(
        &self,
        from: &TraversalPosition,
        to_block: BlockId,
        capabilities: &[String],
    ) -> Result<(), SocialError> {
        if !self.can_traverse(from, to_block, capabilities) {
            let reason = match from.block {
                Some(from_block) => {
                    if let Some(required) = self.required_capability(from_block, to_block) {
                        format!("missing capability: {}", required)
                    } else {
                        "no traversal path exists".to_string()
                    }
                }
                None => "no current position".to_string(),
            };
            return Err(SocialError::traversal_denied(reason));
        }
        Ok(())
    }
}

impl Default for TraversalService {
    fn default() -> Self {
        Self::new()
    }
}

/// Determines the traversal depth for an authority visiting a block.
pub fn determine_depth(
    block: &Block,
    authority_block: Option<BlockId>,
    neighborhoods: &[Neighborhood],
) -> TraversalDepth {
    // If authority is a resident, they have interior access
    if Some(block.block_id) == authority_block {
        return TraversalDepth::Interior;
    }

    // If authority's block shares a neighborhood, they have frontage access
    if let Some(auth_block) = authority_block {
        for neighborhood in neighborhoods {
            if neighborhood.is_member(block.block_id) && neighborhood.is_member(auth_block) {
                return TraversalDepth::Frontage;
            }
        }
    }

    // Otherwise, only street-level access
    TraversalDepth::Street
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{
        identifiers::ContextId,
        time::{PhysicalTime, TimeStamp},
    };

    fn test_timestamp() -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1700000000000,
            uncertainty: None,
        })
    }

    fn test_position(block: BlockId) -> TraversalPosition {
        TraversalPosition {
            neighborhood: None,
            block: Some(block),
            depth: TraversalDepth::Interior,
            context_id: ContextId::new_from_entropy([0u8; 32]),
            entered_at: test_timestamp(),
        }
    }

    #[test]
    fn test_traversal_with_capability() {
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);

        let rule = TraversalAllowedFact::new(block_a, block_b, "neighbor_access");
        let service = TraversalService::from_facts(&[rule]);

        let position = test_position(block_a);
        let caps = vec!["neighbor_access".to_string()];

        assert!(service.can_traverse(&position, block_b, &caps));
        assert!(service
            .validate_traversal(&position, block_b, &caps)
            .is_ok());
    }

    #[test]
    fn test_traversal_without_capability() {
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);

        let rule = TraversalAllowedFact::new(block_a, block_b, "neighbor_access");
        let service = TraversalService::from_facts(&[rule]);

        let position = test_position(block_a);
        let caps: Vec<String> = vec![];

        assert!(!service.can_traverse(&position, block_b, &caps));
        assert!(service
            .validate_traversal(&position, block_b, &caps)
            .is_err());
    }

    #[test]
    fn test_no_traversal_rule() {
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);

        let service = TraversalService::new();
        let position = test_position(block_a);

        assert!(!service.can_traverse(&position, block_b, &["any_cap".to_string()]));
    }

    #[test]
    fn test_determine_depth_resident() {
        let block_id = BlockId::from_bytes([1u8; 32]);
        let block = Block::new_empty(block_id);

        let depth = determine_depth(&block, Some(block_id), &[]);
        assert_eq!(depth, TraversalDepth::Interior);
    }

    #[test]
    fn test_determine_depth_neighbor() {
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);
        let neighborhood_id = crate::facts::NeighborhoodId::from_bytes([1u8; 32]);

        let block = Block::new_empty(block_a);
        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_blocks = vec![block_a, block_b];

        let depth = determine_depth(&block, Some(block_b), &[neighborhood]);
        assert_eq!(depth, TraversalDepth::Frontage);
    }

    #[test]
    fn test_determine_depth_stranger() {
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);

        let block = Block::new_empty(block_a);

        let depth = determine_depth(&block, Some(block_b), &[]);
        assert_eq!(depth, TraversalDepth::Street);
    }
}
