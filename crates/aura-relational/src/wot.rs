//! Web-of-trust facts and order-independent derivation (docs/105 §4.2.1).
//!
//! Direct friend relationships are bilateral relational-context facts reduced
//! as a causal state machine per unordered authority pair:
//!
//! - `Proposed` and `Accepted` are adds of a tagged observed-remove set.
//! - `Accepted` supersedes the proposals its writer observed: a superseded
//!   proposal is consumed and never pending again, however late it arrives.
//! - `Revoked` revokes the proposals and acceptances its writer observed,
//!   ending that friendship episode. A later proposal is a new add and starts
//!   a new episode; an acceptance concurrent with a revocation survives it.
//!
//! A pair is `Friends` while any acceptance is live, otherwise `Pending` on
//! the causally latest live unconsumed proposal. Introductions are a tagged
//! observed-remove set per `(introducer, introduced)`; expiry is explicit
//! validity evaluated at query time against a caller-supplied instant, never
//! inside reduction. Physical timestamps are display and expiry data only.

use crate::reducer_support::{reduce_typed_envelope, stable_authority_pair_bytes};
use aura_core::service::{BootstrapIntroductionHint, LinkEndpoint, ProviderEvidence};
use aura_core::time::{CausalClock, CausalMetadata, CausalTag, LogicalTime, PhysicalTime};
use aura_core::types::identifiers::{AuthorityId, ContextId, DeviceId};
use aura_journal::causal_reduction::{causal_cmp, observed_remove_live, CausalFact};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const FRIENDSHIP_FACT_TYPE_ID: &str = "friendship";
pub const TRUST_INTRODUCTION_FACT_TYPE_ID: &str = "trust_introduction";

/// Unordered authority pair, smaller id first: the key of one friendship.
pub type FriendshipPair = (AuthorityId, AuthorityId);

