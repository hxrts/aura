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

use aura_composition::{IntoViewDelta, ViewDelta, ViewDeltaReducer};
use aura_core::identifiers::AuthorityId;
use aura_journal::DomainFact;

use crate::{InvitationFact, INVITATION_FACT_TYPE_ID};

/// Delta type for invitation view updates.
///
/// These deltas represent incremental changes to invitation UI state,
/// derived from journal facts during view reduction.
#[derive(Debug, Clone, PartialEq)]
pub enum InvitationDelta {
    /// A new invitation was created or received
    InvitationAdded {
        invitation_id: String,
        /// Direction: "inbound" or "outbound"
        direction: String,
        other_party_id: String,
        other_party_name: String,
        /// Type: "guardian", "channel", "contact", "device"
        invitation_type: String,
        created_at: u64,
        expires_at: Option<u64>,
        message: Option<String>,
    },
    /// Invitation status changed
    InvitationStatusChanged {
        invitation_id: String,
        old_status: String,
        /// Status: "pending", "accepted", "declined", "expired", "cancelled"
        new_status: String,
        changed_at: u64,
    },
    /// Invitation was removed/deleted
    InvitationRemoved { invitation_id: String },
    /// Ceremony status changed
    CeremonyStatusChanged {
        ceremony_id: String,
        /// Status: "initiated", "acceptance_received", "committed", "aborted"
        status: String,
        /// For "aborted" status, the reason
        reason: Option<String>,
        /// For "committed" status, the resulting relationship ID
        relationship_id: Option<String>,
        timestamp_ms: u64,
    },
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
                        ("outbound".to_string(), format!("{:?}", receiver_id))
                    } else if receiver_id == own {
                        ("inbound".to_string(), format!("{:?}", sender_id))
                    } else {
                        // Neither sender nor receiver - this is a third-party observation
                        ("observed".to_string(), format!("{:?}", receiver_id))
                    }
                } else {
                    // No authority context - default to outbound for Sent facts
                    ("outbound".to_string(), format!("{:?}", receiver_id))
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
                old_status: "pending".to_string(),
                new_status: "accepted".to_string(),
                changed_at: accepted_at.ts_ms,
            }),
            InvitationFact::Declined {
                invitation_id,
                declined_at,
                ..
            } => Some(InvitationDelta::InvitationStatusChanged {
                invitation_id,
                old_status: "pending".to_string(),
                new_status: "declined".to_string(),
                changed_at: declined_at.ts_ms,
            }),
            InvitationFact::Cancelled { invitation_id, .. } => {
                Some(InvitationDelta::InvitationRemoved { invitation_id })
            }
            // Ceremony facts
            InvitationFact::CeremonyInitiated {
                ceremony_id,
                timestamp_ms,
                ..
            } => Some(InvitationDelta::CeremonyStatusChanged {
                ceremony_id,
                status: "initiated".to_string(),
                reason: None,
                relationship_id: None,
                timestamp_ms,
            }),
            InvitationFact::CeremonyAcceptanceReceived {
                ceremony_id,
                timestamp_ms,
            } => Some(InvitationDelta::CeremonyStatusChanged {
                ceremony_id,
                status: "acceptance_received".to_string(),
                reason: None,
                relationship_id: None,
                timestamp_ms,
            }),
            InvitationFact::CeremonyCommitted {
                ceremony_id,
                relationship_id,
                timestamp_ms,
            } => Some(InvitationDelta::CeremonyStatusChanged {
                ceremony_id,
                status: "committed".to_string(),
                reason: None,
                relationship_id: Some(relationship_id),
                timestamp_ms,
            }),
            InvitationFact::CeremonyAborted {
                ceremony_id,
                reason,
                timestamp_ms,
            } => Some(InvitationDelta::CeremonyStatusChanged {
                ceremony_id,
                status: "aborted".to_string(),
                reason: Some(reason),
                relationship_id: None,
                timestamp_ms,
            }),
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
    use aura_composition::downcast_delta;
    use aura_core::identifiers::{AuthorityId, ContextId};

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
            "inv-123".to_string(),
            sender,
            receiver,
            "guardian".to_string(),
            1234567890,
            Some(1234567890 + 86400000),
            Some("Please be my guardian".to_string()),
        );

        let bytes = fact.to_bytes();
        // Reduce as the sender - should be outbound
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes, Some(sender));

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<InvitationDelta>(&deltas[0]).unwrap();
        match delta {
            InvitationDelta::InvitationAdded {
                invitation_id,
                direction,
                invitation_type,
                message,
                ..
            } => {
                assert_eq!(invitation_id, "inv-123");
                assert_eq!(direction, "outbound");
                assert_eq!(invitation_type, "guardian");
                assert_eq!(message, &Some("Please be my guardian".to_string()));
            }
            _ => panic!("Expected InvitationAdded delta"),
        }
    }

    #[test]
    fn test_invitation_sent_reduction_as_receiver() {
        let reducer = InvitationViewReducer;
        let sender = test_authority_id(1);
        let receiver = test_authority_id(2);

        let fact = InvitationFact::sent_ms(
            test_context_id(),
            "inv-124".to_string(),
            sender,
            receiver,
            "guardian".to_string(),
            1234567890,
            Some(1234567890 + 86400000),
            Some("Please be my guardian".to_string()),
        );

        let bytes = fact.to_bytes();
        // Reduce as the receiver - should be inbound
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes, Some(receiver));

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<InvitationDelta>(&deltas[0]).unwrap();
        match delta {
            InvitationDelta::InvitationAdded {
                invitation_id,
                direction,
                ..
            } => {
                assert_eq!(invitation_id, "inv-124");
                assert_eq!(direction, "inbound");
            }
            _ => panic!("Expected InvitationAdded delta"),
        }
    }

    #[test]
    fn test_invitation_accepted_reduction() {
        let reducer = InvitationViewReducer;

        let fact =
            InvitationFact::accepted_ms("inv-456".to_string(), test_authority_id(3), 1234567899);

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<InvitationDelta>(&deltas[0]).unwrap();
        match delta {
            InvitationDelta::InvitationStatusChanged {
                invitation_id,
                old_status,
                new_status,
                ..
            } => {
                assert_eq!(invitation_id, "inv-456");
                assert_eq!(old_status, "pending");
                assert_eq!(new_status, "accepted");
            }
            _ => panic!("Expected InvitationStatusChanged delta"),
        }
    }

    #[test]
    fn test_invitation_cancelled_reduction() {
        let reducer = InvitationViewReducer;

        let fact =
            InvitationFact::cancelled_ms("inv-789".to_string(), test_authority_id(4), 1234567900);

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes, None);

        assert_eq!(deltas.len(), 1);
        let delta = downcast_delta::<InvitationDelta>(&deltas[0]).unwrap();
        match delta {
            InvitationDelta::InvitationRemoved { invitation_id } => {
                assert_eq!(invitation_id, "inv-789");
            }
            _ => panic!("Expected InvitationRemoved delta"),
        }
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
}
