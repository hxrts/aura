//! Neighborhood materialized view
//!
//! Provides a materialized view of a neighborhood aggregated from journal facts.

use crate::error::SocialError;
use crate::facts::{
    AdjacencyFact, HomeId, HomeMemberFact, NeighborhoodFact, NeighborhoodId,
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
    pub member_homes: Vec<HomeId>,
    /// Adjacency relationships between blocks
    pub adjacencies: Vec<(HomeId, HomeId)>,
}

impl Neighborhood {
    /// Build a Neighborhood view from journal facts.
    ///
    /// # Arguments
    /// * `fact` - The neighborhood existence fact
    /// * `members` - All home membership facts for this neighborhood
    /// * `adjacencies` - All adjacency facts for this neighborhood
    pub fn from_facts(
        fact: &NeighborhoodFact,
        members: &[HomeMemberFact],
        adjacencies: &[AdjacencyFact],
    ) -> Self {
        Self {
            neighborhood_id: fact.neighborhood_id,
            member_homes: members.iter().map(|m| m.home_id).collect(),
            adjacencies: adjacencies.iter().map(|a| (a.home_a, a.home_b)).collect(),
        }
    }

    /// Create a new empty neighborhood.
    pub fn new_empty(neighborhood_id: NeighborhoodId) -> Self {
        Self {
            neighborhood_id,
            member_homes: Vec::new(),
            adjacencies: Vec::new(),
        }
    }

    /// Check if a home is a member of this neighborhood.
    pub fn is_member(&self, home_id: HomeId) -> bool {
        self.member_homes.contains(&home_id)
    }

    /// Check if two blocks are adjacent in this neighborhood.
    pub fn are_adjacent(&self, home_a: HomeId, home_b: HomeId) -> bool {
        // Adjacencies are stored in canonical order (a <= b)
        let (a, b) = if home_a <= home_b {
            (home_a, home_b)
        } else {
            (home_b, home_a)
        };
        self.adjacencies.contains(&(a, b))
    }

    /// Get all homes adjacent to a given home.
    pub fn neighbors_of(&self, home: HomeId) -> Vec<HomeId> {
        let mut neighbors = Vec::new();
        for (a, b) in &self.adjacencies {
            if *a == home {
                neighbors.push(*b);
            } else if *b == home {
                neighbors.push(*a);
            }
        }
        neighbors
    }

    /// Get the number of member blocks.
    pub fn member_count(&self) -> usize {
        self.member_homes.len()
    }

    /// Get all unique authorities that are neighbors through this neighborhood.
    ///
    /// Given a home, returns all blocks that share adjacency with it.
    /// This includes direct adjacencies only (1-hop).
    pub fn adjacent_homes(&self, from_home: HomeId) -> Vec<HomeId> {
        self.neighbors_of(from_home)
    }