fn unordered_pair(a: AuthorityId, b: AuthorityId) -> FriendshipPair {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendshipFactKey {
    pub sub_type: &'static str,
    pub data: Vec<u8>,
}

/// Bilateral friendship lifecycle facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(type_id = "friendship", schema_version = 2, context = "context_id")]
pub enum FriendshipFact {
    Proposed {
        context_id: ContextId,
        requester: AuthorityId,
        accepter: AuthorityId,
        proposed_at: PhysicalTime,
        /// Stamped by the writer; see [`friendship_causal`].
        causal: CausalMetadata,
    },
    Accepted {
        context_id: ContextId,
        requester: AuthorityId,
        accepter: AuthorityId,
        accepted_at: PhysicalTime,
        /// Supersedes the observed proposals of the pair.
        causal: CausalMetadata,
    },
    Revoked {
        context_id: ContextId,
        requester: AuthorityId,
        accepter: AuthorityId,
        revoked_at: PhysicalTime,
        /// Revokes the observed proposals and acceptances of the pair.
        causal: CausalMetadata,
    },
}

impl FriendshipFact {
    /// Decode required friendship evidence without treating corruption as absence.
    ///
    /// # Errors
    /// Returns domain, schema, payload-bound or native declared-codec failures.
    pub fn try_from_envelope(
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Result<Self, aura_core::types::facts::FactError> {
        aura_core::types::facts::try_decode_envelope(
            &aura_core::types::facts::FactTypeId::from(FRIENDSHIP_FACT_TYPE_ID),
            2,
            2,
            envelope,
        )
    }

    pub fn participants(&self) -> (AuthorityId, AuthorityId) {
        match self {
            Self::Proposed {
                requester,
                accepter,
                ..
            }
            | Self::Accepted {
                requester,
                accepter,
                ..
            }
            | Self::Revoked {
                requester,
                accepter,
                ..
            } => (*requester, *accepter),
        }
    }

    /// The friendship the fact is about.
    pub fn pair(&self) -> FriendshipPair {
        let (requester, accepter) = self.participants();
        unordered_pair(requester, accepter)
    }

    /// The writer's causal metadata.
    pub fn causal(&self) -> &CausalMetadata {
        match self {
            Self::Proposed { causal, .. }
            | Self::Accepted { causal, .. }
            | Self::Revoked { causal, .. } => causal,
        }
    }

    pub fn other_participant(&self, local_authority: AuthorityId) -> Option<AuthorityId> {
        let (requester, accepter) = self.participants();
        if requester == local_authority {
            Some(accepter)
        } else if accepter == local_authority {
            Some(requester)
        } else {
            None
        }
    }

    pub fn binding_key(&self) -> FriendshipFactKey {
        let (requester, accepter) = self.participants();
        FriendshipFactKey {
            sub_type: "friendship-edge",
            data: stable_authority_pair_bytes(requester, accepter),
        }
    }
}

pub struct FriendshipFactReducer;

impl FactReducer for FriendshipFactReducer {
    fn handles_type(&self) -> &'static str {
        FRIENDSHIP_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        reduce_typed_envelope::<FriendshipFact>(
            context_id,
            envelope,
            FRIENDSHIP_FACT_TYPE_ID,
            |fact| fact.context_id() == context_id,
            |fact| {
                let key = fact.binding_key();
                RelationalBinding {
                    binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
                    context_id,
                    data: key.data,
                }
            },
        )
    }
}

/// Bounded introduction artifact for an introduced FoF candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(
    type_id = "trust_introduction",
    schema_version = 2,
    context = "context_id"
)]
pub enum TrustIntroductionFact {
    Issued {
        context_id: ContextId,
        introducer: AuthorityId,
        introduced_authority: AuthorityId,
        issued_at: PhysicalTime,
        expires_at: PhysicalTime,
        remaining_depth: u8,
        max_fanout: u8,
        /// Stamped by the writer; see [`trust_introduction_causal`].
        causal: CausalMetadata,
    },
    Revoked {
        context_id: ContextId,
        introducer: AuthorityId,
        introduced_authority: AuthorityId,
        revoked_at: PhysicalTime,
        /// Revokes the observed issuances of the same introduction.
        causal: CausalMetadata,
    },
}

impl TrustIntroductionFact {
    /// `(introducer, introduced_authority)`: the key of one introduction.
    pub fn introduction(&self) -> (AuthorityId, AuthorityId) {
        match self {
            Self::Issued {
                introducer,
                introduced_authority,
                ..
            }
            | Self::Revoked {
                introducer,
                introduced_authority,
                ..
            } => (*introducer, *introduced_authority),
        }
    }

    /// The writer's causal metadata.
    pub fn causal(&self) -> &CausalMetadata {
        match self {
            Self::Issued { causal, .. } | Self::Revoked { causal, .. } => causal,
        }
    }

    pub fn binding_key(&self) -> FriendshipFactKey {
        let (introducer, introduced_authority) = self.introduction();
        let mut data = introducer.to_bytes().to_vec();
        data.extend_from_slice(&introduced_authority.to_bytes());
        FriendshipFactKey {
            sub_type: "trust-introduction",
            data,
        }
    }

    /// Convert an issued bounded introduction into a runtime-consumable
    /// bootstrap hint without promoting it into canonical topology state.
    pub fn bootstrap_hint(
        &self,
        introduced_device: Option<DeviceId>,
        link_endpoints: Vec<LinkEndpoint>,
        replay_window_id: [u8; 32],
    ) -> Option<BootstrapIntroductionHint> {
        match self {
            Self::Issued {
                context_id,
                introducer,
                introduced_authority,
                expires_at,
                remaining_depth,
                max_fanout,
                ..
            } => Some(BootstrapIntroductionHint {
                scope: *context_id,
                introducer_authority: *introducer,
                introduced_authority: *introduced_authority,
                introduced_device,
                link_endpoints,
                route_layer_public_key: None,
                remaining_depth: *remaining_depth,
                max_fanout: *max_fanout,
                valid_until: expires_at.ts_ms,
                replay_window_id,
            }),
            Self::Revoked { .. } => None,
        }
    }
}

