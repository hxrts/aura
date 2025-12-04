//! Invitation domain facts
//!
//! This module defines invitation-specific fact types that implement the `DomainFact`
//! trait from `aura-journal`. These facts are stored as `RelationalFact::Generic`
//! in the journal and reduced using the `InvitationFactReducer`.
//!
//! # Architecture
//!
//! Following the Open/Closed Principle:
//! - `aura-journal` provides the generic fact infrastructure
//! - `aura-invitation` defines domain-specific fact types without modifying `aura-journal`
//! - Runtime registers `InvitationFactReducer` with the `FactRegistry`
//!
//! # Example
//!
//! ```ignore
//! use aura_invitation::facts::{InvitationFact, InvitationFactReducer};
//! use aura_journal::{FactRegistry, DomainFact};
//!
//! // Create an invitation fact
//! let fact = InvitationFact::Sent {
//!     context_id,
//!     invitation_id: "inv-123".to_string(),
//!     sender_id,
//!     receiver_id,
//!     invitation_type: "guardian".to_string(),
//!     sent_at_ms: 1234567890,
//!     expires_at_ms: Some(1234567890 + 86400000),
//!     message: Some("Please be my guardian".to_string()),
//! };
//!
//! // Convert to generic for storage
//! let generic = fact.to_generic();
//!
//! // Register reducer at runtime
//! registry.register::<InvitationFact>("invitation", Box::new(InvitationFactReducer));
//! ```

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use serde::{Deserialize, Serialize};

/// Type identifier for invitation facts
pub const INVITATION_FACT_TYPE_ID: &str = "invitation";

/// Invitation domain fact types
///
/// These facts represent invitation-related state changes in the journal.
/// They are stored as `RelationalFact::Generic` and reduced by `InvitationFactReducer`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InvitationFact {
    /// Invitation sent from one authority to another
    Sent {
        /// Relational context for the invitation
        context_id: ContextId,
        /// Unique invitation identifier
        invitation_id: String,
        /// Authority sending the invitation
        sender_id: AuthorityId,
        /// Authority receiving the invitation
        receiver_id: AuthorityId,
        /// Type of invitation: "guardian", "channel", "contact"
        invitation_type: String,
        /// Timestamp when invitation was sent (ms since epoch)
        sent_at_ms: u64,
        /// Optional expiration timestamp (ms since epoch)
        expires_at_ms: Option<u64>,
        /// Optional message with the invitation
        message: Option<String>,
    },
    /// Invitation accepted
    Accepted {
        /// Invitation being accepted
        invitation_id: String,
        /// Authority accepting the invitation
        acceptor_id: AuthorityId,
        /// Timestamp when invitation was accepted (ms since epoch)
        accepted_at_ms: u64,
    },
    /// Invitation declined
    Declined {
        /// Invitation being declined
        invitation_id: String,
        /// Authority declining the invitation
        decliner_id: AuthorityId,
        /// Timestamp when invitation was declined (ms since epoch)
        declined_at_ms: u64,
    },
    /// Invitation cancelled by sender
    Cancelled {
        /// Invitation being cancelled
        invitation_id: String,
        /// Authority cancelling the invitation (must be sender)
        canceller_id: AuthorityId,
        /// Timestamp when invitation was cancelled (ms since epoch)
        cancelled_at_ms: u64,
    },
}

impl InvitationFact {
    /// Extract the invitation_id from any variant
    pub fn invitation_id(&self) -> &str {
        match self {
            InvitationFact::Sent { invitation_id, .. } => invitation_id,
            InvitationFact::Accepted { invitation_id, .. } => invitation_id,
            InvitationFact::Declined { invitation_id, .. } => invitation_id,
            InvitationFact::Cancelled { invitation_id, .. } => invitation_id,
        }
    }
}

impl DomainFact for InvitationFact {
    fn type_id(&self) -> &'static str {
        INVITATION_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        match self {
            InvitationFact::Sent { context_id, .. } => *context_id,
            // For non-Sent variants, derive context from invitation_id
            // In practice, these would lookup the original context from the Sent fact
            InvitationFact::Accepted { .. }
            | InvitationFact::Declined { .. }
            | InvitationFact::Cancelled { .. } => {
                // Return a deterministic placeholder - actual context comes from lookup
                ContextId::new_from_entropy([0u8; 32])
            }
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        serde_json::from_slice(bytes).ok()
    }
}

/// Reducer for invitation facts
///
/// Converts invitation facts to relational bindings during journal reduction.
pub struct InvitationFactReducer;

impl FactReducer for InvitationFactReducer {
    fn handles_type(&self) -> &'static str {
        INVITATION_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != INVITATION_FACT_TYPE_ID {
            return None;
        }

        let fact: InvitationFact = serde_json::from_slice(binding_data).ok()?;

