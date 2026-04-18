use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::types::identifiers::AuthorityId;
use aura_social::facts::{
    HomeFact, HomeId, HomeMemberFact, ModeratorFact, NeighborhoodFact, NeighborhoodId,
    NeighborhoodMemberFact, OneHopLinkFact,
};
use aura_social::{Home, Neighborhood};

pub fn test_timestamp() -> TimeStamp {
    TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms: 1_700_000_000_000,
        uncertainty: None,
    })
}

pub fn test_authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

pub fn test_home_id(seed: u8) -> HomeId {
    HomeId::from_bytes([seed; 32])
}

pub fn test_neighborhood_id(seed: u8) -> NeighborhoodId {
    NeighborhoodId::from_bytes([seed; 32])
}

pub fn create_home(home_seed: u8, member_count: usize) -> (Home, AuthorityId, Vec<AuthorityId>) {
    let home_id = test_home_id(home_seed);
    let timestamp = test_timestamp();
    let home_fact = HomeFact::new(home_id, timestamp.clone());

    let mut members = Vec::with_capacity(member_count);
    let mut member_facts = Vec::with_capacity(member_count);

    for i in 0..member_count {
        let authority = test_authority((home_seed * 10) + i as u8 + 1);
        members.push(authority);
        member_facts.push(HomeMemberFact::new(authority, home_id, timestamp.clone()));
    }

    let moderator = members[0];
    let moderator_facts = vec![ModeratorFact::new(moderator, home_id, timestamp)];
    let home = Home::from_facts(&home_fact, None, &member_facts, &moderator_facts);

    (home, moderator, members)
}

pub fn create_neighborhood(neighborhood_seed: u8, home_ids: Vec<HomeId>) -> Neighborhood {
    let neighborhood_id = test_neighborhood_id(neighborhood_seed);
    let timestamp = test_timestamp();
    let neighborhood_fact = NeighborhoodFact::new(neighborhood_id, timestamp.clone());

    let member_facts = home_ids
        .iter()
        .map(|home_id| NeighborhoodMemberFact::new(*home_id, neighborhood_id, timestamp.clone()))
        .collect::<Vec<_>>();

    let mut one_hop_link_facts = Vec::new();
    for i in 0..home_ids.len().saturating_sub(1) {
        one_hop_link_facts.push(OneHopLinkFact::new(
            home_ids[i],
            home_ids[i + 1],
            neighborhood_id,
        ));
    }

    Neighborhood::from_facts(&neighborhood_fact, &member_facts, &one_hop_link_facts)
}
