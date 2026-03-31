//! Web-of-trust facts and pure derivation helpers.
//!
//! Direct friend relationships are modeled as bilateral relational-context
//! facts. Introductions are bounded artifacts with explicit expiry, depth, and
//! fan-out metadata. Runtime selection stays out of this crate; this module only
//! exposes pure evidence derivation.

use aura_core::service::ProviderEvidence;
use aura_core::time::PhysicalTime;
use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const FRIENDSHIP_FACT_TYPE_ID: &str = "friendship";
pub const TRUST_INTRODUCTION_FACT_TYPE_ID: &str = "trust_introduction";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FriendshipFactKey {
    pub sub_type: &'static str,
    pub data: Vec<u8>,
}

/// Bilateral friendship lifecycle facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(type_id = "friendship", schema_version = 1, context = "context_id")]
pub enum FriendshipFact {
    Proposed {
        context_id: ContextId,
        requester: AuthorityId,
        accepter: AuthorityId,
        proposed_at: PhysicalTime,
    },
    Accepted {
        context_id: ContextId,
        requester: AuthorityId,
        accepter: AuthorityId,
        accepted_at: PhysicalTime,
    },
    Revoked {
        context_id: ContextId,
        requester: AuthorityId,
        accepter: AuthorityId,
        revoked_at: PhysicalTime,
    },
}

impl FriendshipFact {
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
        let mut a = requester.to_bytes();
        let mut b = accepter.to_bytes();
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        let mut data = a.to_vec();
        data.extend_from_slice(&b);
        FriendshipFactKey {
            sub_type: "friendship-edge",
            data,
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
        if envelope.type_id.as_str() != FRIENDSHIP_FACT_TYPE_ID {
            return None;
        }
        let fact = FriendshipFact::from_envelope(envelope)?;
        if fact.context_id() != context_id {
            return None;
        }
        let key = fact.binding_key();
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
        })
    }
}

/// Bounded introduction artifact for an introduced FoF candidate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(
    type_id = "trust_introduction",
    schema_version = 1,
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
    },
    Revoked {
        context_id: ContextId,
        introducer: AuthorityId,
        introduced_authority: AuthorityId,
        revoked_at: PhysicalTime,
    },
}

impl TrustIntroductionFact {
    pub fn binding_key(&self) -> FriendshipFactKey {
        let (introducer, introduced_authority) = match self {
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
        };
        let mut data = introducer.to_bytes().to_vec();
        data.extend_from_slice(&introduced_authority.to_bytes());
        FriendshipFactKey {
            sub_type: "trust-introduction",
            data,
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
        if envelope.type_id.as_str() != TRUST_INTRODUCTION_FACT_TYPE_ID {
            return None;
        }
        let fact = TrustIntroductionFact::from_envelope(envelope)?;
        if fact.context_id() != context_id {
            return None;
        }
        let key = fact.binding_key();
        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
        })
    }
}

/// Local friendship state derived from bilateral facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FriendshipState {
    pub pending_outbound: BTreeSet<AuthorityId>,
    pub pending_inbound: BTreeSet<AuthorityId>,
    pub direct_friends: BTreeMap<AuthorityId, ContextId>,
}

impl FriendshipState {
    pub fn apply_fact(&mut self, local_authority: AuthorityId, fact: &FriendshipFact) {
        let Some(other) = fact.other_participant(local_authority) else {
            return;
        };

        match fact {
            FriendshipFact::Proposed { requester, .. } => {
                if *requester == local_authority {
                    self.pending_outbound.insert(other);
                } else {
                    self.pending_inbound.insert(other);
                }
                self.direct_friends.remove(&other);
            }
            FriendshipFact::Accepted { .. } => {
                self.pending_outbound.remove(&other);
                self.pending_inbound.remove(&other);
                self.direct_friends.insert(other, fact.context_id());
            }
            FriendshipFact::Revoked { .. } => {
                self.pending_outbound.remove(&other);
                self.pending_inbound.remove(&other);
                self.direct_friends.remove(&other);
            }
        }
    }
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

/// Pure WoT derivation index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebOfTrustIndex {
    friendship: FriendshipState,
    introductions: BTreeMap<AuthorityId, WebOfTrustEvidence>,
}

impl WebOfTrustIndex {
    pub fn friendship_state(&self) -> &FriendshipState {
        &self.friendship
    }

    pub fn apply_friendship_fact(&mut self, local_authority: AuthorityId, fact: &FriendshipFact) {
        self.friendship.apply_fact(local_authority, fact);
    }

