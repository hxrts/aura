//! Neighborhood materialized view
//!
//! Provides a materialized view of a neighborhood aggregated from journal facts.

use crate::error::SocialError;
use crate::facts::{
    AdjacencyFact, BlockId, BlockMemberFact, NeighborhoodFact, NeighborhoodId,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Materialized view of a neighborhood, aggregated from journal facts.
///
/// This is a read-only view that provides efficient queries over neighborhood state.
/// All mutations go through facts in the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Neighborhood {
    /// Unique identifier for this neighborhood
    pub neighborhood_id: NeighborhoodId,
    /// Member blocks
    pub member_blocks: Vec<BlockId>,
    /// Adjacency relationships between blocks
    pub adjacencies: Vec<(BlockId, BlockId)>,
}

impl Neighborhood {
    /// Build a Neighborhood view from journal facts.
    ///
    /// # Arguments
    /// * `fact` - The neighborhood existence fact
    /// * `members` - All block membership facts for this neighborhood
    /// * `adjacencies` - All adjacency facts for this neighborhood
    pub fn from_facts(
        fact: &NeighborhoodFact,
        members: &[BlockMemberFact],
        adjacencies: &[AdjacencyFact],
    ) -> Self {
        Self {
            neighborhood_id: fact.neighborhood_id,
            member_blocks: members.iter().map(|m| m.block_id).collect(),
            adjacencies: adjacencies.iter().map(|a| (a.block_a, a.block_b)).collect(),
        }
    }

    /// Create a new empty neighborhood.
    pub fn new_empty(neighborhood_id: NeighborhoodId) -> Self {
        Self {
            neighborhood_id,
            member_blocks: Vec::new(),
            adjacencies: Vec::new(),
        }
    }

    /// Check if a block is a member of this neighborhood.
    pub fn is_member(&self, block_id: BlockId) -> bool {
        self.member_blocks.contains(&block_id)
    }

    /// Check if two blocks are adjacent in this neighborhood.
    pub fn are_adjacent(&self, block_a: BlockId, block_b: BlockId) -> bool {
        // Adjacencies are stored in canonical order (a <= b)
        let (a, b) = if block_a <= block_b {
            (block_a, block_b)
        } else {
            (block_b, block_a)
        };
        self.adjacencies.contains(&(a, b))
    }

    /// Get all blocks adjacent to a given block.
    pub fn neighbors_of(&self, block: BlockId) -> Vec<BlockId> {
        let mut neighbors = Vec::new();
        for (a, b) in &self.adjacencies {
            if *a == block {
                neighbors.push(*b);
            } else if *b == block {
                neighbors.push(*a);
            }
        }
        neighbors
    }

    /// Get the number of member blocks.
    pub fn member_count(&self) -> usize {
        self.member_blocks.len()
    }

    /// Get all unique authorities that are neighbors through this neighborhood.
    ///
    /// Given a block, returns all blocks that share adjacency with it.
    /// This includes direct adjacencies only (1-hop).
    pub fn adjacent_blocks(&self, from_block: BlockId) -> Vec<BlockId> {
        self.neighbors_of(from_block)
    }

    /// Validate that a block can join this neighborhood.
    pub fn validate_block_join(&self, block_id: BlockId) -> Result<(), SocialError> {
        if self.is_member(block_id) {
            return Err(SocialError::AlreadyMember {
                block_id,
                neighborhood_id: self.neighborhood_id,
            });
        }
        Ok(())
    }

    /// Validate that an adjacency can be created.
    ///
    /// Both blocks must be members and not already adjacent.
    pub fn validate_adjacency(
        &self,
        block_a: BlockId,
        block_b: BlockId,
    ) -> Result<(), SocialError> {
        if !self.is_member(block_a) {
            return Err(SocialError::BlockNotFound(block_a));
        }
        if !self.is_member(block_b) {
            return Err(SocialError::BlockNotFound(block_b));
        }
        // Adjacency already exists is not an error for idempotency
        Ok(())
    }

    /// Get all member blocks except the specified one.
    pub fn other_members(&self, exclude: BlockId) -> Vec<BlockId> {
        self.member_blocks
            .iter()
            .filter(|b| **b != exclude)
            .copied()
            .collect()
    }