pub struct TrustIntroductionFactReducer;

impl FactReducer for TrustIntroductionFactReducer {
    fn handles_type(&self) -> &'static str {
        TRUST_INTRODUCTION_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &aura_core::types::facts::FactEnvelope,
    ) -> Option<RelationalBinding> {
        reduce_typed_envelope::<TrustIntroductionFact>(
            context_id,
            envelope,
            TRUST_INTRODUCTION_FACT_TYPE_ID,
            |fact| fact.context_id() == context_id,
            |fact| {
                let key = fact.binding_key();
                RelationalBinding {
                    binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
                    context_id,
                    data: key.data,
                }
            },
        )
    }
}

/// A causally stamped web-of-trust fact.
pub trait WotFact: DomainFact + Clone {
    /// Fact type id the content tag is derived under.
    const TYPE_ID: &'static str;
    /// The writer's causal metadata.
    fn causal_metadata(&self) -> &CausalMetadata;
}

impl WotFact for FriendshipFact {
    const TYPE_ID: &'static str = FRIENDSHIP_FACT_TYPE_ID;
    fn causal_metadata(&self) -> &CausalMetadata {
        self.causal()
    }
}

impl WotFact for TrustIntroductionFact {
    const TYPE_ID: &'static str = TRUST_INTRODUCTION_FACT_TYPE_ID;
    fn causal_metadata(&self) -> &CausalMetadata {
        self.causal()
    }
}

/// A web-of-trust fact with its content-derived tag (never read from the wire).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedWotFact<F> {
    tag: CausalTag,
    fact: F,
}

impl<F: WotFact> TaggedWotFact<F> {
    /// Wrap a decoded fact, tagging it by type id and canonical encoding.
    #[must_use]
    pub fn new(fact: F) -> Self {
        Self {
            tag: CausalTag::from_content(F::TYPE_ID, &fact.to_envelope().payload),
            fact,
        }
    }

    /// The decoded fact.
    #[must_use]
    pub fn fact(&self) -> &F {
        &self.fact
    }
}

impl<F: WotFact> CausalFact for TaggedWotFact<F> {
    fn causal_tag(&self) -> CausalTag {
        self.tag
    }

    fn causal_metadata(&self) -> &CausalMetadata {
        self.fact.causal_metadata()
    }
}

pub type TaggedFriendshipFact = TaggedWotFact<FriendshipFact>;
pub type TaggedTrustIntroductionFact = TaggedWotFact<TrustIntroductionFact>;

fn observed_tags<F: WotFact>(
    observed: &[TaggedWotFact<F>],
    matches: impl Fn(&F) -> bool,
) -> Vec<CausalTag> {
    observed
        .iter()
        .filter(|fact| matches(&fact.fact))
        .map(|fact| fact.tag)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// What a new friendship fact is; determines what it supersedes or revokes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FriendshipCausalKey {
    /// `FriendshipFact::Proposed`.
    Propose { a: AuthorityId, b: AuthorityId },
    /// `FriendshipFact::Accepted`.
    Accept { a: AuthorityId, b: AuthorityId },
    /// `FriendshipFact::Revoked`.
    Revoke { a: AuthorityId, b: AuthorityId },
}

impl FriendshipCausalKey {
    /// The friendship the new fact is about.
    #[must_use]
    pub fn pair(&self) -> FriendshipPair {
        match *self {
            Self::Propose { a, b } | Self::Accept { a, b } | Self::Revoke { a, b } => {
                unordered_pair(a, b)
            }
        }
    }
}