        let (sub_type, data) = match &fact {
            InvitationFact::Sent { invitation_id, .. } => (
                "invitation-sent".to_string(),
                invitation_id.as_bytes().to_vec(),
            ),
            InvitationFact::Accepted { invitation_id, .. } => (
                "invitation-accepted".to_string(),
                invitation_id.as_bytes().to_vec(),
            ),
            InvitationFact::Declined { invitation_id, .. } => (
                "invitation-declined".to_string(),
                invitation_id.as_bytes().to_vec(),
            ),
            InvitationFact::Cancelled { invitation_id, .. } => (
                "invitation-cancelled".to_string(),
                invitation_id.as_bytes().to_vec(),
            ),
        };

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(sub_type),
            context_id,
            data,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_context_id() -> ContextId {
        ContextId::new_from_entropy([42u8; 32])
    }

    fn test_authority_id(seed: u8) -> AuthorityId {
        AuthorityId::new_from_entropy([seed; 32])
    }

    #[test]
    fn test_invitation_fact_serialization() {
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
        let restored = InvitationFact::from_bytes(&bytes);
        assert!(restored.is_some());
        assert_eq!(restored.unwrap(), fact);
    }

    #[test]
    fn test_invitation_fact_to_generic() {
        let fact = InvitationFact::Accepted {
            invitation_id: "inv-456".to_string(),
            acceptor_id: test_authority_id(3),
            accepted_at_ms: 1234567899,
        };

        let generic = fact.to_generic();

        if let aura_journal::RelationalFact::Generic {
            binding_type,
            binding_data,
            ..
        } = generic
        {
            assert_eq!(binding_type, INVITATION_FACT_TYPE_ID);
            let restored = InvitationFact::from_bytes(&binding_data);
            assert!(restored.is_some());
        } else {
            panic!("Expected Generic variant");
        }
    }

    #[test]
    fn test_invitation_fact_reducer() {
        let reducer = InvitationFactReducer;
        assert_eq!(reducer.handles_type(), INVITATION_FACT_TYPE_ID);

        let fact = InvitationFact::Sent {
            context_id: test_context_id(),
            invitation_id: "inv-789".to_string(),
            sender_id: test_authority_id(4),
            receiver_id: test_authority_id(5),
            invitation_type: "contact".to_string(),
            sent_at_ms: 0,
            expires_at_ms: None,
            message: None,
        };

        let bytes = fact.to_bytes();
        let binding = reducer.reduce(test_context_id(), INVITATION_FACT_TYPE_ID, &bytes);

        assert!(binding.is_some());
        let binding = binding.unwrap();
        assert!(matches!(
            binding.binding_type,
            RelationalBindingType::Generic(ref s) if s == "invitation-sent"
        ));
    }

    #[test]
    fn test_invitation_id_extraction() {
        let facts = vec![
            InvitationFact::Sent {
                context_id: test_context_id(),
                invitation_id: "inv-1".to_string(),
                sender_id: test_authority_id(1),
                receiver_id: test_authority_id(2),
                invitation_type: "guardian".to_string(),
                sent_at_ms: 0,
                expires_at_ms: None,
                message: None,
            },
            InvitationFact::Accepted {
                invitation_id: "inv-2".to_string(),
                acceptor_id: test_authority_id(3),
                accepted_at_ms: 0,
            },
            InvitationFact::Declined {
                invitation_id: "inv-3".to_string(),
                decliner_id: test_authority_id(4),
                declined_at_ms: 0,
            },
            InvitationFact::Cancelled {
                invitation_id: "inv-4".to_string(),
                canceller_id: test_authority_id(5),
                cancelled_at_ms: 0,
            },
        ];

        assert_eq!(facts[0].invitation_id(), "inv-1");
        assert_eq!(facts[1].invitation_id(), "inv-2");
        assert_eq!(facts[2].invitation_id(), "inv-3");
        assert_eq!(facts[3].invitation_id(), "inv-4");
    }

    #[test]
    fn test_type_id_consistency() {
        let facts: Vec<InvitationFact> = vec![
            InvitationFact::Sent {
                context_id: test_context_id(),
                invitation_id: "x".to_string(),
                sender_id: test_authority_id(1),
                receiver_id: test_authority_id(2),
                invitation_type: "t".to_string(),
                sent_at_ms: 0,
                expires_at_ms: None,
                message: None,
            },
            InvitationFact::Accepted {
                invitation_id: "x".to_string(),
                acceptor_id: test_authority_id(3),
                accepted_at_ms: 0,
            },
            InvitationFact::Declined {
                invitation_id: "x".to_string(),
                decliner_id: test_authority_id(4),
                declined_at_ms: 0,
            },
            InvitationFact::Cancelled {
                invitation_id: "x".to_string(),
                canceller_id: test_authority_id(5),
                cancelled_at_ms: 0,
            },
        ];

        for fact in facts {
            assert_eq!(fact.type_id(), INVITATION_FACT_TYPE_ID);
        }
    }
}
