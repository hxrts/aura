//! Invitation View Delta and Reducer
//!
//! This module provides view-level reduction for invitation facts, transforming
//! journal facts into UI-level deltas for invitation views.
//!
//! # Architecture
//!
//! View reduction is separate from journal-level reduction:
//! - **Journal reduction** (`InvitationFactReducer`): Facts → `RelationalBinding` for storage
//! - **View reduction** (this module): Facts → `InvitationDelta` for UI updates
//!
//! # Usage
//!
//! Register the reducer with the runtime's `ViewDeltaRegistry`:
//!
//! ```ignore
//! use aura_invitation::{InvitationViewReducer, INVITATION_FACT_TYPE_ID};
//! use aura_composition::ViewDeltaRegistry;
//!
//! let mut registry = ViewDeltaRegistry::new();
//! registry.register(INVITATION_FACT_TYPE_ID, Box::new(InvitationViewReducer));
//! ```

use aura_composition::{ComposableDelta, IntoViewDelta, ViewDelta, ViewDeltaReducer};
use aura_core::threshold::AgreementMode;
use aura_core::types::identifiers::{AuthorityId, CeremonyId, InvitationId};
use aura_journal::DomainFact;

use crate::{
    facts::CeremonyRelationshipId,
    lifecycle::{ceremony_status_cmp, InvitationOutcome, InvitationOutcomes},
    InvitationFact, INVITATION_FACT_TYPE_ID,
};
use aura_core::time::PhysicalTime;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvitationDirection {
    Inbound,
    Outbound,
    Observed,
}

impl InvitationDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            InvitationDirection::Inbound => "inbound",
            InvitationDirection::Outbound => "outbound",
            InvitationDirection::Observed => "observed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeremonyViewStatus {
    Initiated,
    AcceptanceReceived,
    Committed,
    Aborted,
    Superseded,
}

impl CeremonyViewStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CeremonyViewStatus::Initiated => "initiated",
            CeremonyViewStatus::AcceptanceReceived => "acceptance_received",
            CeremonyViewStatus::Committed => "committed",
            CeremonyViewStatus::Aborted => "aborted",
            CeremonyViewStatus::Superseded => "superseded",
        }
    }
}

