//! Order-independent reduction of home and neighborhood lifecycle facts
//! (docs/115 §3.5).
//!
//! Governance facts (membership episodes, moderators, access) reduce in
//! `moderation::governance`. This module reduces the remaining `SocialFact`
//! pairs with the shared rules of `aura_journal::causal_reduction`:
//!
//! - Home existence is a tagged observed-remove set. Each `HomeCreated` is an
//!   add tagged by its content; a `HomeDeleted` by the home's creator revokes
//!   the creations it observed. Deletion is terminal: a `HomeId` is minted
//!   once, so recreating a home means creating a new `HomeId`.
//! - Neighborhood membership is a tagged observed-remove set of episodes per
//!   home and neighborhood. A `HomeLeftNeighborhood` revokes the joins its
//!   writer observed; a later join is a new episode and survives.
//! - Storage readings are a multi-value register per home. Concurrent
//!   survivors resolve to the largest usage, then the largest capacity, then
//!   the later write in causal order.
//! - Neighborhood names come from `NeighborhoodCreated`; duplicate creations
//!   of one id resolve to the smallest name.
//!
//! Physical timestamps never order these facts.

use crate::facts::{HomeId, NeighborhoodId, SocialFact};
use aura_core::time::{CausalMetadata, CausalTag};
use aura_journal::causal_reduction::{
    causal_cmp, observed_remove_live, register_survivors, CausalFact,
};
use std::collections::{BTreeMap, BTreeSet};

/// Reduced storage reading of one home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeStorageReading {
    /// Total bytes used.
    pub used_bytes: u64,
    /// Total bytes available.
    pub total_bytes: u64,
}

/// Lifecycle state reduced from a set of social facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SocialLifecycle {
    /// Homes with a live (unrevoked) creation.
    pub live_homes: BTreeSet<HomeId>,
    /// Homes whose every observed creation was deleted.
    pub deleted_homes: BTreeSet<HomeId>,
    /// Live neighborhood memberships per home.
    pub neighborhoods: BTreeMap<HomeId, BTreeSet<NeighborhoodId>>,
    /// Memberships with join facts but no live episode.
    pub left_neighborhoods: BTreeMap<HomeId, BTreeSet<NeighborhoodId>>,
    /// Neighborhood names by id.
    pub neighborhood_names: BTreeMap<NeighborhoodId, String>,
    /// Resolved storage reading per home.
    pub storage: BTreeMap<HomeId, HomeStorageReading>,
}

/// Whether `fact` is reduced here rather than by home governance.
#[must_use]
pub fn is_lifecycle_fact(fact: &SocialFact) -> bool {
    matches!(
        fact,
        SocialFact::HomeCreated { .. }
            | SocialFact::HomeDeleted { .. }
            | SocialFact::NeighborhoodCreated { .. }
            | SocialFact::HomeJoinedNeighborhood { .. }
            | SocialFact::HomeLeftNeighborhood { .. }
            | SocialFact::StorageUpdated { .. }
    )
}

/// The lifecycle facts held across batches, keyed by content tag so replays
/// are idempotent.
#[derive(Debug, Clone, Default)]
pub struct SocialLifecycleLog {
    facts: BTreeMap<CausalTag, SocialFact>,
}

impl SocialLifecycleLog {
    /// Add a lifecycle fact; returns false for a fact already held or one
    /// that is not a lifecycle fact.
    pub fn insert(&mut self, fact: &SocialFact) -> bool {
        is_lifecycle_fact(fact)
            && self
                .facts
                .insert(fact.content_tag(), fact.clone())
                .is_none()
    }

    /// Reduce the held facts.
    #[must_use]
    pub fn reduce(&self) -> SocialLifecycle {
        SocialLifecycle::from_facts(self.facts.values())
    }
}

struct StorageWrite<'a> {
    tag: CausalTag,
    causal: &'a CausalMetadata,
    reading: HomeStorageReading,
}

impl CausalFact for StorageWrite<'_> {
    fn causal_tag(&self) -> CausalTag {
        self.tag
    }

    fn causal_metadata(&self) -> &CausalMetadata {
        self.causal
    }
}

