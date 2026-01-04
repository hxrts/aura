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
//!     InvitationId::new("inv-123"),
//!     sender_id,
//!     receiver_id,
//!     InvitationType::Contact { nickname: None },
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

use crate::InvitationType;
use aura_core::identifiers::{AuthorityId, CeremonyId, ContextId, InvitationId};
use aura_core::threshold::AgreementMode;
use aura_core::time::PhysicalTime;
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};

/// Type identifier for invitation facts
pub const INVITATION_FACT_TYPE_ID: &str = "invitation";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvitationFactKey {
    pub sub_type: &'static str,
    pub data: Vec<u8>,
}

/// Invitation domain fact types
///
/// These facts represent invitation-related state changes in the journal.
/// They are stored as `RelationalFact::Generic` and reduced by `InvitationFactReducer`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(
    type_id = "invitation",
    schema_version = 1,
    context_fn = "context_id_or_default"
)]
#[allow(clippy::large_enum_variant)] // Sent variant contains rich invitation data
pub enum InvitationFact {
    /// Invitation sent from one authority to another
    Sent {
        /// Relational context for the invitation
        context_id: ContextId,
        /// Unique invitation identifier
        invitation_id: InvitationId,
        /// Authority sending the invitation
        sender_id: AuthorityId,
        /// Authority receiving the invitation
        receiver_id: AuthorityId,
        /// Type of invitation: "guardian", "channel", "contact", "device"
        invitation_type: InvitationType,
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
        invitation_id: InvitationId,
        /// Authority accepting the invitation
        acceptor_id: AuthorityId,
        /// Timestamp when invitation was accepted (uses unified time system)
        accepted_at: PhysicalTime,
    },
    /// Invitation declined
    Declined {
        /// Invitation being declined
        invitation_id: InvitationId,
        /// Authority declining the invitation
        decliner_id: AuthorityId,
        /// Timestamp when invitation was declined (uses unified time system)
        declined_at: PhysicalTime,
    },
    /// Invitation cancelled by sender
    Cancelled {
        /// Invitation being cancelled
        invitation_id: InvitationId,
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
        ceremony_id: CeremonyId,
        /// Authority initiating the ceremony
        sender: String,
        /// Agreement mode at initiation (A1)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agreement_mode: Option<AgreementMode>,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },

    /// Acceptance received from acceptor
    CeremonyAcceptanceReceived {
        /// Ceremony identifier
        ceremony_id: CeremonyId,
        /// Agreement mode after acceptance (A2)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agreement_mode: Option<AgreementMode>,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },

    /// Ceremony committed (relationship established)
    CeremonyCommitted {
        /// Ceremony identifier
        ceremony_id: CeremonyId,
        /// Resulting relationship identifier
        relationship_id: String,
        /// Agreement mode after commit (A3)
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agreement_mode: Option<AgreementMode>,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },

    /// Ceremony aborted
    CeremonyAborted {
        /// Ceremony identifier
        ceremony_id: CeremonyId,
        /// Reason for abortion
        reason: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },

    /// Ceremony superseded by a newer ceremony
    ///
    /// Emitted when a new ceremony replaces an existing one. The old ceremony
    /// should stop processing immediately. Supersession propagates via anti-entropy.
    CeremonySuperseded {
        /// The ceremony being superseded (old ceremony)
        superseded_ceremony_id: CeremonyId,
        /// The ceremony that supersedes it (new ceremony)
        superseding_ceremony_id: CeremonyId,
        /// Reason for supersession (e.g., "prestate_stale", "newer_request", "timeout")
        reason: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default, skip_serializing_if = "Option::is_none")]
        trace_id: Option<String>,
        /// Timestamp in milliseconds
        timestamp_ms: u64,
    },
}

impl InvitationFact {
    /// Extract the invitation_id for invitation-scoped facts.
    pub fn invitation_id(&self) -> Option<&InvitationId> {
        match self {
            InvitationFact::Sent { invitation_id, .. }
            | InvitationFact::Accepted { invitation_id, .. }
            | InvitationFact::Declined { invitation_id, .. }
            | InvitationFact::Cancelled { invitation_id, .. } => Some(invitation_id),
            _ => None,
        }
    }