/// Delta type for invitation view updates.
///
/// These deltas represent incremental changes to invitation UI state,
/// derived from journal facts during view reduction.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)] // InvitationAdded variant contains rich invitation data
pub enum InvitationDelta {
    /// A new invitation was created or received
    InvitationAdded {
        invitation_id: InvitationId,
        direction: InvitationDirection,
        other_party_id: AuthorityId,
        other_party_name: String,
        /// Type: "guardian", "channel", "contact", "device"
        invitation_type: crate::InvitationType,
        created_at: u64,
        expires_at: Option<u64>,
        message: Option<String>,
    },
    /// Invitation outcomes observed. Merging unions the outcome sets, so the
    /// resolved status (`InvitationOutcomes::status`) is order-independent.
    InvitationStatusChanged {
        invitation_id: InvitationId,
        outcomes: InvitationOutcomes,
    },
    /// Ceremony status changed
    CeremonyStatusChanged {
        ceremony_id: CeremonyId,
        status: CeremonyViewStatus,
        /// For "aborted" status, the reason
        reason: Option<String>,
        /// For "committed" status, the resulting relationship ID
        relationship_id: Option<CeremonyRelationshipId>,
        /// Agreement mode (A1/A2/A3) if available
        agreement_mode: Option<AgreementMode>,
        /// Whether reversion is still possible
        reversion_risk: bool,
        /// Physical time of the winning stage, for display only
        observed_at: PhysicalTime,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InvitationDeltaKey {
    Invitation(InvitationId),
    Ceremony(CeremonyId),
}

impl ComposableDelta for InvitationDelta {
    type Key = InvitationDeltaKey;

    fn key(&self) -> Self::Key {
        match self {
            InvitationDelta::InvitationAdded { invitation_id, .. }
            | InvitationDelta::InvitationStatusChanged { invitation_id, .. } => {
                InvitationDeltaKey::Invitation(invitation_id.clone())
            }
            InvitationDelta::CeremonyStatusChanged { ceremony_id, .. } => {
                InvitationDeltaKey::Ceremony(ceremony_id.clone())
            }
        }
    }

    fn try_merge(&mut self, other: Self) -> bool {
        match (self, other) {
            (
                InvitationDelta::InvitationAdded {
                    created_at,
                    invitation_id: id,
                    direction: dir,
                    other_party_id: other_id,
                    other_party_name: other_name,
                    invitation_type: inv_type,
                    expires_at: exp,
                    message: msg,
                },
                InvitationDelta::InvitationAdded {
                    created_at: other_ts,
                    invitation_id,
                    direction,
                    other_party_id,
                    other_party_name,
                    invitation_type,
                    expires_at,
                    message,
                },
            ) => {
                if other_ts >= *created_at {
                    *created_at = other_ts;
                    *id = invitation_id;
                    *dir = direction;
                    *other_id = other_party_id;
                    *other_name = other_party_name;
                    *inv_type = invitation_type;
                    *exp = expires_at;
                    *msg = message;
                }
                true
            }
            (
                InvitationDelta::InvitationStatusChanged { outcomes, .. },
                InvitationDelta::InvitationStatusChanged {
                    outcomes: other, ..
                },
            ) => {
                outcomes.merge(other);
                true
            }
            (
                current @ InvitationDelta::CeremonyStatusChanged { .. },
                other @ InvitationDelta::CeremonyStatusChanged { .. },
            ) => {
                if ceremony_delta_cmp(&other, current) == Ordering::Greater {
                    *current = other;
                }
                true
            }
            _ => false,
        }
    }
}

/// Deterministic total order of two ceremony deltas for the same ceremony:
/// stage precedence (`lifecycle::ceremony_status_cmp`), then content. Physical
/// time is only the last tie-break between otherwise identical records.
fn ceremony_delta_cmp(a: &InvitationDelta, b: &InvitationDelta) -> Ordering {
    let key = |delta: &InvitationDelta| match delta {
        InvitationDelta::CeremonyStatusChanged {
            status,
            reason,
            relationship_id,
            agreement_mode,
            observed_at,
            ..
        } => Some((
            *status,
            relationship_id.as_ref().map(|id| id.as_str().to_owned()),
            reason.clone(),
            agreement_mode.map(|mode| format!("{mode:?}")),
            observed_at.ts_ms,
        )),
        _ => None,
    };
    match (key(a), key(b)) {
        (Some(left), Some(right)) => ceremony_status_cmp(left.0, right.0)
            .then_with(|| (left.1, left.2, left.3).cmp(&(right.1, right.2, right.3)))
            .then_with(|| right.4.cmp(&left.4)),
        _ => Ordering::Equal,
    }
}

/// View reducer for invitation facts.
///
/// Transforms `InvitationFact` instances into `InvitationDelta` view updates.
pub struct InvitationViewReducer;

impl InvitationViewReducer {
    fn invitation_direction(
        own_authority: Option<AuthorityId>,
        sender_id: AuthorityId,
        receiver_id: AuthorityId,
    ) -> (InvitationDirection, AuthorityId) {
        match own_authority {
            Some(own) if sender_id == own => (InvitationDirection::Outbound, receiver_id),
            Some(own) if receiver_id == own => (InvitationDirection::Inbound, sender_id),
            Some(_) => (InvitationDirection::Observed, receiver_id),
            None => (InvitationDirection::Outbound, receiver_id),
        }
    }

    fn ceremony_status_delta(
        ceremony_id: CeremonyId,
        status: CeremonyViewStatus,
        reason: Option<String>,
        relationship_id: Option<CeremonyRelationshipId>,
        agreement_mode: Option<AgreementMode>,
        observed_at: PhysicalTime,
    ) -> InvitationDelta {
        InvitationDelta::CeremonyStatusChanged {
            ceremony_id,
            status,
            reason,
            relationship_id,
            reversion_risk: !matches!(agreement_mode, Some(AgreementMode::ConsensusFinalized)),
            agreement_mode,
            observed_at,
        }
    }
}

impl ViewDeltaReducer for InvitationViewReducer {
    fn handles_type(&self) -> &'static str {
        INVITATION_FACT_TYPE_ID
    }

    fn reduce_fact(
        &self,
        binding_type: &str,
        binding_data: &[u8],
        own_authority: Option<AuthorityId>,
    ) -> Vec<ViewDelta> {
        if binding_type != INVITATION_FACT_TYPE_ID {
            return vec![];
        }

        let Some(inv_fact) = InvitationFact::from_bytes(binding_data) else {
            return vec![];
        };

        if let Some(outcome) = InvitationOutcome::from_fact(&inv_fact) {
            let invitation_id = outcome.invitation_id.clone();
            let mut outcomes = InvitationOutcomes::default();
            outcomes.insert(outcome);
            return vec![InvitationDelta::InvitationStatusChanged {
                invitation_id,
                outcomes,
            }
            .into_view_delta()];
        }

        let delta = match inv_fact {
            InvitationFact::Sent {
                invitation_id,
                sender_id,
                receiver_id,
                invitation_type,
                sent_at,
                expires_at,
                message,
                ..
            } => {
                let (direction, other_party_id) =
                    Self::invitation_direction(own_authority, sender_id, receiver_id);

                InvitationDelta::InvitationAdded {
                    invitation_id,
                    direction,
                    other_party_id,
                    other_party_name: "Unknown".to_string(), // Would come from contact facts
                    invitation_type,
                    created_at: sent_at.ts_ms,
                    expires_at: expires_at.map(|t| t.ts_ms),
                    message,
                }
            }
            InvitationFact::Accepted { .. }
            | InvitationFact::Declined { .. }
            | InvitationFact::Cancelled { .. } => return vec![],
            InvitationFact::CeremonyInitiated {
                ceremony_id,
                agreement_mode,
                observed_at,
                ..
            } => Self::ceremony_status_delta(
                ceremony_id,
                CeremonyViewStatus::Initiated,
                None,
                None,
                agreement_mode,
                observed_at,
            ),
            InvitationFact::CeremonyAcceptanceReceived {
                ceremony_id,
                agreement_mode,
                observed_at,
                ..
            } => Self::ceremony_status_delta(
                ceremony_id,
                CeremonyViewStatus::AcceptanceReceived,
                None,
                None,
                agreement_mode,
                observed_at,
            ),
            InvitationFact::CeremonyCommitted {
                ceremony_id,
                relationship_id,
                agreement_mode,
                observed_at,
                ..
            } => Self::ceremony_status_delta(
                ceremony_id,
                CeremonyViewStatus::Committed,
                None,
                Some(relationship_id),
                agreement_mode,
                observed_at,
            ),
            InvitationFact::CeremonyAborted {
                ceremony_id,
                reason,
                observed_at,
                ..
            } => Self::ceremony_status_delta(
                ceremony_id,
                CeremonyViewStatus::Aborted,
                Some(reason),
                None,
                None,
                observed_at,
            ),
            InvitationFact::CeremonySuperseded {
                superseded_ceremony_id,
                superseding_ceremony_id,
                reason,
                observed_at,
                ..
            } => Self::ceremony_status_delta(
                superseded_ceremony_id,
                CeremonyViewStatus::Superseded,
                Some(format!(
                    "{reason} (superseded by {superseding_ceremony_id})"
                )),
                None,
                None,
                observed_at,
            ),
        };

        vec![delta.into_view_delta()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::InvitationStatus;
    use assert_matches::assert_matches;
    use aura_composition::compact_deltas;
    use aura_composition::downcast_delta;
    use aura_core::types::identifiers::{AuthorityId, ContextId, InvitationId};

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_invitation_sent_reduction_as_sender() {
        let reducer = InvitationViewReducer;
        let sender = test_authority_id(1);
        let receiver = test_authority_id(2);

        let fact = InvitationFact::sent_ms(
            test_context_id(),
            InvitationId::new("inv-123"),
            sender,
            receiver,
            crate::InvitationType::Contact { nickname: None },
            1234567890,
            Some(1234567890 + 86400000),
            Some("Please be my guardian".to_string()),
        );

        let bytes = fact.to_bytes();
        // Reduce as the sender - should be outbound
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes, Some(sender));

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<InvitationDelta>(&deltas[0]).unwrap();
        let InvitationDelta::InvitationAdded {
            invitation_id,
            direction,
            invitation_type,
            message,
            ..
        } = delta
        else {
            panic!("Expected InvitationAdded delta");
        };
        assert_eq!(invitation_id.as_str(), "inv-123");
        assert_eq!(*direction, InvitationDirection::Outbound);
        assert_matches!(
            invitation_type,
            crate::InvitationType::Contact { nickname: None }
        );
        assert_eq!(message, &Some("Please be my guardian".to_string()));
    }

    #[test]
    fn test_invitation_sent_reduction_as_receiver() {
        let reducer = InvitationViewReducer;
        let sender = test_authority_id(1);
        let receiver = test_authority_id(2);

        let fact = InvitationFact::sent_ms(
            test_context_id(),
            InvitationId::new("inv-124"),
            sender,
            receiver,
            crate::InvitationType::Contact { nickname: None },
            1234567890,
            Some(1234567890 + 86400000),
            Some("Please be my guardian".to_string()),
        );

        let bytes = fact.to_bytes();
        // Reduce as the receiver - should be inbound
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes, Some(receiver));

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<InvitationDelta>(&deltas[0]).unwrap();
        let InvitationDelta::InvitationAdded {
            invitation_id,
            direction,
            ..
        } = delta
        else {
            panic!("Expected InvitationAdded delta");
        };
        assert_eq!(invitation_id.as_str(), "inv-124");
        assert_eq!(*direction, InvitationDirection::Inbound);
    }

    fn causal(device: u8, counter: u64) -> aura_core::time::CausalMetadata {
        aura_core::time::CausalMetadata {
            revokes: Vec::new(),
            supersedes: Vec::new(),
            clock: aura_core::time::CausalClock {
                lamport: counter,
                vector: vec![(aura_core::DeviceId::new_from_entropy([device; 32]), counter)],
            },
        }
    }

    fn status_delta(fact: &InvitationFact) -> InvitationDelta {
        let deltas =
            InvitationViewReducer.reduce_fact(INVITATION_FACT_TYPE_ID, &fact.to_bytes(), None);
        assert_eq!(deltas.len(), 1);
        downcast_delta::<InvitationDelta>(&deltas[0])
            .unwrap()
            .clone()
    }

    fn resolved(delta: &InvitationDelta) -> Option<InvitationStatus> {
        match delta {
            InvitationDelta::InvitationStatusChanged { outcomes, .. } => outcomes.status(),
            other => panic!("expected InvitationStatusChanged, got {other:?}"),
        }
    }

    #[test]
    fn test_invitation_outcome_reductions() {
        let id = || InvitationId::new("inv-456");
        let accepted = InvitationFact::accepted_ms(id(), test_authority_id(3), 1, causal(3, 1));
        assert_eq!(
            resolved(&status_delta(&accepted)),
            Some(InvitationStatus::Accepted)
        );
        let cancelled = InvitationFact::cancelled_ms(id(), test_authority_id(4), 2, causal(4, 1));
        assert_eq!(
            resolved(&status_delta(&cancelled)),
            Some(InvitationStatus::Cancelled)
        );
    }

    #[test]
    fn test_wrong_type_returns_empty() {
        let reducer = InvitationViewReducer;
        let deltas = reducer.reduce_fact("wrong_type", b"some data", None);
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_invalid_data_returns_empty() {
        let reducer = InvitationViewReducer;
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, b"invalid json data", None);
        assert!(deltas.is_empty());
    }

    /// Compacting status deltas unions outcome sets: every arrival order
    /// resolves to the same status, regardless of physical time.
    #[test]
    fn test_compact_status_deltas_is_order_independent() {
        let id = || InvitationId::new("inv-1");
        let deltas = vec![
            status_delta(&InvitationFact::accepted_ms(
                id(),
                test_authority_id(2),
                900,
                causal(2, 1),
            )),
            status_delta(&InvitationFact::declined_ms(
                id(),
                test_authority_id(3),
                100,
                causal(3, 1),
            )),
            status_delta(&InvitationFact::accepted_ms(
                id(),
                test_authority_id(2),
                900,
                causal(2, 1),
            )),
        ];
        let compacted =
            aura_journal::causal_reduction::assert_permutation_invariant(&deltas, |order| {
                compact_deltas(order.to_vec())
            });
        assert_eq!(compacted.len(), 1);
        assert_eq!(resolved(&compacted[0]), Some(InvitationStatus::Declined));
    }

    /// Ceremony deltas resolve by stage precedence, not by timestamp.
    #[test]
    fn test_compact_ceremony_deltas_is_order_independent() {
        let ceremony = || CeremonyId::new("ceremony-1");
        let facts = [
            InvitationFact::CeremonyInitiated {
                context_id: None,
                ceremony_id: ceremony(),
                sender: test_authority_id(1),
                agreement_mode: None,
                trace_id: None,
                observed_at: PhysicalTime::exact(300),
            },
            InvitationFact::CeremonyCommitted {
                context_id: Some(test_context_id()),
                ceremony_id: ceremony(),
                relationship_id: CeremonyRelationshipId::parse("rel-0011223344556677").unwrap(),
                agreement_mode: Some(AgreementMode::ConsensusFinalized),
                trace_id: None,
                observed_at: PhysicalTime::exact(100),
            },
            InvitationFact::CeremonyAborted {
                context_id: None,
                ceremony_id: ceremony(),
                reason: "timeout".to_string(),
                trace_id: None,
                observed_at: PhysicalTime::exact(200),
            },
        ];
        let deltas: Vec<InvitationDelta> = facts.iter().map(status_delta).collect();
        let compacted =
            aura_journal::causal_reduction::assert_permutation_invariant(&deltas, |order| {
                compact_deltas(order.to_vec())
            });
        assert_matches!(
            &compacted[..],
            [InvitationDelta::CeremonyStatusChanged {
                status: CeremonyViewStatus::Committed,
                reversion_risk: false,
                ..
            }]
        );
    }
}
