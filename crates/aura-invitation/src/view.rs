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
use aura_core::identifiers::{AuthorityId, CeremonyId, InvitationId};
use aura_core::threshold::AgreementMode;
use aura_journal::DomainFact;

use crate::{
    facts::CeremonyRelationshipId, InvitationFact, InvitationStatus, INVITATION_FACT_TYPE_ID,
};

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
    /// Invitation status changed
    InvitationStatusChanged {
        invitation_id: InvitationId,
        old_status: InvitationStatus,
        new_status: InvitationStatus,
        changed_at: u64,
    },
    /// Invitation was removed/deleted
    InvitationRemoved { invitation_id: InvitationId },
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
        timestamp_ms: u64,
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
            | InvitationDelta::InvitationStatusChanged { invitation_id, .. }
            | InvitationDelta::InvitationRemoved { invitation_id } => {
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
                InvitationDelta::InvitationStatusChanged {
                    changed_at,
                    invitation_id: id,
                    old_status: old,
                    new_status: new,
                },
                InvitationDelta::InvitationStatusChanged {
                    changed_at: other_ts,
                    invitation_id,
                    old_status,
                    new_status,
                },
            ) => {
                if other_ts >= *changed_at {
                    *changed_at = other_ts;
                    *id = invitation_id;
                    *old = old_status;
                    *new = new_status;
                }
                true
            }
            (
                InvitationDelta::InvitationRemoved { .. },
                InvitationDelta::InvitationRemoved { .. },
            ) => true,
            (
                InvitationDelta::CeremonyStatusChanged {
                    timestamp_ms,
                    ceremony_id: id,
                    status: st,
                    reason: rsn,
                    relationship_id: rel,
                    agreement_mode: mode,
                    reversion_risk: risk,
                },
                InvitationDelta::CeremonyStatusChanged {
                    timestamp_ms: other_ts,
                    ceremony_id,
                    status,
                    reason,
                    relationship_id,
                    agreement_mode,
                    reversion_risk,
                },
            ) => {
                if other_ts >= *timestamp_ms {
                    *timestamp_ms = other_ts;
                    *id = ceremony_id;
                    *st = status;
                    *rsn = reason;
                    *rel = relationship_id;
                    *mode = agreement_mode;
                    *risk = reversion_risk;
                }
                true
            }
            _ => false,
        }
    }
}

