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
        /// Type: "guardian", "channel", "contact"
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
}

/// View reducer for invitation facts.
///
/// Transforms `InvitationFact` instances into `InvitationDelta` view updates.
pub struct InvitationViewReducer;

impl ViewDeltaReducer for InvitationViewReducer {
    fn handles_type(&self) -> &'static str {
        INVITATION_FACT_TYPE_ID
    }

    fn reduce_fact(&self, binding_type: &str, binding_data: &[u8]) -> Vec<ViewDelta> {
        if binding_type != INVITATION_FACT_TYPE_ID {
            return vec![];
        }

        let Some(inv_fact) = InvitationFact::from_bytes(binding_data) else {
            return vec![];
        };

        let delta = match inv_fact {
            InvitationFact::Sent {
                invitation_id,
                sender_id: _,
                receiver_id,
                invitation_type,
                sent_at_ms,
                expires_at_ms,
                message,
                ..
            } => {
                // Note: Direction would need current authority context to determine
                // For now, we assume outbound since we're reducing a Sent fact
                Some(InvitationDelta::InvitationAdded {
                    invitation_id,
                    direction: "outbound".to_string(), // Would need current authority context
                    other_party_id: format!("{:?}", receiver_id),
                    other_party_name: "Unknown".to_string(), // Would come from contact facts
                    invitation_type,
                    created_at: sent_at_ms,
                    expires_at: expires_at_ms,
                    message,
                })
            }
            InvitationFact::Accepted {
                invitation_id,
                accepted_at_ms,
                ..
            } => Some(InvitationDelta::InvitationStatusChanged {
                invitation_id,
                old_status: "pending".to_string(),
                new_status: "accepted".to_string(),
                changed_at: accepted_at_ms,
            }),
            InvitationFact::Declined {
                invitation_id,
                declined_at_ms,
                ..
            } => Some(InvitationDelta::InvitationStatusChanged {
                invitation_id,
                old_status: "pending".to_string(),
                new_status: "declined".to_string(),
                changed_at: declined_at_ms,
            }),
            InvitationFact::Cancelled { invitation_id, .. } => {
                Some(InvitationDelta::InvitationRemoved { invitation_id })
            }
        };

        delta.map(|d| vec![d.into_view_delta()]).unwrap_or_default()
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
    fn test_invitation_sent_reduction() {
        let reducer = InvitationViewReducer;

        let fact = InvitationFact::Sent {
            context_id: test_context_id(),
            invitation_id: "inv-123".to_string(),
            sender_id: test_authority_id(1),
            receiver_id: test_authority_id(2),
            invitation_type: "guardian".to_string(),
            sent_at_ms: 1234567890,
            expires_at_ms: Some(1234567890 + 86400000),
            message: Some("Please be my guardian".to_string()),
        };

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes);

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
    fn test_invitation_accepted_reduction() {
        let reducer = InvitationViewReducer;

        let fact = InvitationFact::Accepted {
            invitation_id: "inv-456".to_string(),
            acceptor_id: test_authority_id(3),
            accepted_at_ms: 1234567899,
        };

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes);

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

        let fact = InvitationFact::Cancelled {
            invitation_id: "inv-789".to_string(),
            canceller_id: test_authority_id(4),
            cancelled_at_ms: 1234567900,
        };

        let bytes = fact.to_bytes();
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, &bytes);

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
        let deltas = reducer.reduce_fact("wrong_type", b"some data");
        assert!(deltas.is_empty());
    }

    #[test]
    fn test_invalid_data_returns_empty() {
        let reducer = InvitationViewReducer;
        let deltas = reducer.reduce_fact(INVITATION_FACT_TYPE_ID, b"invalid json data");
        assert!(deltas.is_empty());
    }
}
