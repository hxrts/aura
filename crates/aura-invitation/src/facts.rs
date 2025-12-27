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
//! // Create an invitation fact using backward-compatible constructor
//! let fact = InvitationFact::sent_ms(
//!     context_id,
//!     "inv-123".to_string(),
//!     sender_id,
//!     receiver_id,
//!     "guardian".to_string(),
//!     1234567890,
//!     Some(1234567890 + 86400000),
//!     Some("Please be my guardian".to_string()),
//! );
//!
//! // Convert to generic for storage
//! let generic = fact.to_generic();
//!
//! // Register reducer at runtime
//! registry.register::<InvitationFact>("invitation", Box::new(InvitationFactReducer));
//! ```

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::PhysicalTime;
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use serde::{Deserialize, Serialize};

/// Type identifier for invitation facts
pub const INVITATION_FACT_TYPE_ID: &str = "invitation";
pub const INVITATION_FACT_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationFactKey {
    pub sub_type: &'static str,
    pub data: Vec<u8>,
}

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
        /// Type of invitation: "guardian", "channel", "contact", "device"
        invitation_type: String,
        /// Timestamp when invitation was sent (uses unified time system)
        sent_at: PhysicalTime,
        /// Optional expiration timestamp (uses unified time system)
        expires_at: Option<PhysicalTime>,
        /// Optional message with the invitation
        message: Option<String>,
    },
    /// Invitation accepted
    Accepted {
        /// Invitation being accepted
        invitation_id: String,
        /// Authority accepting the invitation
        acceptor_id: AuthorityId,
        /// Timestamp when invitation was accepted (uses unified time system)
        accepted_at: PhysicalTime,
    },
    /// Invitation declined
    Declined {
        /// Invitation being declined
        invitation_id: String,
        /// Authority declining the invitation
        decliner_id: AuthorityId,
        /// Timestamp when invitation was declined (uses unified time system)
        declined_at: PhysicalTime,
    },
    /// Invitation cancelled by sender
    Cancelled {
        /// Invitation being cancelled
        invitation_id: String,
        /// Authority cancelling the invitation (must be sender)
        canceller_id: AuthorityId,
        /// Timestamp when invitation was cancelled (uses unified time system)
        cancelled_at: PhysicalTime,
    },

    // =========================================================================
    // Consensus-Based Ceremony Facts
    // =========================================================================
    /// Ceremony initiated by sender
    CeremonyInitiated {
        /// Unique ceremony identifier
        ceremony_id: String,
        /// Authority initiating the ceremony
        sender: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },

    /// Acceptance received from acceptor
    CeremonyAcceptanceReceived {
        /// Ceremony identifier
        ceremony_id: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },

    /// Ceremony committed (relationship established)
    CeremonyCommitted {
        /// Ceremony identifier
        ceremony_id: String,
        /// Resulting relationship identifier
        relationship_id: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },

    /// Ceremony aborted
    CeremonyAborted {
        /// Ceremony identifier
        ceremony_id: String,
        /// Reason for abortion
        reason: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },
}

impl InvitationFact {
    /// Extract the invitation_id from any variant (returns empty for ceremony facts)
    pub fn invitation_id(&self) -> &str {
        match self {
            InvitationFact::Sent { invitation_id, .. } => invitation_id,
            InvitationFact::Accepted { invitation_id, .. } => invitation_id,
            InvitationFact::Declined { invitation_id, .. } => invitation_id,
            InvitationFact::Cancelled { invitation_id, .. } => invitation_id,
            // Ceremony facts use ceremony_id, not invitation_id
            InvitationFact::CeremonyInitiated { ceremony_id, .. } => ceremony_id,
            InvitationFact::CeremonyAcceptanceReceived { ceremony_id, .. } => ceremony_id,
            InvitationFact::CeremonyCommitted { ceremony_id, .. } => ceremony_id,
            InvitationFact::CeremonyAborted { ceremony_id, .. } => ceremony_id,
        }
    }

    /// Extract the context_id when present.
    pub fn context_id_opt(&self) -> Option<ContextId> {
        match self {
            InvitationFact::Sent { context_id, .. } => Some(*context_id),
            InvitationFact::Accepted { .. }
            | InvitationFact::Declined { .. }
            | InvitationFact::Cancelled { .. }
            | InvitationFact::CeremonyInitiated { .. }
            | InvitationFact::CeremonyAcceptanceReceived { .. }
            | InvitationFact::CeremonyCommitted { .. }
            | InvitationFact::CeremonyAborted { .. } => None,
        }
    }