/// View reducer for invitation facts.
///
/// Transforms `InvitationFact` instances into `InvitationDelta` view updates.
pub struct InvitationViewReducer;

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
                // Determine direction based on whether we sent or received the invitation
                let (direction, other_party_id) = if let Some(own) = own_authority {
                    if sender_id == own {
                        (InvitationDirection::Outbound, receiver_id)
                    } else if receiver_id == own {
                        (InvitationDirection::Inbound, sender_id)
                    } else {
                        // Neither sender nor receiver - this is a third-party observation
                        (InvitationDirection::Observed, receiver_id)
                    }
                } else {
                    // No authority context - default to outbound for Sent facts
                    (InvitationDirection::Outbound, receiver_id)
                };

                Some(InvitationDelta::InvitationAdded {
                    invitation_id,
                    direction,
                    other_party_id,
                    other_party_name: "Unknown".to_string(), // Would come from contact facts
                    invitation_type,
                    created_at: sent_at.ts_ms,
                    expires_at: expires_at.map(|t| t.ts_ms),
                    message,
                })
            }
            InvitationFact::Accepted {
                invitation_id,
                accepted_at,
                ..
            } => Some(InvitationDelta::InvitationStatusChanged {
                invitation_id,
                old_status: InvitationStatus::Pending,
                new_status: InvitationStatus::Accepted,
                changed_at: accepted_at.ts_ms,
            }),
            InvitationFact::Declined {
                invitation_id,
                declined_at,
                ..
            } => Some(InvitationDelta::InvitationStatusChanged {
                invitation_id,
                old_status: InvitationStatus::Pending,
                new_status: InvitationStatus::Declined,
                changed_at: declined_at.ts_ms,
            }),
            InvitationFact::Cancelled { invitation_id, .. } => {
                Some(InvitationDelta::InvitationRemoved { invitation_id })
            }
            // Ceremony facts
            InvitationFact::CeremonyInitiated {
                ceremony_id,
                agreement_mode,
                timestamp_ms,
                ..
            } => Some(InvitationDelta::CeremonyStatusChanged {
                ceremony_id,
                status: CeremonyViewStatus::Initiated,
                reason: None,
                relationship_id: None,
                reversion_risk: !matches!(agreement_mode, Some(AgreementMode::ConsensusFinalized)),
                agreement_mode,
                timestamp_ms,
            }),
            InvitationFact::CeremonyAcceptanceReceived {
                ceremony_id,
                agreement_mode,
                timestamp_ms,
                ..
            } => Some(InvitationDelta::CeremonyStatusChanged {
                ceremony_id,
                status: CeremonyViewStatus::AcceptanceReceived,
                reason: None,
                relationship_id: None,
                reversion_risk: !matches!(agreement_mode, Some(AgreementMode::ConsensusFinalized)),
                agreement_mode,
                timestamp_ms,
            }),
            InvitationFact::CeremonyCommitted {
                ceremony_id,
                relationship_id,
                agreement_mode,
                timestamp_ms,
                ..
            } => Some(InvitationDelta::CeremonyStatusChanged {
                ceremony_id,
                status: CeremonyViewStatus::Committed,
                reason: None,
                relationship_id: Some(relationship_id),
                reversion_risk: !matches!(agreement_mode, Some(AgreementMode::ConsensusFinalized)),
                agreement_mode,
                timestamp_ms,
            }),
            InvitationFact::CeremonyAborted {
                ceremony_id,
                reason,
                timestamp_ms,
                ..
            } => Some(InvitationDelta::CeremonyStatusChanged {
                ceremony_id,
                status: CeremonyViewStatus::Aborted,
                reason: Some(reason),
                relationship_id: None,
                reversion_risk: true,
                agreement_mode: None,
                timestamp_ms,
            }),
            InvitationFact::CeremonySuperseded {
                superseded_ceremony_id,
                superseding_ceremony_id,
                reason,
                timestamp_ms,
                ..
            } => {
                let superseded_reason =
                    format!("{reason} (superseded by {superseding_ceremony_id})");
                Some(InvitationDelta::CeremonyStatusChanged {
                    ceremony_id: superseded_ceremony_id,
                    status: CeremonyViewStatus::Superseded,
                    reason: Some(superseded_reason),
                    relationship_id: None,
                    reversion_risk: true,
                    agreement_mode: None,
                    timestamp_ms,
                })
            }
        };

        match delta {
            Some(d) => vec![d.into_view_delta()],
            None => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assert_matches::assert_matches;
    use aura_composition::compact_deltas;
    use aura_composition::downcast_delta;
    use aura_core::identifiers::{AuthorityId, ContextId, InvitationId};

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

    #[test]
    fn test_invitation_accepted_reduction() {
        let reducer = InvitationViewReducer;

        let fact = InvitationFact::accepted_ms(
            InvitationId::new("inv-456"),
            test_authority_id(3),
            1234567899,
        );

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<InvitationDelta>(&deltas[0]).unwrap();
        let InvitationDelta::InvitationStatusChanged {
            invitation_id,
            old_status,
            new_status,
            ..
        } = delta
        else {
            panic!("Expected InvitationStatusChanged delta");
        };
        assert_eq!(invitation_id.as_str(), "inv-456");
        assert_eq!(*old_status, InvitationStatus::Pending);
        assert_eq!(*new_status, InvitationStatus::Accepted);
    }

    #[test]
    fn test_invitation_cancelled_reduction() {
        let reducer = InvitationViewReducer;

        let fact = InvitationFact::cancelled_ms(
            InvitationId::new("inv-789"),
            test_authority_id(4),
            1234567900,
        );

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<InvitationDelta>(&deltas[0]).unwrap();
        assert_matches!(delta, InvitationDelta::InvitationRemoved { invitation_id } if invitation_id.as_str() == "inv-789");
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

    #[test]
    fn test_view_reducer_handles_legacy_ceremony_committed_payload() {
        let reducer = InvitationViewReducer;
        let legacy = serde_json::json!({
            "CeremonyCommitted": {
                "context_id": null,
                "ceremony_id": "ceremony-legacy-view-1",
                "relationship_id": "rel-0011223344556677",
                "agreement_mode": null,
                "trace_id": null,
                "timestamp_ms": 123
            }
        });
        let legacy_fact: InvitationFact = serde_json::from_value(legacy)
            .unwrap_or_else(|error| panic!("legacy json should decode: {error}"));
        let bytes = legacy_fact.to_bytes();
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<InvitationDelta>(&deltas[0]).unwrap();
        let InvitationDelta::CeremonyStatusChanged {
            status,
            relationship_id,
            ..
        } = delta
        else {
            panic!("Expected CeremonyStatusChanged delta");
        };
        assert_eq!(*status, CeremonyViewStatus::Committed);
        assert_eq!(
            relationship_id.as_ref().map(|id| id.as_str()),
            Some("rel-0011223344556677")
        );
    }

    #[test]
    fn test_compact_deltas_merges_status_updates() {
        let deltas = vec![
            InvitationDelta::InvitationStatusChanged {
                invitation_id: InvitationId::new("inv-1"),
                old_status: InvitationStatus::Pending,
                new_status: InvitationStatus::Accepted,
                changed_at: 100,
            },
            InvitationDelta::InvitationStatusChanged {
                invitation_id: InvitationId::new("inv-1"),
                old_status: InvitationStatus::Accepted,
                new_status: InvitationStatus::Cancelled,
                changed_at: 200,
            },
        ];

        let compacted = compact_deltas(deltas);
        assert_eq!(compacted.len(), 1);
        let InvitationDelta::InvitationStatusChanged {
            new_status,
            changed_at,
            ..
        } = &compacted[0]
        else {
            panic!("Expected InvitationStatusChanged after compaction");
        };
        assert_eq!(*new_status, InvitationStatus::Cancelled);
        assert_eq!(*changed_at, 200);
    }
}
