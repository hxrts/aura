//! Storage service
//!
//! Provides storage budget enforcement and allocation policies.

use crate::error::SocialError;
use crate::facts::{HomeFact, HomeMemberFact, NeighborhoodMemberFact, HomeStorageBudget};

/// Service for storage budget calculations and validation.
pub struct StorageService;

impl StorageService {
    /// Validate that a storage allocation request can be satisfied.
    pub fn validate_allocation(
        budget: &HomeStorageBudget,
        requested: u64,
    ) -> Result<(), SocialError> {
        let available = budget.remaining_shared_storage();
        if requested > available {
            return Err(SocialError::storage_exceeded(available, requested));
        }
        Ok(())
    }

    /// Calculate available space in a home.
    pub fn available_space(budget: &HomeStorageBudget) -> u64 {
        budget.remaining_shared_storage()
    }

    /// Check if content can be pinned.
    pub fn can_pin(budget: &HomeStorageBudget, size: u64) -> bool {
        let pinned_limit = budget.pinned_storage_limit();
        let available = pinned_limit.saturating_sub(budget.pinned_storage_spent);
        size <= available
    }

    /// Calculate the total storage used by members.
    pub fn member_storage_used(members: &[HomeMemberFact]) -> u64 {
        members.iter().map(|r| r.storage_allocated).sum()
    }

    /// Calculate neighborhood allocation obligations.
    pub fn neighborhood_allocations(memberships: &[NeighborhoodMemberFact]) -> u64 {
        memberships.iter().map(|m| m.allocated_storage).sum()
    }

    /// Build a storage budget from component facts.
    pub fn build_budget(
        home_fact: &HomeFact,
        members: &[HomeMemberFact],
        memberships: &[NeighborhoodMemberFact],
        pinned_storage: u64,
    ) -> HomeStorageBudget {
        HomeStorageBudget {
            home_id: home_fact.home_id,
            member_storage_spent: Self::member_storage_used(members),
            pinned_storage_spent: pinned_storage,
            neighborhood_allocations: Self::neighborhood_allocations(memberships),
        }
    }

    /// Calculate remaining member allocation capacity.
    ///
    /// Returns how much more storage could be allocated to members.
    pub fn remaining_member_capacity(budget: &HomeStorageBudget) -> u64 {
        let limit = budget.member_storage_limit();
        limit.saturating_sub(budget.member_storage_spent)
    }

    /// Validate that a new member can be added with default allocation.
    pub fn validate_new_member(budget: &HomeStorageBudget) -> Result<(), SocialError> {
        let default_allocation = HomeMemberFact::DEFAULT_STORAGE_ALLOCATION;
        let remaining = Self::remaining_member_capacity(budget);

        if default_allocation > remaining {
            return Err(SocialError::storage_exceeded(remaining, default_allocation));
        }
        Ok(())
    }

    /// Validate that a home can join another neighborhood.
    ///
    /// Joining requires donating storage to the neighborhood.
    pub fn validate_neighborhood_join(budget: &HomeStorageBudget) -> Result<(), SocialError> {
        let allocation = NeighborhoodMemberFact::DEFAULT_ALLOCATION;
        let available = budget.remaining_shared_storage();

        if allocation > available {
            return Err(SocialError::storage_exceeded(available, allocation));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{HomeId, NeighborhoodId};
    use aura_core::time::{PhysicalTime, TimeStamp};

    fn test_timestamp() -> TimeStamp {
        TimeStamp::PhysicalClock(PhysicalTime {
            ts_ms: 1700000000000,
            uncertainty: None,
        })
    }

    #[test]
    fn test_available_space_empty_block() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let budget = HomeStorageBudget::new(home_id);

        // Empty home has full 10 MB available
        let available = StorageService::available_space(&budget);
        assert_eq!(available, HomeFact::DEFAULT_STORAGE_LIMIT);
    }

    #[test]
    fn test_available_space_with_members() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let mut budget = HomeStorageBudget::new(home_id);

        // Add storage for 4 members
        budget.member_storage_spent = 4 * HomeMemberFact::DEFAULT_STORAGE_ALLOCATION;

        let available = StorageService::available_space(&budget);
        let expected =
            HomeFact::DEFAULT_STORAGE_LIMIT - (4 * HomeMemberFact::DEFAULT_STORAGE_ALLOCATION);
        assert_eq!(available, expected);
    }

    #[test]
    fn test_validate_allocation_success() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let budget = HomeStorageBudget::new(home_id);

        // Request less than available
        assert!(StorageService::validate_allocation(&budget, 1000).is_ok());
    }

    #[test]
    fn test_validate_allocation_failure() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let mut budget = HomeStorageBudget::new(home_id);

        // Use up all storage
        budget.member_storage_spent = HomeFact::DEFAULT_STORAGE_LIMIT;

        // Request more than available
        let result = StorageService::validate_allocation(&budget, 1000);
        assert!(matches!(result, Err(SocialError::StorageExceeded { .. })));
    }

    #[test]
    fn test_can_pin() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let budget = HomeStorageBudget::new(home_id);

        // Can pin reasonable size
        assert!(StorageService::can_pin(&budget, 1024 * 1024)); // 1 MB

        // Cannot pin more than limit
        assert!(!StorageService::can_pin(
            &budget,
            HomeFact::DEFAULT_STORAGE_LIMIT + 1
        ));
    }

    #[test]
    fn test_build_budget() {
        let home_id = HomeId::from_bytes([1u8; 32]);
        let neighborhood_id = NeighborhoodId::from_bytes([1u8; 32]);

        let home_fact = HomeFact::new(home_id, test_timestamp());
        let members = vec![
            HomeMemberFact::new(
                aura_core::identifiers::AuthorityId::new_from_entropy([1u8; 32]),
                home_id,
                test_timestamp(),
            ),
            HomeMemberFact::new(
                aura_core::identifiers::AuthorityId::new_from_entropy([2u8; 32]),
                home_id,
                test_timestamp(),
            ),
        ];
        let memberships = vec![NeighborhoodMemberFact::new(
            home_id,
            neighborhood_id,
            test_timestamp(),
        )];

        let budget = StorageService::build_budget(&home_fact, &members, &memberships, 0);

        assert_eq!(
            budget.member_storage_spent,
            2 * HomeMemberFact::DEFAULT_STORAGE_ALLOCATION
        );
        assert_eq!(
            budget.neighborhood_allocations,
            NeighborhoodMemberFact::DEFAULT_ALLOCATION
        );
    }
}
