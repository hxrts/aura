//! Integration tests for hop-based access, capability enforcement, and deterministic views.

use aura_core::types::identifiers::AuthorityId;
use aura_social::{
    determine_access_level, minimum_hop_distance, resolve_access_capabilities, AccessLevel, Home,
    HomeId, Neighborhood, NeighborhoodId,
};

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

fn edge(seed: u8, left: HomeId, right: HomeId) -> Neighborhood {
    let mut neighborhood = Neighborhood::new_empty(neighborhood_id(seed));
    neighborhood.member_homes = vec![left, right];
    neighborhood
}

#[test]
fn test_hop_distance_through_neighborhood() {
    let home_a = home_id(1);
    let home_b = home_id(2);
    let home_c = home_id(3);

    let neighborhoods = vec![edge(1, home_a, home_b), edge(2, home_b, home_c)];

    assert_eq!(
        minimum_hop_distance(home_a, home_c, &neighborhoods),
        Some(2)
    );
}

#[test]
fn test_limited_cannot_access_full_content() {
    let target_home = Home::new_empty(home_id(1));
    let remote_home = Some(home_id(9)); // disconnected => Limited by default
    let requester = authority_id(7);

    let level = determine_access_level(&target_home, requester, remote_home, &[], &[]);
    assert_eq!(level, AccessLevel::Limited);

    let capabilities =
        resolve_access_capabilities(&target_home, requester, remote_home, &[], &[], &[]);

    assert!(capabilities.contains("send_dm"));
    assert!(!capabilities.contains("manage_channel"));
    assert!(!capabilities.contains("grant_moderator"));
}

#[test]
fn test_access_mapping_for_hops_and_disconnected() {
    let home_a = home_id(1);
    let home_b = home_id(2);
    let home_c = home_id(3);
    let home_d = home_id(4);

    let target_home = Home::new_empty(home_a);
    let neighborhoods = vec![edge(1, home_a, home_b), edge(2, home_b, home_c)];
    let requester = authority_id(4);

    let full = determine_access_level(&target_home, requester, Some(home_a), &neighborhoods, &[]);
    let partial =
        determine_access_level(&target_home, requester, Some(home_b), &neighborhoods, &[]);
    let limited_two_hop =
        determine_access_level(&target_home, requester, Some(home_c), &neighborhoods, &[]);
    let limited_disconnected =
        determine_access_level(&target_home, requester, Some(home_d), &neighborhoods, &[]);

    assert_eq!(full, AccessLevel::Full);
    assert_eq!(partial, AccessLevel::Partial);
    assert_eq!(limited_two_hop, AccessLevel::Limited);
    assert_eq!(limited_disconnected, AccessLevel::Limited);
}

#[test]
fn test_access_computation_is_deterministic_for_identical_fact_sets() {
    let home_a = home_id(1);
    let home_b = home_id(2);
    let home_c = home_id(3);
    let requester = authority_id(5);
    let target_home = Home::new_empty(home_a);
    let neighborhoods = vec![edge(1, home_a, home_b), edge(2, home_b, home_c)];

    let first = determine_access_level(&target_home, requester, Some(home_c), &neighborhoods, &[]);
    let second = determine_access_level(&target_home, requester, Some(home_c), &neighborhoods, &[]);

    assert_eq!(first, second);
}