    /// Build a set of all blocks reachable from a given block via adjacencies.
    ///
    /// This performs a breadth-first search through adjacencies.
    pub fn reachable_from(&self, start: BlockId) -> HashSet<BlockId> {
        let mut visited = HashSet::new();
        let mut queue = vec![start];

        while let Some(block) = queue.pop() {
            if visited.insert(block) {
                for neighbor in self.neighbors_of(block) {
                    if !visited.contains(&neighbor) {
                        queue.push(neighbor);
                    }
                }
            }
        }

        visited
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

    #[test]
    fn test_neighborhood_from_facts() {
        let neighborhood_id = NeighborhoodId::from_bytes([1u8; 32]);
        let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, test_timestamp());

        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);
        let block_c = BlockId::from_bytes([3u8; 32]);

        let members = vec![
            BlockMemberFact::new(block_a, neighborhood_id, test_timestamp()),
            BlockMemberFact::new(block_b, neighborhood_id, test_timestamp()),
            BlockMemberFact::new(block_c, neighborhood_id, test_timestamp()),
        ];

        let adjacencies = vec![
            AdjacencyFact::new(block_a, block_b, neighborhood_id),
            AdjacencyFact::new(block_b, block_c, neighborhood_id),
        ];

        let neighborhood = Neighborhood::from_facts(&neighborhood_fact, &members, &adjacencies);

        assert_eq!(neighborhood.member_count(), 3);
        assert!(neighborhood.is_member(block_a));
        assert!(neighborhood.is_member(block_b));
        assert!(neighborhood.is_member(block_c));
    }

    #[test]
    fn test_adjacency_queries() {
        let neighborhood_id = NeighborhoodId::from_bytes([2u8; 32]);
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);
        let block_c = BlockId::from_bytes([3u8; 32]);

        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_blocks = vec![block_a, block_b, block_c];
        neighborhood.adjacencies = vec![(block_a, block_b), (block_b, block_c)];

        // A is adjacent to B
        assert!(neighborhood.are_adjacent(block_a, block_b));
        assert!(neighborhood.are_adjacent(block_b, block_a)); // Order doesn't matter

        // B is adjacent to C
        assert!(neighborhood.are_adjacent(block_b, block_c));

        // A is NOT directly adjacent to C
        assert!(!neighborhood.are_adjacent(block_a, block_c));
    }

    #[test]
    fn test_neighbors_of() {
        let neighborhood_id = NeighborhoodId::from_bytes([3u8; 32]);
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);
        let block_c = BlockId::from_bytes([3u8; 32]);

        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_blocks = vec![block_a, block_b, block_c];
        // B is adjacent to both A and C
        neighborhood.adjacencies = vec![(block_a, block_b), (block_b, block_c)];

        let neighbors_of_b = neighborhood.neighbors_of(block_b);
        assert_eq!(neighbors_of_b.len(), 2);
        assert!(neighbors_of_b.contains(&block_a));
        assert!(neighbors_of_b.contains(&block_c));

        let neighbors_of_a = neighborhood.neighbors_of(block_a);
        assert_eq!(neighbors_of_a.len(), 1);
        assert!(neighbors_of_a.contains(&block_b));
    }

    #[test]
    fn test_reachable_from() {
        let neighborhood_id = NeighborhoodId::from_bytes([4u8; 32]);
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);
        let block_c = BlockId::from_bytes([3u8; 32]);
        let block_d = BlockId::from_bytes([4u8; 32]); // Disconnected

        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_blocks = vec![block_a, block_b, block_c, block_d];
        // A-B-C connected, D isolated
        neighborhood.adjacencies = vec![(block_a, block_b), (block_b, block_c)];

        let reachable = neighborhood.reachable_from(block_a);
        assert!(reachable.contains(&block_a));
        assert!(reachable.contains(&block_b));
        assert!(reachable.contains(&block_c));
        assert!(!reachable.contains(&block_d)); // Not connected

        let reachable_from_d = neighborhood.reachable_from(block_d);
        assert_eq!(reachable_from_d.len(), 1);
        assert!(reachable_from_d.contains(&block_d));
    }

    #[test]
    fn test_validate_block_join() {
        let neighborhood_id = NeighborhoodId::from_bytes([5u8; 32]);
        let block_a = BlockId::from_bytes([1u8; 32]);
        let block_b = BlockId::from_bytes([2u8; 32]);

        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_blocks = vec![block_a];

        // New block can join
        assert!(neighborhood.validate_block_join(block_b).is_ok());

        // Existing member cannot join again
        let result = neighborhood.validate_block_join(block_a);
        assert!(matches!(result, Err(SocialError::AlreadyMember { .. })));
    }
}