    pub fn apply_introduction_fact(&mut self, fact: &TrustIntroductionFact) {
        match fact {
            TrustIntroductionFact::Issued {
                context_id,
                introducer,
                introduced_authority,
                expires_at,
                remaining_depth,
                max_fanout,
                ..
            } => {
                self.introductions.insert(
                    *introduced_authority,
                    WebOfTrustEvidence {
                        authority_id: *introduced_authority,
                        evidence: ProviderEvidence::IntroducedFof,
                        context_id: *context_id,
                        introduced_by: Some(*introducer),
                        expires_at: Some(expires_at.clone()),
                        remaining_depth: *remaining_depth,
                        max_fanout: *max_fanout,
                    },
                );
                // Keep the key/value associated with the introduced authority;
                // introducer validation happens when evidence is queried.
                let _ = introducer;
            }
            TrustIntroductionFact::Revoked {
                introduced_authority,
                ..
            } => {
                self.introductions.remove(introduced_authority);
            }
        }
    }

    pub fn provider_evidence(&self, now_ms: u64) -> Vec<WebOfTrustEvidence> {
        let mut output = Vec::new();

        output.extend(
            self.friendship
                .direct_friends
                .iter()
                .map(|(authority_id, context_id)| WebOfTrustEvidence {
                    authority_id: *authority_id,
                    evidence: ProviderEvidence::DirectFriend,
                    context_id: *context_id,
                    introduced_by: None,
                    expires_at: None,
                    remaining_depth: 0,
                    max_fanout: 0,
                }),
        );

        output.extend(
            self.introductions
                .values()
                .filter(|evidence| {
                    evidence.remaining_depth > 0
                        && evidence.max_fanout > 0
                        && evidence.introduced_by.is_some_and(|introducer| {
                            self.friendship.direct_friends.contains_key(&introducer)
                        })
                        && evidence
                            .expires_at
                            .as_ref()
                            .map_or(true, |expires_at| expires_at.ts_ms > now_ms)
                })
                .cloned(),
        );

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authority(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    fn context(seed: u8) -> ContextId {
        ContextId::new_from_entropy([seed; 32])
    }

    fn time(ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms: ms,
            uncertainty: None,
        }
    }

    #[test]
    fn friendship_state_tracks_pending_and_bilateral_acceptance() {
        let local = authority(1);
        let peer = authority(2);
        let ctx = context(3);
        let mut state = FriendshipState::default();

        state.apply_fact(
            local,
            &FriendshipFact::Proposed {
                context_id: ctx,
                requester: local,
                accepter: peer,
                proposed_at: time(10),
            },
        );
        assert!(state.pending_outbound.contains(&peer));

        state.apply_fact(
            local,
            &FriendshipFact::Accepted {
                context_id: ctx,
                requester: local,
                accepter: peer,
                accepted_at: time(20),
            },
        );
        assert!(state.direct_friends.contains_key(&peer));
        assert!(state.pending_outbound.is_empty());

        state.apply_fact(
            local,
            &FriendshipFact::Revoked {
                context_id: ctx,
                requester: local,
                accepter: peer,
                revoked_at: time(30),
            },
        );
        assert!(!state.direct_friends.contains_key(&peer));
    }

    #[test]
    fn wot_index_enforces_intro_expiry_depth_and_fanout() {
        let local = authority(1);
        let friend = authority(2);
        let fof = authority(3);
        let ctx = context(4);
        let mut index = WebOfTrustIndex::default();

        index.apply_friendship_fact(
            local,
            &FriendshipFact::Accepted {
                context_id: ctx,
                requester: local,
                accepter: friend,
                accepted_at: time(10),
            },
        );
        index.apply_introduction_fact(&TrustIntroductionFact::Issued {
            context_id: ctx,
            introducer: friend,
            introduced_authority: fof,
            issued_at: time(20),
            expires_at: time(100),
            remaining_depth: 1,
            max_fanout: 2,
        });

        let evidence = index.provider_evidence(50);
        assert!(evidence.iter().any(|entry| {
            entry.authority_id == friend && entry.evidence == ProviderEvidence::DirectFriend
        }));
        assert!(evidence.iter().any(|entry| {
            entry.authority_id == fof && entry.evidence == ProviderEvidence::IntroducedFof
        }));

        let expired = index.provider_evidence(150);
        assert!(!expired.iter().any(|entry| entry.authority_id == fof));
    }

    #[test]
    fn revocation_removes_introduction_evidence() {
        let local = authority(1);
        let friend = authority(2);
        let fof = authority(3);
        let ctx = context(4);
        let mut index = WebOfTrustIndex::default();

        index.apply_friendship_fact(
            local,
            &FriendshipFact::Accepted {
                context_id: ctx,
                requester: local,
                accepter: friend,
                accepted_at: time(10),
            },
        );
        index.apply_introduction_fact(&TrustIntroductionFact::Issued {
            context_id: ctx,
            introducer: friend,
            introduced_authority: fof,
            issued_at: time(20),
            expires_at: time(100),
            remaining_depth: 1,
            max_fanout: 1,
        });
        index.apply_introduction_fact(&TrustIntroductionFact::Revoked {
            context_id: ctx,
            introducer: friend,
            introduced_authority: fof,
            revoked_at: time(30),
        });

        let evidence = index.provider_evidence(50);
        assert!(!evidence.iter().any(|entry| entry.authority_id == fof));
    }
}