/// Causal metadata for a new friendship fact about `key`, written by a
/// writer holding `observed` at its freshly advanced logical `clock`. An
/// acceptance supersedes the observed proposals; a revocation revokes the
/// observed proposals and acceptances.
#[must_use]
pub fn friendship_causal(
    key: FriendshipCausalKey,
    observed: &[TaggedFriendshipFact],
    clock: &LogicalTime,
) -> CausalMetadata {
    let pair = key.pair();
    let (revokes, supersedes) = match key {
        FriendshipCausalKey::Propose { .. } => (Vec::new(), Vec::new()),
        FriendshipCausalKey::Accept { .. } => (
            Vec::new(),
            observed_tags(observed, |fact| {
                fact.pair() == pair && matches!(fact, FriendshipFact::Proposed { .. })
            }),
        ),
        FriendshipCausalKey::Revoke { .. } => (
            observed_tags(observed, |fact| {
                fact.pair() == pair && !matches!(fact, FriendshipFact::Revoked { .. })
            }),
            Vec::new(),
        ),
    };
    CausalMetadata {
        revokes,
        supersedes,
        clock: CausalClock::from_logical(clock),
    }
}

/// Causal metadata for a new introduction fact about `(introducer,
/// introduced)`: a revocation (`revoke`) revokes the observed issuances.
#[must_use]
pub fn trust_introduction_causal(
    introduction: (AuthorityId, AuthorityId),
    revoke: bool,
    observed: &[TaggedTrustIntroductionFact],
    clock: &LogicalTime,
) -> CausalMetadata {
    let revokes = if revoke {
        observed_tags(observed, |fact| {
            fact.introduction() == introduction
                && matches!(fact, TrustIntroductionFact::Issued { .. })
        })
    } else {
        Vec::new()
    };
    CausalMetadata {
        revokes,
        supersedes: Vec::new(),
        clock: CausalClock::from_logical(clock),
    }
}

/// Reduced status of one friendship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FriendshipStatus {
    /// A live, unconsumed proposal by `requester`.
    Pending { requester: AuthorityId },
    /// A live acceptance in `context_id`.
    Friends { context_id: ContextId },
}

/// Live adds (unrevoked) of a fact set, sorted by causal order.
fn live_adds<'a, F: WotFact>(
    facts: &[&'a TaggedWotFact<F>],
    is_add: impl Fn(&F) -> bool,
) -> Vec<&'a TaggedWotFact<F>> {
    let adds: Vec<_> = facts
        .iter()
        .filter(|fact| is_add(&fact.fact))
        .map(|fact| ((), fact.tag))
        .collect();
    let reversals: Vec<_> = facts
        .iter()
        .filter(|fact| !is_add(&fact.fact))
        .map(|fact| ((), fact.fact.causal_metadata().revokes.as_slice()))
        .collect();
    let live = observed_remove_live(&adds, &reversals);
    let mut kept: Vec<_> = facts
        .iter()
        .copied()
        .filter(|fact| live.contains(&fact.tag))
        .collect();
    kept.sort_by(|a, b| causal_cmp(*a, *b));
    kept
}

fn reduce_friendship(facts: &[&TaggedFriendshipFact]) -> Option<FriendshipStatus> {
    let consumed: BTreeSet<CausalTag> = facts
        .iter()
        .filter(|fact| matches!(fact.fact, FriendshipFact::Accepted { .. }))
        .flat_map(|fact| fact.fact.causal().supersedes.iter().copied())
        .collect();
    let live = live_adds(facts, |fact| {
        !matches!(fact, FriendshipFact::Revoked { .. })
    });
    if let Some(accepted) = live
        .iter()
        .rev()
        .find(|fact| matches!(fact.fact, FriendshipFact::Accepted { .. }))
    {
        return Some(FriendshipStatus::Friends {
            context_id: accepted.fact.context_id(),
        });
    }
    live.iter()
        .rev()
        .find(|fact| !consumed.contains(&fact.tag))
        .map(|fact| FriendshipStatus::Pending {
            requester: fact.fact.participants().0,
        })
}