impl SocialLifecycle {
    /// Reduce lifecycle facts; a pure function of the fact set.
    pub fn from_facts<'a>(facts: impl IntoIterator<Item = &'a SocialFact>) -> Self {
        let facts: Vec<&SocialFact> = facts.into_iter().collect();
        let mut state = Self::default();

        let mut creations = Vec::new();
        let mut creators = BTreeMap::new();
        let mut joins = Vec::new();
        let mut storage: BTreeMap<HomeId, Vec<StorageWrite<'_>>> = BTreeMap::new();
        for fact in &facts {
            match fact {
                SocialFact::HomeCreated {
                    home_id,
                    creator_id,
                    ..
                } => {
                    let tag = fact.content_tag();
                    creators.insert(tag, *creator_id);
                    creations.push((*home_id, tag));
                }
                SocialFact::HomeJoinedNeighborhood {
                    home_id,
                    neighborhood_id,
                    ..
                } => joins.push(((*home_id, *neighborhood_id), fact.content_tag())),
                SocialFact::NeighborhoodCreated {
                    neighborhood_id,
                    name,
                    ..
                } => {
                    let entry = state
                        .neighborhood_names
                        .entry(*neighborhood_id)
                        .or_insert_with(|| name.clone());
                    if name < entry {
                        entry.clone_from(name);
                    }
                }
                SocialFact::StorageUpdated {
                    home_id,
                    used_bytes,
                    total_bytes,
                    causal,
                    ..
                } => storage.entry(*home_id).or_default().push(StorageWrite {
                    tag: fact.content_tag(),
                    causal,
                    reading: HomeStorageReading {
                        used_bytes: *used_bytes,
                        total_bytes: *total_bytes,
                    },
                }),
                _ => {}
            }
        }

        // Only the home's creator deletes it: other deletions revoke nothing.
        let deletions: Vec<(HomeId, Vec<CausalTag>)> = facts
            .iter()
            .filter_map(|fact| match fact {
                SocialFact::HomeDeleted {
                    home_id,
                    actor_id,
                    causal,
                    ..
                } => Some((
                    *home_id,
                    causal
                        .revokes
                        .iter()
                        .copied()
                        .filter(|tag| creators.get(tag) == Some(actor_id))
                        .collect(),
                )),
                _ => None,
            })
            .collect();
        let deletions: Vec<(HomeId, &[CausalTag])> = deletions
            .iter()
            .map(|(home, revokes)| (*home, revokes.as_slice()))
            .collect();
        let live = observed_remove_live(&creations, &deletions);
        for (home, tag) in &creations {
            if live.contains(tag) {
                state.live_homes.insert(*home);
            }
        }
        for (home, _) in &creations {
            if !state.live_homes.contains(home) {
                state.deleted_homes.insert(*home);
            }
        }

        let leaves: Vec<((HomeId, NeighborhoodId), &[CausalTag])> = facts
            .iter()
            .filter_map(|fact| match fact {
                SocialFact::HomeLeftNeighborhood {
                    home_id,
                    neighborhood_id,
                    causal,
                    ..
                } => Some(((*home_id, *neighborhood_id), causal.revokes.as_slice())),
                _ => None,
            })
            .collect();
        let live = observed_remove_live(&joins, &leaves);
        for ((home, neighborhood), tag) in &joins {
            if live.contains(tag) {
                state
                    .neighborhoods
                    .entry(*home)
                    .or_default()
                    .insert(*neighborhood);
            }
        }
        for ((home, neighborhood), _) in &joins {
            if !state
                .neighborhoods
                .get(home)
                .is_some_and(|set| set.contains(neighborhood))
            {
                state
                    .left_neighborhoods
                    .entry(*home)
                    .or_default()
                    .insert(*neighborhood);
            }
        }

        for (home, writes) in &storage {
            let refs: Vec<&StorageWrite<'_>> = writes.iter().collect();
            let survivors = register_survivors(&refs);
            let resolved = writes
                .iter()
                .filter(|write| survivors.contains(&write.tag))
                .max_by(|a, b| {
                    a.reading
                        .used_bytes
                        .cmp(&b.reading.used_bytes)
                        .then(a.reading.total_bytes.cmp(&b.reading.total_bytes))
                        .then_with(|| causal_cmp(*a, *b))
                });
            if let Some(write) = resolved {
                state.storage.insert(*home, write.reading);
            }
        }

        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::CausalClock;
    use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
    use aura_journal::causal_reduction::assert_permutation_invariant;

    fn context() -> ContextId {
        ContextId::new_from_entropy([3; 32])
    }

    fn home() -> HomeId {
        HomeId::from_bytes([4; 32])
    }

    fn creator() -> AuthorityId {
        AuthorityId::new_from_entropy([5; 32])
    }

    fn clock(device: u8, counter: u64) -> CausalClock {
        CausalClock {
            lamport: counter,
            vector: vec![(DeviceId(uuid::Uuid::from_bytes([device; 16])), counter)],
        }
    }

    fn causal(
        revokes: Vec<CausalTag>,
        supersedes: Vec<CausalTag>,
        at: CausalClock,
    ) -> CausalMetadata {
        CausalMetadata {
            revokes,
            supersedes,
            clock: at,
        }
    }

    fn created() -> SocialFact {
        SocialFact::home_created_ms(home(), context(), 10, creator(), "Den".into())
    }

    fn reduce(facts: &[SocialFact]) -> SocialLifecycle {
        SocialLifecycle::from_facts(facts)
    }

    #[test]
    fn deletion_is_terminal_in_every_order() {
        let created = created();
        let deleted = SocialFact::home_deleted_ms(
            home(),
            context(),
            1,
            creator(),
            causal(vec![created.content_tag()], vec![], clock(1, 2)),
        );
        let state = assert_permutation_invariant(&[created, deleted], reduce);
        assert!(state.live_homes.is_empty());
        assert_eq!(state.deleted_homes, BTreeSet::from([home()]));
    }

    #[test]
    fn deletion_by_a_non_creator_or_without_observing_creation_removes_nothing() {
        let created = created();
        let foreign = SocialFact::home_deleted_ms(
            home(),
            context(),
            1,
            AuthorityId::new_from_entropy([9; 32]),
            causal(vec![created.content_tag()], vec![], clock(2, 1)),
        );
        let blind = SocialFact::home_deleted_ms(
            home(),
            context(),
            2,
            creator(),
            causal(vec![], vec![], clock(1, 1)),
        );
        let state = assert_permutation_invariant(&[created, foreign, blind], reduce);
        assert_eq!(state.live_homes, BTreeSet::from([home()]));
    }

    #[test]
    fn neighborhood_leave_revokes_observed_join_and_rejoin_survives() {
        let neighborhood = NeighborhoodId::from_bytes([6; 32]);
        let named = SocialFact::neighborhood_created_ms(neighborhood, context(), 1, "Block".into());
        let joined = SocialFact::home_joined_neighborhood_ms(home(), neighborhood, context(), 1);
        let left = SocialFact::home_left_neighborhood_ms(
            home(),
            neighborhood,
            context(),
            2,
            causal(vec![joined.content_tag()], vec![], clock(1, 2)),
        );
        let after_leave =
            assert_permutation_invariant(&[named.clone(), joined.clone(), left.clone()], reduce);
        assert!(after_leave.neighborhoods.is_empty());
        assert_eq!(
            after_leave.left_neighborhoods[&home()],
            BTreeSet::from([neighborhood])
        );
        assert_eq!(after_leave.neighborhood_names[&neighborhood], "Block");

        let rejoined = SocialFact::home_joined_neighborhood_ms(home(), neighborhood, context(), 3);
        let state = assert_permutation_invariant(&[named, joined, left, rejoined], reduce);
        assert_eq!(state.neighborhoods[&home()], BTreeSet::from([neighborhood]));
        assert!(state.left_neighborhoods.is_empty());
    }

    #[test]
    fn storage_register_supersedes_observed_and_resolves_concurrent_deterministically() {
        let reading = |used, at: CausalClock, supersedes| {
            SocialFact::storage_updated_ms(
                home(),
                context(),
                used,
                1_000,
                0,
                causal(vec![], supersedes, at),
            )
        };
        let first = reading(900, clock(1, 1), vec![]);
        let replacement = reading(100, clock(1, 2), vec![first.content_tag()]);
        let concurrent = reading(300, clock(2, 1), vec![]);
        let state = assert_permutation_invariant(&[first, replacement, concurrent], reduce);
        assert_eq!(state.storage[&home()].used_bytes, 300);
    }
}
