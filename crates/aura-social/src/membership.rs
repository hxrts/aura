//! Membership validation logic
//!
//! Provides validation for membership operations on blocks and neighborhoods.

use crate::{error::SocialError, home::Home, Neighborhood};
use aura_core::identifiers::AuthorityId;
use crate::facts::{HomeId, NeighborhoodId};

/// Validates membership operations for blocks and neighborhoods.
pub struct MembershipValidator;

impl MembershipValidator {
    /// Validate that an authority can join a home as a resident.
    pub fn validate_home_join(home: &Home, authority: &AuthorityId) -> Result<(), SocialError> {
        home.validate_join(authority)
    }

    /// Validate that a home can join a neighborhood.
    pub fn validate_neighborhood_join(
        home: &Home,
        neighborhood: &Neighborhood,
        current_neighborhood_count: usize,
    ) -> Result<(), SocialError> {
        // Check if home is already a member
        neighborhood.validate_home_join(home.home_id)?;

        // Check if home has capacity for another neighborhood
        if current_neighborhood_count >= home.neighborhood_limit as usize {
            return Err(SocialError::NeighborhoodLimitReached {
                home_id: home.home_id,
                max: home.neighborhood_limit,
            });
        }

        Ok(())
    }

    /// Validate that an adjacency can be created between two blocks.
    pub fn validate_adjacency(
        neighborhood: &Neighborhood,
        home_a: HomeId,
        home_b: HomeId,
    ) -> Result<(), SocialError> {
        neighborhood.validate_adjacency(home_a, home_b)
    }

    /// Check if an authority has the minimum required relationship
    /// to be considered a home peer or neighborhood peer.
    pub fn can_relay_for(
        home: &Home,
        neighborhoods: &[Neighborhood],
        authority: &AuthorityId,
        target: &AuthorityId,
        target_block: Option<HomeId>,
    ) -> bool {
        // Home peers can relay for each other
        if home.is_resident(authority) && home.is_resident(target) {
            return true;
        }

        // Check neighborhood peer relationships
        if let Some(target_home_id) = target_block {
            for neighborhood in neighborhoods {
                if neighborhood.is_member(home.home_id) && neighborhood.is_member(target_home_id)
                {
                    // Both blocks are in the same neighborhood
                    return true;
                }
            }
        }

        false
    }
}

/// Membership state tracking for a single authority.
#[derive(Debug, Clone, Default)]
pub struct MembershipState {
    /// The home the authority resides in, if any
    pub resident_block: Option<HomeId>,
    /// Neighborhoods the authority.s home belongs to
    pub neighborhood_memberships: Vec<NeighborhoodId>,
}

impl MembershipState {
    /// Create a new membership state.
    pub fn new(
        resident_block: Option<HomeId>,
        neighborhood_memberships: Vec<NeighborhoodId>,
    ) -> Self {
        Self {
            resident_block,
            neighborhood_memberships,
        }
    }

    /// Check if the authority has any social relationships.
    pub fn has_social_presence(&self) -> bool {
        self.resident_block.is_some()
    }

    /// Check if the authority is in a specific neighborhood.
    pub fn is_in_neighborhood(&self, neighborhood_id: NeighborhoodId) -> bool {
        self.neighborhood_memberships.contains(&neighborhood_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_validate_home_join() {
        let home_instance = Home::new_empty(HomeId::from_bytes([1u8; 32]));
        let authority = test_authority(1);

        assert!(MembershipValidator::validate_home_join(&home_instance, &authority).is_ok());
    }

    #[test]
    fn test_validate_neighborhood_join() {
        let home_instance = Home::new_empty(HomeId::from_bytes([1u8; 32]));
        let neighborhood = Neighborhood::new_empty(NeighborhoodId::from_bytes([1u8; 32]));

        // Can join with 0 current memberships
        assert!(MembershipValidator::validate_neighborhood_join(&home_instance, &neighborhood, 0).is_ok());

        // Cannot join at limit (4 for v1)
        let result = MembershipValidator::validate_neighborhood_join(&home_instance, &neighborhood, 4);
        assert!(matches!(
            result,
            Err(SocialError::NeighborhoodLimitReached { .. })
        ));
    }

    #[test]
    fn test_membership_state() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let neighborhood_id = NeighborhoodId::from_bytes([1u8; 32]);

        let state = MembershipState::new(Some(home_id), vec![neighborhood_id]);

        assert!(state.has_social_presence());
        assert!(state.is_in_neighborhood(neighborhood_id));
        assert!(!state.is_in_neighborhood(NeighborhoodId::from_bytes([2u8; 32])));
    }
}