/// The friendship fact set. Reduction is a deterministic function of the
/// set: independent of arrival order and duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FriendshipLog {
    facts: BTreeMap<FriendshipPair, BTreeMap<CausalTag, TaggedFriendshipFact>>,
}

impl FriendshipLog {
    /// Add a fact to the set; returns the friendship it is about.
    pub fn insert(&mut self, fact: FriendshipFact) -> FriendshipPair {
        let tagged = TaggedWotFact::new(fact);
        let pair = tagged.fact.pair();
        self.facts
            .entry(pair)
            .or_default()
            .insert(tagged.tag, tagged);
        pair
    }

    /// Every fact held for the friendship of `a` and `b`.
    pub fn facts_for(&self, a: AuthorityId, b: AuthorityId) -> Vec<TaggedFriendshipFact> {
        self.facts
            .get(&unordered_pair(a, b))
            .map(|facts| facts.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Reduced status of the friendship of `a` and `b`.
    pub fn status(&self, a: AuthorityId, b: AuthorityId) -> Option<FriendshipStatus> {
        let facts: Vec<_> = self.facts.get(&unordered_pair(a, b))?.values().collect();
        reduce_friendship(&facts)
    }

    /// `local_authority`'s friendships.
    pub fn state(&self, local_authority: AuthorityId) -> FriendshipState {
        let mut state = FriendshipState::default();
        for &(a, b) in self.facts.keys() {
            let other = match (a == local_authority, b == local_authority) {
                (true, _) => b,
                (_, true) => a,
                _ => continue,
            };
            match self.status(a, b) {
                Some(FriendshipStatus::Friends { context_id }) => {
                    state.direct_friends.insert(other, context_id);
                }
                Some(FriendshipStatus::Pending { requester }) if requester == local_authority => {
                    state.pending_outbound.insert(other);
                }
                Some(FriendshipStatus::Pending { .. }) => {
                    state.pending_inbound.insert(other);
                }
                None => {}
            }
        }
        state
    }
}

/// Local friendship state reduced from the friendship fact set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FriendshipState {
    pub pending_outbound: BTreeSet<AuthorityId>,
    pub pending_inbound: BTreeSet<AuthorityId>,
    pub direct_friends: BTreeMap<AuthorityId, ContextId>,
}

/// Runtime-consumable WoT evidence record derived from relational facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebOfTrustEvidence {
    pub authority_id: AuthorityId,
    pub evidence: ProviderEvidence,
    pub context_id: ContextId,
    pub introduced_by: Option<AuthorityId>,
    pub expires_at: Option<PhysicalTime>,
    pub remaining_depth: u8,
    pub max_fanout: u8,
}

/// Pure WoT derivation index over the friendship and introduction fact sets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebOfTrustIndex {
    friendships: FriendshipLog,
    introductions:
        BTreeMap<(AuthorityId, AuthorityId), BTreeMap<CausalTag, TaggedTrustIntroductionFact>>,
}

impl WebOfTrustIndex {
    /// `local_authority`'s reduced friendships.
    pub fn friendship_state(&self, local_authority: AuthorityId) -> FriendshipState {
        self.friendships.state(local_authority)
    }

    pub fn apply_friendship_fact(&mut self, fact: &FriendshipFact) {
        self.friendships.insert(fact.clone());
    }

    pub fn apply_introduction_fact(&mut self, fact: &TrustIntroductionFact) {
        let tagged = TaggedWotFact::new(fact.clone());
        self.introductions
            .entry(fact.introduction())
            .or_default()
            .insert(tagged.tag, tagged);
    }