    /// Validate that this fact can be reduced under the provided context.
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.context_id_opt()
            .map(|fact_context_id| fact_context_id == context_id)
            .unwrap_or(true)
    }

    /// Get the timestamp in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            InvitationFact::Sent { sent_at, .. } => sent_at.ts_ms,
            InvitationFact::Accepted { accepted_at, .. } => accepted_at.ts_ms,
            InvitationFact::Declined { declined_at, .. } => declined_at.ts_ms,
            InvitationFact::Cancelled { cancelled_at, .. } => cancelled_at.ts_ms,
            // Ceremony facts already store ms
            InvitationFact::CeremonyInitiated { timestamp_ms, .. } => *timestamp_ms,
            InvitationFact::CeremonyAcceptanceReceived { timestamp_ms, .. } => *timestamp_ms,
            InvitationFact::CeremonyCommitted { timestamp_ms, .. } => *timestamp_ms,
            InvitationFact::CeremonyAborted { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    /// Derive the relational binding subtype and key data for this fact.
    pub fn binding_key(&self) -> InvitationFactKey {
        match self {
            InvitationFact::Sent { invitation_id, .. } => InvitationFactKey {
                sub_type: "invitation-sent",
                data: invitation_id.as_bytes().to_vec(),
            },
            InvitationFact::Accepted { invitation_id, .. } => InvitationFactKey {
                sub_type: "invitation-accepted",
                data: invitation_id.as_bytes().to_vec(),
            },
            InvitationFact::Declined { invitation_id, .. } => InvitationFactKey {
                sub_type: "invitation-declined",
                data: invitation_id.as_bytes().to_vec(),
            },
            InvitationFact::Cancelled { invitation_id, .. } => InvitationFactKey {
                sub_type: "invitation-cancelled",
                data: invitation_id.as_bytes().to_vec(),
            },
            InvitationFact::CeremonyInitiated { ceremony_id, .. } => InvitationFactKey {
                sub_type: "ceremony-initiated",
                data: ceremony_id.as_bytes().to_vec(),
            },
            InvitationFact::CeremonyAcceptanceReceived { ceremony_id, .. } => InvitationFactKey {
                sub_type: "ceremony-acceptance-received",
                data: ceremony_id.as_bytes().to_vec(),
            },
            InvitationFact::CeremonyCommitted { ceremony_id, .. } => InvitationFactKey {
                sub_type: "ceremony-committed",
                data: ceremony_id.as_bytes().to_vec(),
            },
            InvitationFact::CeremonyAborted { ceremony_id, .. } => InvitationFactKey {
                sub_type: "ceremony-aborted",
                data: ceremony_id.as_bytes().to_vec(),
            },
        }
    }

    /// Create a Sent fact with millisecond timestamps (backward compatibility)
    #[allow(clippy::too_many_arguments)]
    pub fn sent_ms(
        context_id: ContextId,
        invitation_id: String,
        sender_id: AuthorityId,
        receiver_id: AuthorityId,
        invitation_type: String,
        sent_at_ms: u64,
        expires_at_ms: Option<u64>,
        message: Option<String>,
    ) -> Self {
        Self::Sent {
            context_id,
            invitation_id,
            sender_id,
            receiver_id,
            invitation_type,
            sent_at: PhysicalTime {
                ts_ms: sent_at_ms,
                uncertainty: None,
            },
            expires_at: expires_at_ms.map(|ts_ms| PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
            message,
        }
    }

    /// Create an Accepted fact with millisecond timestamp (backward compatibility)
    pub fn accepted_ms(
        invitation_id: String,
        acceptor_id: AuthorityId,
        accepted_at_ms: u64,
    ) -> Self {
        Self::Accepted {
            invitation_id,
            acceptor_id,
            accepted_at: PhysicalTime {
                ts_ms: accepted_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a Declined fact with millisecond timestamp (backward compatibility)
    pub fn declined_ms(
        invitation_id: String,
        decliner_id: AuthorityId,
        declined_at_ms: u64,
    ) -> Self {
        Self::Declined {
            invitation_id,
            decliner_id,
            declined_at: PhysicalTime {
                ts_ms: declined_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a Cancelled fact with millisecond timestamp (backward compatibility)
    pub fn cancelled_ms(
        invitation_id: String,
        canceller_id: AuthorityId,
        cancelled_at_ms: u64,
    ) -> Self {
        Self::Cancelled {
            invitation_id,
            canceller_id,
            cancelled_at: PhysicalTime {
                ts_ms: cancelled_at_ms,
                uncertainty: None,
            },
        }
    }
}

impl DomainFact for InvitationFact {
    fn type_id(&self) -> &'static str {
        INVITATION_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.context_id_opt()
            .unwrap_or_else(|| ContextId::new_from_entropy([0u8; 32]))
    }

    fn to_bytes(&self) -> Vec<u8> {
        aura_journal::encode_domain_fact(self.type_id(), INVITATION_FACT_SCHEMA_VERSION, self)
    }

    fn from_bytes(bytes: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        aura_journal::decode_domain_fact(
            INVITATION_FACT_TYPE_ID,
            INVITATION_FACT_SCHEMA_VERSION,
            bytes,
        )
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

        let fact = InvitationFact::from_bytes(binding_data)?;
        if !fact.validate_for_reduction(context_id) {
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
        let fact = InvitationFact::sent_ms(
            test_context_id(),
            "inv-123".to_string(),
            test_authority_id(1),
            test_authority_id(2),
            "guardian".to_string(),
            1234567890,
            Some(1234567890 + 86400000),
            Some("Please be my guardian".to_string()),
        );

        let bytes = fact.to_bytes();
        let restored = InvitationFact::from_bytes(&bytes);
        assert!(restored.is_some());
        assert_eq!(restored.unwrap(), fact);
    }

    #[test]
    fn test_invitation_fact_to_generic() {
        let fact =
            InvitationFact::accepted_ms("inv-456".to_string(), test_authority_id(3), 1234567899);

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

        let fact = InvitationFact::sent_ms(
            test_context_id(),
            "inv-789".to_string(),
            test_authority_id(4),
            test_authority_id(5),
            "contact".to_string(),
            0,
            None,
            None,
        );

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
    fn test_reducer_rejects_context_mismatch() {
        let reducer = InvitationFactReducer;

        let fact = InvitationFact::sent_ms(
            test_context_id(),
            "inv-789".to_string(),
            test_authority_id(4),
            test_authority_id(5),
            "contact".to_string(),
            0,
            None,
            None,
        );

        let other_context = ContextId::new_from_entropy([24u8; 32]);
        let binding = reducer.reduce(other_context, INVITATION_FACT_TYPE_ID, &fact.to_bytes());
        assert!(binding.is_none());
    }

    #[test]
    fn test_binding_key_derivation() {
        let fact = InvitationFact::declined_ms(
            "inv-42".to_string(),
            test_authority_id(4),
            1234,
        );

        let key = fact.binding_key();
        assert_eq!(key.sub_type, "invitation-declined");
        assert_eq!(key.data, b"inv-42".to_vec());
    }

    #[test]
    fn test_reducer_idempotence() {
        let reducer = InvitationFactReducer;
        let context_id = test_context_id();
        let fact = InvitationFact::sent_ms(
            context_id,
            "inv-100".to_string(),
            test_authority_id(1),
            test_authority_id(2),
            "contact".to_string(),
            0,
            None,
            None,
        );

        let bytes = fact.to_bytes();
        let binding1 = reducer.reduce(context_id, INVITATION_FACT_TYPE_ID, &bytes);
        let binding2 = reducer.reduce(context_id, INVITATION_FACT_TYPE_ID, &bytes);
        assert!(binding1.is_some());
        assert!(binding2.is_some());
        let binding1 = binding1.unwrap();
        let binding2 = binding2.unwrap();
        assert_eq!(binding1.binding_type, binding2.binding_type);
        assert_eq!(binding1.context_id, binding2.context_id);
        assert_eq!(binding1.data, binding2.data);
    }

    #[test]
    fn test_invitation_id_extraction() {
        let facts = [
            InvitationFact::sent_ms(
                test_context_id(),
                "inv-1".to_string(),
                test_authority_id(1),
                test_authority_id(2),
                "guardian".to_string(),
                0,
                None,
                None,
            ),
            InvitationFact::accepted_ms("inv-2".to_string(), test_authority_id(3), 0),
            InvitationFact::declined_ms("inv-3".to_string(), test_authority_id(4), 0),
            InvitationFact::cancelled_ms("inv-4".to_string(), test_authority_id(5), 0),
        ];

        assert_eq!(facts[0].invitation_id(), "inv-1");
        assert_eq!(facts[1].invitation_id(), "inv-2");
        assert_eq!(facts[2].invitation_id(), "inv-3");
        assert_eq!(facts[3].invitation_id(), "inv-4");
    }

    #[test]
    fn test_type_id_consistency() {
        let facts = [
            InvitationFact::sent_ms(
                test_context_id(),
                "x".to_string(),
                test_authority_id(1),
                test_authority_id(2),
                "t".to_string(),
                0,
                None,
                None,
            ),
            InvitationFact::accepted_ms("x".to_string(), test_authority_id(3), 0),
            InvitationFact::declined_ms("x".to_string(), test_authority_id(4), 0),
            InvitationFact::cancelled_ms("x".to_string(), test_authority_id(5), 0),
        ];

        for fact in facts {
            assert_eq!(fact.type_id(), INVITATION_FACT_TYPE_ID);
        }
    }

    #[test]
    fn test_timestamp_ms_backward_compat() {
        let sent = InvitationFact::sent_ms(
            test_context_id(),
            "inv".to_string(),
            test_authority_id(1),
            test_authority_id(2),
            "guardian".to_string(),
            1234567890,
            None,
            None,
        );
        assert_eq!(sent.timestamp_ms(), 1234567890);

        let accepted =
            InvitationFact::accepted_ms("inv".to_string(), test_authority_id(1), 1111111111);
        assert_eq!(accepted.timestamp_ms(), 1111111111);

        let declined =
            InvitationFact::declined_ms("inv".to_string(), test_authority_id(1), 2222222222);
        assert_eq!(declined.timestamp_ms(), 2222222222);

        let cancelled =
            InvitationFact::cancelled_ms("inv".to_string(), test_authority_id(1), 3333333333);
        assert_eq!(cancelled.timestamp_ms(), 3333333333);
    }
}