    /// Validate that a home can join this neighborhood.
    pub fn validate_home_join(&self, home_id: HomeId) -> Result<(), SocialError> {
        if self.is_member(home_id) {
            return Err(SocialError::AlreadyMember {
                home_id,
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
        home_a: HomeId,
        home_b: HomeId,
    ) -> Result<(), SocialError> {
        if !self.is_member(home_a) {
            return Err(SocialError::HomeNotFound(home_a));
        }
        if !self.is_member(home_b) {
            return Err(SocialError::HomeNotFound(home_b));
        }
        // Adjacency already exists is not an error for idempotency
        Ok(())
    }

    /// Get all member blocks except the specified one.
    pub fn other_members(&self, exclude: HomeId) -> Vec<HomeId> {
        self.member_homes
            .iter()
            .filter(|b| **b != exclude)
            .copied()
            .collect()
    }

    /// Build a set of all homes reachable from a given home via adjacencies.
    ///
    /// This performs a breadth-first search through adjacencies.
    pub fn reachable_from(&self, start: HomeId) -> HashSet<HomeId> {
        let mut visited = HashSet::new();
        let mut queue = vec![start];

        while let Some(home) = queue.pop() {
            if visited.insert(home) {
                for neighbor in self.neighbors_of(home) {
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

        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let home_c = HomeId::from_bytes([3u8; 32]);

        let members = vec![
            HomeMemberFact::new(home_a, neighborhood_id, test_timestamp()),
            HomeMemberFact::new(home_b, neighborhood_id, test_timestamp()),
            HomeMemberFact::new(home_c, neighborhood_id, test_timestamp()),
        ];

        let adjacencies = vec![
            AdjacencyFact::new(home_a, home_b, neighborhood_id),
            AdjacencyFact::new(home_b, home_c, neighborhood_id),
        ];

        let neighborhood = Neighborhood::from_facts(&neighborhood_fact, &members, &adjacencies);

        assert_eq!(neighborhood.member_count(), 3);
        assert!(neighborhood.is_member(home_a));
        assert!(neighborhood.is_member(home_b));
        assert!(neighborhood.is_member(home_c));
    }

    #[test]
    fn test_adjacency_queries() {
        let neighborhood_id = NeighborhoodId::from_bytes([2u8; 32]);
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let home_c = HomeId::from_bytes([3u8; 32]);

        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_homes = vec![home_a, home_b, home_c];
        neighborhood.adjacencies = vec![(home_a, home_b), (home_b, home_c)];

        // A is adjacent to B
        assert!(neighborhood.are_adjacent(home_a, home_b));
        assert!(neighborhood.are_adjacent(home_b, home_a)); // Order doesn't matter

        // B is adjacent to C
        assert!(neighborhood.are_adjacent(home_b, home_c));

        // A is NOT directly adjacent to C
        assert!(!neighborhood.are_adjacent(home_a, home_c));
    }

    #[test]
    fn test_neighbors_of() {
        let neighborhood_id = NeighborhoodId::from_bytes([3u8; 32]);
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let home_c = HomeId::from_bytes([3u8; 32]);

        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_homes = vec![home_a, home_b, home_c];
        // B is adjacent to both A and C
        neighborhood.adjacencies = vec![(home_a, home_b), (home_b, home_c)];

        let neighbors_of_b = neighborhood.neighbors_of(home_b);
        assert_eq!(neighbors_of_b.len(), 2);
        assert!(neighbors_of_b.contains(&home_a));
        assert!(neighbors_of_b.contains(&home_c));

        let neighbors_of_a = neighborhood.neighbors_of(home_a);
        assert_eq!(neighbors_of_a.len(), 1);
        assert!(neighbors_of_a.contains(&home_b));
    }

    #[test]
    fn test_reachable_from() {
        let neighborhood_id = NeighborhoodId::from_bytes([4u8; 32]);
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);
        let home_c = HomeId::from_bytes([3u8; 32]);
        let home_d = HomeId::from_bytes([4u8; 32]); // Disconnected

        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_homes = vec![home_a, home_b, home_c, home_d];
        // A-B-C connected, D isolated
        neighborhood.adjacencies = vec![(home_a, home_b), (home_b, home_c)];

        let reachable = neighborhood.reachable_from(home_a);
        assert!(reachable.contains(&home_a));
        assert!(reachable.contains(&home_b));
        assert!(reachable.contains(&home_c));
        assert!(!reachable.contains(&home_d)); // Not connected

        let reachable_from_d = neighborhood.reachable_from(home_d);
        assert_eq!(reachable_from_d.len(), 1);
        assert!(reachable_from_d.contains(&home_d));
    }

    #[test]
    fn test_validate_home_join() {
        let neighborhood_id = NeighborhoodId::from_bytes([5u8; 32]);
        let home_a = HomeId::from_bytes([1u8; 32]);
        let home_b = HomeId::from_bytes([2u8; 32]);

        let mut neighborhood = Neighborhood::new_empty(neighborhood_id);
        neighborhood.member_homes = vec![home_a];

        // New home can join
        assert!(neighborhood.validate_home_join(home_b).is_ok());

        // Existing member cannot join again
        let result = neighborhood.validate_home_join(home_a);
        assert!(matches!(result, Err(SocialError::AlreadyMember { .. })));
    }
}