    /// Provider evidence for `local_authority`, with introduction validity
    /// evaluated at the caller-supplied instant `valid_at_ms`.
    pub fn provider_evidence(
        &self,
        local_authority: AuthorityId,
        valid_at_ms: u64,
    ) -> Vec<WebOfTrustEvidence> {
        let friends = self.friendships.state(local_authority).direct_friends;
        let mut output: Vec<_> = friends
            .iter()
            .map(|(authority_id, context_id)| WebOfTrustEvidence {
                authority_id: *authority_id,
                evidence: ProviderEvidence::DirectFriend,
                context_id: *context_id,
                introduced_by: None,
                expires_at: None,
                remaining_depth: 0,
                max_fanout: 0,
            })
            .collect();

        for ((introducer, _), facts) in &self.introductions {
            if !friends.contains_key(introducer) {
                continue;
            }
            let facts: Vec<_> = facts.values().collect();
            let live = live_adds(&facts, |fact| {
                matches!(fact, TrustIntroductionFact::Issued { .. })
            });
            output.extend(live.iter().rev().find_map(|fact| match &fact.fact {
                TrustIntroductionFact::Issued {
                    context_id,
                    introducer,
                    introduced_authority,
                    expires_at,
                    remaining_depth,
                    max_fanout,
                    ..
                } if *remaining_depth > 0 && *max_fanout > 0 && expires_at.ts_ms > valid_at_ms => {
                    Some(WebOfTrustEvidence {
                        authority_id: *introduced_authority,
                        evidence: ProviderEvidence::IntroducedFof,
                        context_id: *context_id,
                        introduced_by: Some(*introducer),
                        expires_at: Some(expires_at.clone()),
                        remaining_depth: *remaining_depth,
                        max_fanout: *max_fanout,
                    })
                }
                _ => None,
            }));
        }
        output
    }
}

/// Deterministic stamping for tests and mocks.
pub mod test_support {
    use super::*;
    use crate::contacts::test_support::advance;

    /// Causal metadata for friendship `key` written by `device` after
    /// observing `observed`.
    #[must_use]
    pub fn friendship_causal_after(
        device: u8,
        key: FriendshipCausalKey,
        observed: &[TaggedFriendshipFact],
    ) -> CausalMetadata {
        friendship_causal(key, observed, &advance(device, observed))
    }

