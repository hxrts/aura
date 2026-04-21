//! Property tests for access-level mapping, override rules, and storage allocation bounds.

use crate::support::test_timestamp;
use aura_core::types::identifiers::AuthorityId;
use aura_social::{
    determine_default_access_level, AccessLevel, AccessOverrideFact, Home, HomeFact, HomeId,
    HomeMemberFact, HomeStorageBudget, Neighborhood, NeighborhoodId, NeighborhoodMemberFact,
};
use proptest::prelude::*;

fn home_id(seed: u8) -> HomeId {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    HomeId::from_bytes(bytes)
}

fn neighborhood_id(seed: u8) -> NeighborhoodId {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    NeighborhoodId::from_bytes(bytes)
}

fn authority_id(seed: u8) -> AuthorityId {
    let mut bytes = [0u8; 32];
    bytes[0] = seed;
    AuthorityId::new_from_entropy(bytes)
}

fn access_level_strategy() -> impl Strategy<Value = AccessLevel> {
    prop_oneof![
        Just(AccessLevel::Limited),
        Just(AccessLevel::Partial),
        Just(AccessLevel::Full),
    ]
}

fn allocation_strategy() -> impl Strategy<Value = (u8, u8, u64)> {
    (0u8..=8u8, 0u8..=4u8).prop_flat_map(|(member_count, neighborhood_count)| {
        let member_spent = member_count as u64 * HomeMemberFact::DEFAULT_STORAGE_ALLOCATION;
        let neighborhood_spent =
            neighborhood_count as u64 * NeighborhoodMemberFact::DEFAULT_ALLOCATION;
        let max_pinned =
            HomeFact::DEFAULT_STORAGE_LIMIT.saturating_sub(member_spent + neighborhood_spent);
        (
            Just(member_count),
            Just(neighborhood_count),
            0u64..=max_pinned,
        )
    })
}

fn access_rank(level: AccessLevel) -> u8 {
    match level {
        AccessLevel::Limited => 0,
        AccessLevel::Partial => 1,
        AccessLevel::Full => 2,
    }
}

fn neighborhoods_for_hops(hops: u8) -> (Home, Option<HomeId>, Vec<Neighborhood>) {
    if hops == 0 {
        let target = home_id(1);
        return (Home::new_empty(target), Some(target), Vec::new());
    }

    let homes: Vec<HomeId> = (0..=hops)
        .map(|step| home_id(step.saturating_add(1)))
        .collect();
    let mut neighborhoods = Vec::new();

    for step in 0..hops {
        let index = step as usize;
        let mut neighborhood = Neighborhood::new_empty(neighborhood_id(step.saturating_add(1)));
        neighborhood.member_homes = vec![homes[index], homes[index + 1]];
        neighborhoods.push(neighborhood);
    }

    (
        Home::new_empty(homes[0]),
        Some(homes[hops as usize]),
        neighborhoods,
    )
}

proptest! {
    #[test]
    #[ignore = "property"]
    fn property_access_level_ordering(
        left in access_level_strategy(),
        right in access_level_strategy(),
    ) {
        prop_assert_eq!(left.cmp(&right), access_rank(left).cmp(&access_rank(right)));
    }

    #[test]
    #[ignore = "property"]
    fn property_hop_distance_maps_to_access_level(hops in 0u8..10u8) {
        let (home, authority_home, neighborhoods) = neighborhoods_for_hops(hops);
        let actual = determine_default_access_level(&home, authority_home, &neighborhoods);
        let expected = match hops {
            0 => AccessLevel::Full,
            1 => AccessLevel::Partial,
            _ => AccessLevel::Limited,
        };

        prop_assert_eq!(actual, expected);
    }

    #[test]
    #[ignore = "property"]
    fn property_override_only_downgrades_or_upgrades_one_level(
        default_level in access_level_strategy(),
        override_target in access_level_strategy(),
    ) {
        let expected_allowed = matches!(
            (default_level, override_target),
            (AccessLevel::Limited, AccessLevel::Partial)
                | (AccessLevel::Partial, AccessLevel::Limited)
        );

        prop_assert_eq!(default_level.allows_override_to(override_target), expected_allowed);

        let override_fact = AccessOverrideFact::new_validated(
            authority_id(9),
            home_id(8),
            default_level,
            override_target,
            test_timestamp(),
        );
        prop_assert_eq!(override_fact.is_ok(), expected_allowed);
    }

    #[test]
    #[ignore = "property"]
    fn property_allocations_sum_to_total(
        (member_count, neighborhood_count, pinned) in allocation_strategy(),
    ) {
        let member_spent = member_count as u64 * HomeMemberFact::DEFAULT_STORAGE_ALLOCATION;
        let neighborhood_spent = neighborhood_count as u64 * NeighborhoodMemberFact::DEFAULT_ALLOCATION;

        let mut budget = HomeStorageBudget::new(home_id(7));
        budget.member_storage_spent = member_spent;
        budget.neighborhood_allocations = neighborhood_spent;
        budget.pinned_storage_spent = pinned;

        let spent = budget.member_storage_spent
            + budget.neighborhood_allocations
            + budget.pinned_storage_spent;

        prop_assert!(spent <= HomeFact::DEFAULT_STORAGE_LIMIT);
        prop_assert_eq!(budget.remaining_shared_storage(), HomeFact::DEFAULT_STORAGE_LIMIT - spent);
    }
}