    /// Extract the ceremony_id for ceremony-scoped facts.
    pub fn ceremony_id(&self) -> Option<&CeremonyId> {
        match self {
            InvitationFact::CeremonyInitiated { ceremony_id, .. }
            | InvitationFact::CeremonyAcceptanceReceived { ceremony_id, .. }
            | InvitationFact::CeremonyCommitted { ceremony_id, .. }
            | InvitationFact::CeremonyAborted { ceremony_id, .. } => Some(ceremony_id),
            InvitationFact::CeremonySuperseded {
                superseded_ceremony_id,
                ..
            } => Some(superseded_ceremony_id),
            _ => None,
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
            | InvitationFact::CeremonyAborted { .. }
            | InvitationFact::CeremonySuperseded { .. } => None,
        }
    }

    /// Context ID with a default sentinel for non-context facts.
    pub fn context_id_or_default(&self) -> ContextId {
        self.context_id_opt()
            .unwrap_or_else(|| ContextId::new_from_entropy([0u8; 32]))
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
            InvitationFact::CeremonySuperseded { timestamp_ms, .. } => *timestamp_ms,
        }
    }

    /// Derive the relational binding subtype and key data for this fact.
    pub fn binding_key(&self) -> InvitationFactKey {
        match self {
            InvitationFact::Sent { invitation_id, .. } => InvitationFactKey {
                sub_type: "invitation-sent",
                data: invitation_id.as_str().as_bytes().to_vec(),
            },
            InvitationFact::Accepted { invitation_id, .. } => InvitationFactKey {
                sub_type: "invitation-accepted",
                data: invitation_id.as_str().as_bytes().to_vec(),
            },
            InvitationFact::Declined { invitation_id, .. } => InvitationFactKey {
                sub_type: "invitation-declined",
                data: invitation_id.as_str().as_bytes().to_vec(),
            },
            InvitationFact::Cancelled { invitation_id, .. } => InvitationFactKey {
                sub_type: "invitation-cancelled",
                data: invitation_id.as_str().as_bytes().to_vec(),
            },
            InvitationFact::CeremonyInitiated { ceremony_id, .. } => InvitationFactKey {
                sub_type: "ceremony-initiated",
                data: ceremony_id.as_str().as_bytes().to_vec(),
            },
            InvitationFact::CeremonyAcceptanceReceived { ceremony_id, .. } => InvitationFactKey {
                sub_type: "ceremony-acceptance-received",
                data: ceremony_id.as_str().as_bytes().to_vec(),
            },
            InvitationFact::CeremonyCommitted { ceremony_id, .. } => InvitationFactKey {
                sub_type: "ceremony-committed",
                data: ceremony_id.as_str().as_bytes().to_vec(),
            },
            InvitationFact::CeremonyAborted { ceremony_id, .. } => InvitationFactKey {
                sub_type: "ceremony-aborted",
                data: ceremony_id.as_str().as_bytes().to_vec(),
            },
            InvitationFact::CeremonySuperseded {
                superseded_ceremony_id,
                superseding_ceremony_id,
                ..
            } => {
                // Key includes both IDs for unique identification
                let mut data = superseded_ceremony_id.as_str().as_bytes().to_vec();
                data.extend_from_slice(b":");
                data.extend_from_slice(superseding_ceremony_id.as_str().as_bytes());
                InvitationFactKey {
                    sub_type: "ceremony-superseded",
                    data,
                }
            }
        }
    }

    /// Create a Sent fact with millisecond timestamps (backward compatibility)
    #[allow(clippy::too_many_arguments)]
    pub fn sent_ms(
        context_id: ContextId,
        invitation_id: InvitationId,
        sender_id: AuthorityId,
        receiver_id: AuthorityId,
        invitation_type: InvitationType,
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
        invitation_id: InvitationId,
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
        invitation_id: InvitationId,
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
        invitation_id: InvitationId,
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
            InvitationId::new("inv-123"),
            test_authority_id(1),
            test_authority_id(2),
            InvitationType::Guardian {
                subject_authority: test_authority_id(9),
            },
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
        let fact = InvitationFact::accepted_ms(
            InvitationId::new("inv-456"),
            test_authority_id(3),
            1234567899,
        );

        let generic = fact.to_generic();

        let aura_journal::RelationalFact::Generic { binding_type, binding_data, .. } = generic else {
            panic!("Expected Generic variant");
        };
        assert_eq!(binding_type, INVITATION_FACT_TYPE_ID);
        let restored = InvitationFact::from_bytes(&binding_data);
        assert!(restored.is_some());
    }

    #[test]
    fn test_invitation_fact_reducer() {
        let reducer = InvitationFactReducer;
        assert_eq!(reducer.handles_type(), INVITATION_FACT_TYPE_ID);

        let fact = InvitationFact::sent_ms(
            test_context_id(),
            InvitationId::new("inv-789"),
            test_authority_id(4),
            test_authority_id(5),
            InvitationType::Contact { nickname: None },
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
            InvitationId::new("inv-789"),
            test_authority_id(4),
            test_authority_id(5),
            InvitationType::Contact { nickname: None },
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
        let fact =
            InvitationFact::declined_ms(InvitationId::new("inv-42"), test_authority_id(4), 1234);

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
            InvitationId::new("inv-100"),
            test_authority_id(1),
            test_authority_id(2),
            InvitationType::Contact { nickname: None },
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
                InvitationId::new("inv-1"),
                test_authority_id(1),
                test_authority_id(2),
                InvitationType::Guardian {
                    subject_authority: test_authority_id(9),
                },
                0,
                None,
                None,
            ),
            InvitationFact::accepted_ms(InvitationId::new("inv-2"), test_authority_id(3), 0),
            InvitationFact::declined_ms(InvitationId::new("inv-3"), test_authority_id(4), 0),
            InvitationFact::cancelled_ms(InvitationId::new("inv-4"), test_authority_id(5), 0),
        ];

        assert_eq!(facts[0].invitation_id().unwrap().as_str(), "inv-1");
        assert_eq!(facts[1].invitation_id().unwrap().as_str(), "inv-2");
        assert_eq!(facts[2].invitation_id().unwrap().as_str(), "inv-3");
        assert_eq!(facts[3].invitation_id().unwrap().as_str(), "inv-4");
    }

    #[test]
    fn test_type_id_consistency() {
        let facts = [
            InvitationFact::sent_ms(
                test_context_id(),
                InvitationId::new("x"),
                test_authority_id(1),
                test_authority_id(2),
                InvitationType::Contact { nickname: None },
                0,
                None,
                None,
            ),
            InvitationFact::accepted_ms(InvitationId::new("x"), test_authority_id(3), 0),
            InvitationFact::declined_ms(InvitationId::new("x"), test_authority_id(4), 0),
            InvitationFact::cancelled_ms(InvitationId::new("x"), test_authority_id(5), 0),
        ];

        for fact in facts {
            assert_eq!(fact.type_id(), INVITATION_FACT_TYPE_ID);
        }
    }

    #[test]
    fn test_timestamp_ms_backward_compat() {
        let sent = InvitationFact::sent_ms(
            test_context_id(),
            InvitationId::new("inv"),
            test_authority_id(1),
            test_authority_id(2),
            InvitationType::Guardian {
                subject_authority: test_authority_id(9),
            },
            1234567890,
            None,
            None,
        );
        assert_eq!(sent.timestamp_ms(), 1234567890);

        let accepted =
            InvitationFact::accepted_ms(InvitationId::new("inv"), test_authority_id(1), 1111111111);
        assert_eq!(accepted.timestamp_ms(), 1111111111);

        let declined =
            InvitationFact::declined_ms(InvitationId::new("inv"), test_authority_id(1), 2222222222);
        assert_eq!(declined.timestamp_ms(), 2222222222);

        let cancelled = InvitationFact::cancelled_ms(
            InvitationId::new("inv"),
            test_authority_id(1),
            3333333333,
        );
        assert_eq!(cancelled.timestamp_ms(), 3333333333);
    }
}