    /// Causal metadata for an introduction fact written by `device` after
    /// observing `observed`.
    #[must_use]
    pub fn trust_introduction_causal_after(
        device: u8,
        introduction: (AuthorityId, AuthorityId),
        revoke: bool,
        observed: &[TaggedTrustIntroductionFact],
    ) -> CausalMetadata {
        trust_introduction_causal(introduction, revoke, observed, &advance(device, observed))
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{friendship_causal_after, trust_introduction_causal_after};
    use super::*;
    use aura_journal::causal_reduction::assert_permutation_invariant;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn time(ms: u64) -> PhysicalTime {
        crate::reducer_support::physical_time_ms(ms)
    }

    #[derive(Clone, Copy)]
    enum Step {
        Propose,
        Accept,
        Revoke,
    }

    /// A friendship fact of `step` between `requester` and `accepter`,
    /// written by `device` after observing `observed`.
    fn friendship(
        device: u8,
        step: Step,
        requester: AuthorityId,
        accepter: AuthorityId,
        observed: &[&FriendshipFact],
    ) -> FriendshipFact {
        let observed: Vec<_> = observed
            .iter()
            .map(|fact| TaggedWotFact::new((*fact).clone()))
            .collect();
        let (a, b) = (requester, accepter);
        let key = match step {
            Step::Propose => FriendshipCausalKey::Propose { a, b },
            Step::Accept => FriendshipCausalKey::Accept { a, b },
            Step::Revoke => FriendshipCausalKey::Revoke { a, b },
        };
        let causal = friendship_causal_after(device, key, &observed);
        let context_id = context(9);
        match step {
            Step::Propose => FriendshipFact::Proposed {
                context_id,
                requester,
                accepter,
                proposed_at: time(10),
                causal,
            },
            Step::Accept => FriendshipFact::Accepted {
                context_id,
                requester,
                accepter,
                accepted_at: time(20),
                causal,
            },
            Step::Revoke => FriendshipFact::Revoked {
                context_id,
                requester,
                accepter,
                revoked_at: time(30),
                causal,
            },
        }
    }

    fn status_of(facts: &[FriendshipFact]) -> Option<FriendshipStatus> {
        let mut log = FriendshipLog::default();
        for fact in facts {
            log.insert(fact.clone());
        }
        log.status(authority(1), authority(2))
    }

    #[test]
    fn late_proposal_after_acceptance_keeps_the_friendship() {
        let (local, peer) = (authority(1), authority(2));
        let proposed = friendship(1, Step::Propose, local, peer, &[]);
        let accepted = friendship(2, Step::Accept, local, peer, &[&proposed]);
        // Every order, including the proposal arriving after the acceptance.
        let status = assert_permutation_invariant(&[proposed, accepted], status_of);
        assert_eq!(
            status,
            Some(FriendshipStatus::Friends {
                context_id: context(9)
            })
        );
    }

    #[test]
    fn revocation_concurrent_with_acceptance_is_order_independent() {
        let (local, peer) = (authority(1), authority(2));
        let proposed = friendship(1, Step::Propose, local, peer, &[]);
        // The requester cancels while the peer concurrently accepts: the
        // revocation never observed the acceptance, so the acceptance survives.
        let revoked = friendship(1, Step::Revoke, local, peer, &[&proposed]);
        let accepted = friendship(2, Step::Accept, local, peer, &[&proposed]);
        let status = assert_permutation_invariant(&[proposed, revoked, accepted], status_of);
        assert_eq!(
            status,
            Some(FriendshipStatus::Friends {
                context_id: context(9)
            })
        );
    }

    #[test]
    fn revocation_ends_the_observed_episode_and_reproposal_starts_a_new_one() {
        let (local, peer) = (authority(1), authority(2));
        let proposed = friendship(1, Step::Propose, local, peer, &[]);
        let accepted = friendship(2, Step::Accept, local, peer, &[&proposed]);
        let revoked = friendship(1, Step::Revoke, local, peer, &[&proposed, &accepted]);
        let ended = assert_permutation_invariant(
            &[proposed.clone(), accepted.clone(), revoked.clone()],
            status_of,
        );
        assert_eq!(ended, None);

        let reproposed = friendship(
            2,
            Step::Propose,
            peer,
            local,
            &[&proposed, &accepted, &revoked],
        );
        let status =
            assert_permutation_invariant(&[proposed, accepted, revoked, reproposed], status_of);
        assert_eq!(status, Some(FriendshipStatus::Pending { requester: peer }));
    }

    #[test]
    fn friendship_state_reports_direction_for_the_local_authority() {
        let (local, peer) = (authority(1), authority(2));
        let mut log = FriendshipLog::default();
        log.insert(friendship(1, Step::Propose, local, peer, &[]));
        assert!(log.state(local).pending_outbound.contains(&peer));
        assert!(log.state(peer).pending_inbound.contains(&local));
    }

    fn issued(
        introducer: AuthorityId,
        introduced: AuthorityId,
        expires_ms: u64,
        observed: &[TaggedTrustIntroductionFact],
    ) -> TrustIntroductionFact {
        TrustIntroductionFact::Issued {
            context_id: context(4),
            introducer,
            introduced_authority: introduced,
            issued_at: time(20),
            expires_at: time(expires_ms),
            remaining_depth: 1,
            max_fanout: 2,
            causal: trust_introduction_causal_after(3, (introducer, introduced), false, observed),
        }
    }

    fn friends_with(local: AuthorityId, friend: AuthorityId) -> Vec<FriendshipFact> {
        let proposed = friendship(1, Step::Propose, local, friend, &[]);
        let accepted = friendship(2, Step::Accept, local, friend, &[&proposed]);
        vec![proposed, accepted]
    }

    #[test]
    fn wot_index_enforces_intro_validity_depth_and_fanout() {
        let (local, friend, fof) = (authority(1), authority(2), authority(3));
        let mut index = WebOfTrustIndex::default();
        for fact in friends_with(local, friend) {
            index.apply_friendship_fact(&fact);
        }
        index.apply_introduction_fact(&issued(friend, fof, 100, &[]));

        let evidence = index.provider_evidence(local, 50);
        assert!(evidence.iter().any(|entry| {
            entry.authority_id == friend && entry.evidence == ProviderEvidence::DirectFriend
        }));
        assert!(evidence.iter().any(|entry| {
            entry.authority_id == fof && entry.evidence == ProviderEvidence::IntroducedFof
        }));

        let expired = index.provider_evidence(local, 150);
        assert!(!expired.iter().any(|entry| entry.authority_id == fof));
    }

    #[test]
    fn introduction_revocation_is_order_independent_and_reissue_survives() {
        let (local, friend, fof) = (authority(1), authority(2), authority(3));
        let first = issued(friend, fof, 100, &[]);
        let tagged_first = vec![TaggedWotFact::new(first.clone())];
        let revoked = TrustIntroductionFact::Revoked {
            context_id: context(4),
            introducer: friend,
            introduced_authority: fof,
            revoked_at: time(30),
            causal: trust_introduction_causal_after(3, (friend, fof), true, &tagged_first),
        };
        let reduce = |facts: &[TrustIntroductionFact]| {
            let mut index = WebOfTrustIndex::default();
            for fact in friends_with(local, friend) {
                index.apply_friendship_fact(&fact);
            }
            for fact in facts {
                index.apply_introduction_fact(fact);
            }
            index
                .provider_evidence(local, 50)
                .iter()
                .any(|entry| entry.authority_id == fof)
        };
        assert!(!assert_permutation_invariant(
            &[first.clone(), revoked.clone()],
            reduce
        ));

        let mut observed = tagged_first;
        observed.push(TaggedWotFact::new(revoked.clone()));
        let reissued = issued(friend, fof, 120, &observed);
        assert!(assert_permutation_invariant(
            &[first, revoked, reissued],
            reduce
        ));
    }

    #[test]
    fn issued_introduction_converts_to_bounded_bootstrap_hint() {
        let mut fact = issued(authority(2), authority(3), 120, &[]);
        if let TrustIntroductionFact::Issued {
            remaining_depth,
            max_fanout,
            ..
        } = &mut fact
        {
            *remaining_depth = 2;
            *max_fanout = 3;
        }

        let hint = fact
            .bootstrap_hint(
                Some(DeviceId::from_bytes([7u8; 32])),
                vec![LinkEndpoint::direct(
                    aura_core::service::LinkProtocol::Tcp,
                    "127.0.0.1:7551",
                )],
                [9u8; 32],
            )
            .unwrap_or_else(|| panic!("issued introduction should produce bootstrap hint"));

        assert_eq!(hint.scope, context(4));
        assert_eq!(hint.introducer_authority, authority(2));
        assert_eq!(hint.introduced_authority, authority(3));
        assert_eq!(hint.remaining_depth, 2);
        assert_eq!(hint.max_fanout, 3);
        assert_eq!(hint.valid_until, 120);
        assert_eq!(hint.replay_window_id, [9u8; 32]);
    }

    #[test]
    fn revoked_introduction_does_not_produce_bootstrap_hint() {
        let fact = TrustIntroductionFact::Revoked {
            context_id: context(4),
            introducer: authority(2),
            introduced_authority: authority(3),
            revoked_at: time(20),
            causal: trust_introduction_causal_after(3, (authority(2), authority(3)), true, &[]),
        };

        assert!(fact
            .bootstrap_hint(
                None,
                vec![LinkEndpoint::direct(
                    aura_core::service::LinkProtocol::Tcp,
                    "127.0.0.1:7552"
                )],
                [10u8; 32],
            )
            .is_none());
    }
}
