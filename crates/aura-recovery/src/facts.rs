//! Recovery domain facts
//!
//! This module defines recovery-specific fact types that implement the `DomainFact`
//! trait from `aura-journal`. These facts are stored as `RelationalFact::Generic`
//! in the journal and reduced using the `RecoveryFactReducer`.
//!
//! # Architecture
//!
//! Following the Open/Closed Principle:
//! - `aura-journal` provides the generic fact infrastructure
//! - `aura-recovery` defines domain-specific fact types without modifying `aura-journal`
//! - Runtime registers `RecoveryFactReducer` with the `FactRegistry`
//!
//! # Relationship to Protocol-Level Facts
//!
//! Note that `GuardianBinding` and `RecoveryGrant` are protocol-level facts defined
//! directly in `aura-journal/src/fact.rs` as `RelationalFact` variants. These core
//! binding facts are NOT duplicated here.
//!
//! This module defines *lifecycle* facts that track the recovery protocol's progress:
//! - Guardian setup ceremony phases
//! - Membership change proposals and votes
//! - Key recovery initiation and completion
//!
//! # Example
//!
//! ```ignore
//! use aura_recovery::facts::{RecoveryFact, RecoveryFactReducer, RECOVERY_FACT_TYPE_ID};
//! use aura_journal::{FactRegistry, DomainFact};
//!
//! // Create a recovery fact
//! let fact = RecoveryFact::GuardianSetupInitiated {
//!     context_id,
//!     initiator_id,
//!     guardian_ids: vec![guardian1, guardian2, guardian3],
//!     threshold: 2,
//!     initiated_at_ms: 1234567890,
//! };
//!
//! // Convert to generic for storage
//! let generic = fact.to_generic();
//!
//! // Register reducer at runtime
//! registry.register::<RecoveryFact>(RECOVERY_FACT_TYPE_ID, Box::new(RecoveryFactReducer));
//! ```

use aura_core::{
    hash,
    identifiers::{AuthorityId, ContextId},
    Hash32,
};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use serde::{Deserialize, Serialize};

/// Type identifier for recovery facts
pub const RECOVERY_FACT_TYPE_ID: &str = "recovery";

/// Recovery domain fact types
///
/// These facts represent recovery-related state changes in the journal.
/// They track the lifecycle of recovery operations (setup, membership, key recovery).
///
/// **Note**: Core binding facts (`GuardianBinding`, `RecoveryGrant`) are protocol-level
/// facts in `aura-journal`. This enum tracks operational lifecycle only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecoveryFact {
    // ========================================================================
    // Guardian Setup Lifecycle
    // ========================================================================
    /// Guardian setup ceremony initiated
    GuardianSetupInitiated {
        /// Relational context for the setup
        context_id: ContextId,
        /// Authority initiating the setup
        initiator_id: AuthorityId,
        /// Target guardians for the setup
        guardian_ids: Vec<AuthorityId>,
        /// Required threshold for recovery
        threshold: u16,
        /// Timestamp when setup was initiated (ms since epoch)
        initiated_at_ms: u64,
    },

    /// Invitation sent to a guardian
    GuardianInvitationSent {
        /// Relational context for the setup
        context_id: ContextId,
        /// Guardian receiving the invitation
        guardian_id: AuthorityId,
        /// Hash of the invitation details
        invitation_hash: Hash32,
        /// Timestamp when invitation was sent (ms since epoch)
        sent_at_ms: u64,
    },

    /// Guardian accepted the invitation
    GuardianAccepted {
        /// Relational context for the setup
        context_id: ContextId,
        /// Guardian that accepted
        guardian_id: AuthorityId,
        /// Timestamp when accepted (ms since epoch)
        accepted_at_ms: u64,
    },

    /// Guardian declined the invitation
    GuardianDeclined {
        /// Relational context for the setup
        context_id: ContextId,
        /// Guardian that declined
        guardian_id: AuthorityId,
        /// Timestamp when declined (ms since epoch)
        declined_at_ms: u64,
    },

    /// Guardian setup completed successfully
    GuardianSetupCompleted {
        /// Relational context for the setup
        context_id: ContextId,
        /// Guardians that were bound
        guardian_ids: Vec<AuthorityId>,
        /// Final threshold for recovery
        threshold: u16,
        /// Timestamp when setup completed (ms since epoch)
        completed_at_ms: u64,
    },

    /// Guardian setup failed
    GuardianSetupFailed {
        /// Relational context for the setup
        context_id: ContextId,
        /// Reason for failure
        reason: String,
        /// Timestamp when setup failed (ms since epoch)
        failed_at_ms: u64,
    },

    // ========================================================================
    // Membership Change Lifecycle
    // ========================================================================
    /// Membership change proposed
    MembershipChangeProposed {
        /// Relational context for the membership
        context_id: ContextId,
        /// Authority proposing the change
        proposer_id: AuthorityId,
        /// Type of change being proposed
        change_type: MembershipChangeType,
        /// Hash of the proposal details
        proposal_hash: Hash32,
        /// Timestamp when proposed (ms since epoch)
        proposed_at_ms: u64,
    },

    /// Vote cast on membership change
    MembershipVoteCast {
        /// Relational context for the membership
        context_id: ContextId,
        /// Authority casting the vote
        voter_id: AuthorityId,
        /// Hash of the proposal being voted on
        proposal_hash: Hash32,
        /// Whether the vote is in favor
        approved: bool,
        /// Timestamp when vote was cast (ms since epoch)
        voted_at_ms: u64,
    },

    /// Membership change completed
    MembershipChangeCompleted {
        /// Relational context for the membership
        context_id: ContextId,
        /// Hash of the completed proposal
        proposal_hash: Hash32,
        /// New guardian set after the change
        new_guardian_ids: Vec<AuthorityId>,
        /// New threshold after the change
        new_threshold: u16,
        /// Timestamp when change completed (ms since epoch)
        completed_at_ms: u64,
    },

    /// Membership change rejected
    MembershipChangeRejected {
        /// Relational context for the membership
        context_id: ContextId,
        /// Hash of the rejected proposal
        proposal_hash: Hash32,
        /// Reason for rejection
        reason: String,
        /// Timestamp when rejected (ms since epoch)
        rejected_at_ms: u64,
    },

    // ========================================================================
    // Key Recovery Lifecycle
    // ========================================================================
    /// Key recovery initiated
    RecoveryInitiated {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Account requesting recovery
        account_id: AuthorityId,
        /// Hash of the recovery request
        request_hash: Hash32,
        /// Timestamp when recovery was initiated (ms since epoch)
        initiated_at_ms: u64,
    },

    /// Recovery share submitted by a guardian
    RecoveryShareSubmitted {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Guardian submitting the share
        guardian_id: AuthorityId,
        /// Hash of the share (not the share itself)
        share_hash: Hash32,
        /// Timestamp when share was submitted (ms since epoch)
        submitted_at_ms: u64,
    },

    /// Dispute filed during recovery dispute window
    RecoveryDisputeFiled {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Authority filing the dispute
        disputer_id: AuthorityId,
        /// Reason for the dispute
        reason: String,
        /// Timestamp when dispute was filed (ms since epoch)
        filed_at_ms: u64,
    },

    /// Recovery completed successfully
    RecoveryCompleted {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Account that was recovered
        account_id: AuthorityId,
        /// Hash of the recovery evidence
        evidence_hash: Hash32,
        /// Timestamp when recovery completed (ms since epoch)
        completed_at_ms: u64,
    },

    /// Recovery failed
    RecoveryFailed {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Account that attempted recovery
        account_id: AuthorityId,
        /// Reason for failure
        reason: String,
        /// Timestamp when recovery failed (ms since epoch)
        failed_at_ms: u64,
    },
}

/// Type of membership change being proposed
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MembershipChangeType {
    /// Add a new guardian
    AddGuardian {
        /// Guardian to add
        guardian_id: AuthorityId,
    },
    /// Remove an existing guardian
    RemoveGuardian {
        /// Guardian to remove
        guardian_id: AuthorityId,
    },
    /// Update the recovery threshold
    UpdateThreshold {
        /// New threshold value
        new_threshold: u16,
    },
}

impl RecoveryFact {
    /// Extract the context_id from any variant
    pub fn get_context_id(&self) -> ContextId {
        match self {
            // Guardian setup
            RecoveryFact::GuardianSetupInitiated { context_id, .. } => *context_id,
            RecoveryFact::GuardianInvitationSent { context_id, .. } => *context_id,
            RecoveryFact::GuardianAccepted { context_id, .. } => *context_id,
            RecoveryFact::GuardianDeclined { context_id, .. } => *context_id,
            RecoveryFact::GuardianSetupCompleted { context_id, .. } => *context_id,
            RecoveryFact::GuardianSetupFailed { context_id, .. } => *context_id,
            // Membership change
            RecoveryFact::MembershipChangeProposed { context_id, .. } => *context_id,
            RecoveryFact::MembershipVoteCast { context_id, .. } => *context_id,
            RecoveryFact::MembershipChangeCompleted { context_id, .. } => *context_id,
            RecoveryFact::MembershipChangeRejected { context_id, .. } => *context_id,
            // Key recovery
            RecoveryFact::RecoveryInitiated { context_id, .. } => *context_id,
            RecoveryFact::RecoveryShareSubmitted { context_id, .. } => *context_id,
            RecoveryFact::RecoveryDisputeFiled { context_id, .. } => *context_id,
            RecoveryFact::RecoveryCompleted { context_id, .. } => *context_id,
            RecoveryFact::RecoveryFailed { context_id, .. } => *context_id,
        }
    }

    /// Get the sub-type string for this fact variant
    pub fn sub_type(&self) -> &'static str {
        match self {
            RecoveryFact::GuardianSetupInitiated { .. } => "guardian-setup-initiated",
            RecoveryFact::GuardianInvitationSent { .. } => "guardian-invitation-sent",
            RecoveryFact::GuardianAccepted { .. } => "guardian-accepted",
            RecoveryFact::GuardianDeclined { .. } => "guardian-declined",
            RecoveryFact::GuardianSetupCompleted { .. } => "guardian-setup-completed",
            RecoveryFact::GuardianSetupFailed { .. } => "guardian-setup-failed",
            RecoveryFact::MembershipChangeProposed { .. } => "membership-change-proposed",
            RecoveryFact::MembershipVoteCast { .. } => "membership-vote-cast",
            RecoveryFact::MembershipChangeCompleted { .. } => "membership-change-completed",
            RecoveryFact::MembershipChangeRejected { .. } => "membership-change-rejected",
            RecoveryFact::RecoveryInitiated { .. } => "recovery-initiated",
            RecoveryFact::RecoveryShareSubmitted { .. } => "recovery-share-submitted",
            RecoveryFact::RecoveryDisputeFiled { .. } => "recovery-dispute-filed",
            RecoveryFact::RecoveryCompleted { .. } => "recovery-completed",
            RecoveryFact::RecoveryFailed { .. } => "recovery-failed",
        }
    }
}

impl DomainFact for RecoveryFact {
    fn type_id(&self) -> &'static str {
        RECOVERY_FACT_TYPE_ID
    }

    fn context_id(&self) -> ContextId {
        self.get_context_id()
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

/// Reducer for recovery facts
///
/// Converts recovery facts to relational bindings during journal reduction.
pub struct RecoveryFactReducer;

impl FactReducer for RecoveryFactReducer {
    fn handles_type(&self) -> &'static str {
        RECOVERY_FACT_TYPE_ID
    }

    fn reduce(
        &self,
        context_id: ContextId,
        binding_type: &str,
        binding_data: &[u8],
    ) -> Option<RelationalBinding> {
        if binding_type != RECOVERY_FACT_TYPE_ID {
            return None;
        }

        let fact: RecoveryFact = serde_json::from_slice(binding_data).ok()?;
        let sub_type = fact.sub_type().to_string();

        // For the binding data, we include key identifiers based on fact type
        let data = match &fact {
            RecoveryFact::GuardianSetupInitiated { initiator_id, .. } => {
                initiator_id.to_bytes().to_vec()
            }
            RecoveryFact::GuardianInvitationSent { guardian_id, .. } => {
                guardian_id.to_bytes().to_vec()
            }
            RecoveryFact::GuardianAccepted { guardian_id, .. } => guardian_id.to_bytes().to_vec(),
            RecoveryFact::GuardianDeclined { guardian_id, .. } => guardian_id.to_bytes().to_vec(),
            RecoveryFact::GuardianSetupCompleted { .. } => Vec::new(),
            RecoveryFact::GuardianSetupFailed { .. } => Vec::new(),
            RecoveryFact::MembershipChangeProposed { proposal_hash, .. } => {
                proposal_hash.0.to_vec()
            }
            RecoveryFact::MembershipVoteCast {
                proposal_hash,
                voter_id,
                ..
            } => {
                let mut data = proposal_hash.0.to_vec();
                data.extend_from_slice(&voter_id.to_bytes());
                data
            }
            RecoveryFact::MembershipChangeCompleted { proposal_hash, .. } => {
                proposal_hash.0.to_vec()
            }
            RecoveryFact::MembershipChangeRejected { proposal_hash, .. } => {
                proposal_hash.0.to_vec()
            }
            RecoveryFact::RecoveryInitiated {
                account_id,
                request_hash,
                ..
            } => {
                let mut data = account_id.to_bytes().to_vec();
                data.extend_from_slice(&request_hash.0);
                data
            }
            RecoveryFact::RecoveryShareSubmitted {
                guardian_id,
                share_hash,
                ..
            } => {
                let mut data = guardian_id.to_bytes().to_vec();
                data.extend_from_slice(&share_hash.0);
                data
            }
            RecoveryFact::RecoveryDisputeFiled { disputer_id, .. } => {
                disputer_id.to_bytes().to_vec()
            }
            RecoveryFact::RecoveryCompleted {
                account_id,
                evidence_hash,
                ..
            } => {
                let mut data = account_id.to_bytes().to_vec();
                data.extend_from_slice(&evidence_hash.0);
                data
            }
            RecoveryFact::RecoveryFailed { account_id, .. } => account_id.to_bytes().to_vec(),
        };

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(sub_type),
            context_id,
            data,
        })
    }
}

// ============================================================================
// Fact Emission Helpers
// ============================================================================

/// Helper for emitting recovery facts to the journal.
///
/// This struct provides convenience methods for serializing `RecoveryFact`
/// instances and generating fact keys. Coordinators can use this to
/// emit facts via `JournalEffects::insert_with_context`.
///
/// # Usage Pattern
///
/// The recommended pattern for emitting facts is:
///
/// ```ignore
/// use aura_recovery::facts::{RecoveryFact, RecoveryFactEmitter, RECOVERY_FACT_TYPE_ID};
///
/// async fn emit_fact<E: RecoveryEffects>(effects: &E, fact: RecoveryFact) -> AuraResult<()> {
///     // Get timestamp from effects (respects effect system)
///     let timestamp = effects.now_physical().await;
///
///     // Get journal and insert fact
///     let mut journal = effects.get_journal().await?;
///     journal.facts.insert_with_context(
///         RecoveryFactEmitter::fact_key(&fact),
///         aura_core::FactValue::Bytes(fact.to_bytes()),
///         fact.context_id().to_string(),
///         timestamp,
///         None,
///     );
///     effects.persist_journal(&journal).await
/// }
/// ```
pub struct RecoveryFactEmitter;

impl RecoveryFactEmitter {
    /// Generate a unique key for a recovery fact.
    ///
    /// Keys are formatted as `{type_id}:{sub_type}:{context_id}:{content_hash}` for uniqueness.
    /// Uses content hash to ensure different facts with the same context get unique keys.
    pub fn fact_key(fact: &RecoveryFact) -> String {
        let content_hash = hash::hash(&fact.to_bytes());
        format!(
            "{}:{}:{}:{}",
            RECOVERY_FACT_TYPE_ID,
            fact.sub_type(),
            fact.context_id(),
            hex::encode(&content_hash[..8])
        )
    }

    /// Generate a deterministic key for a recovery fact (for idempotent operations).
    ///
    /// Use this when you need the same fact to always have the same key,
    /// such as for setup completion or membership finalization.
    pub fn deterministic_key(fact: &RecoveryFact, discriminator: &str) -> String {
        format!(
            "{}:{}:{}",
            RECOVERY_FACT_TYPE_ID,
            fact.sub_type(),
            discriminator
        )
    }

    /// Serialize a recovery fact to bytes for storage.
    ///
    /// This is equivalent to calling `fact.to_bytes()` but provided
    /// here for consistency with the emitter pattern.
    pub fn to_bytes(fact: &RecoveryFact) -> Vec<u8> {
        fact.to_bytes()
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

    fn test_hash(seed: u8) -> Hash32 {
        Hash32([seed; 32])
    }

    #[test]
    fn test_recovery_fact_serialization() {
        let fact = RecoveryFact::GuardianSetupInitiated {
            context_id: test_context_id(),
            initiator_id: test_authority_id(1),
            guardian_ids: vec![
                test_authority_id(2),
                test_authority_id(3),
                test_authority_id(4),
            ],
            threshold: 2,
            initiated_at_ms: 1234567890,
        };

        let bytes = fact.to_bytes();
        let restored = RecoveryFact::from_bytes(&bytes);
        assert!(restored.is_some());
        assert_eq!(restored.unwrap(), fact);
    }

    #[test]
    fn test_recovery_fact_to_generic() {
        let fact = RecoveryFact::RecoveryCompleted {
            context_id: test_context_id(),
            account_id: test_authority_id(1),
            evidence_hash: test_hash(99),
            completed_at_ms: 1234567899,
        };

        let generic = fact.to_generic();

        if let aura_journal::RelationalFact::Generic {
            binding_type,
            binding_data,
            ..
        } = generic
        {
            assert_eq!(binding_type, RECOVERY_FACT_TYPE_ID);
            let restored = RecoveryFact::from_bytes(&binding_data);
            assert!(restored.is_some());
        } else {
            panic!("Expected Generic variant");
        }
    }

    #[test]
    fn test_recovery_fact_reducer() {
        let reducer = RecoveryFactReducer;
        assert_eq!(reducer.handles_type(), RECOVERY_FACT_TYPE_ID);

        let fact = RecoveryFact::GuardianAccepted {
            context_id: test_context_id(),
            guardian_id: test_authority_id(5),
            accepted_at_ms: 1234567890,
        };

        let bytes = fact.to_bytes();
        let binding = reducer.reduce(test_context_id(), RECOVERY_FACT_TYPE_ID, &bytes);

        assert!(binding.is_some());
        let binding = binding.unwrap();
        assert!(matches!(
            binding.binding_type,
            RelationalBindingType::Generic(ref s) if s == "guardian-accepted"
        ));
    }

    #[test]
    fn test_context_id_extraction() {
        let ctx = test_context_id();
        let facts = vec![
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: test_authority_id(1),
                guardian_ids: vec![],
                threshold: 2,
                initiated_at_ms: 0,
            },
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: test_authority_id(2),
                request_hash: test_hash(1),
                initiated_at_ms: 0,
            },
            RecoveryFact::MembershipChangeProposed {
                context_id: ctx,
                proposer_id: test_authority_id(3),
                change_type: MembershipChangeType::UpdateThreshold { new_threshold: 3 },
                proposal_hash: test_hash(2),
                proposed_at_ms: 0,
            },
        ];

        for fact in facts {
            assert_eq!(fact.get_context_id(), ctx);
            assert_eq!(fact.context_id(), ctx);
        }
    }

    #[test]
    fn test_type_id_consistency() {
        let ctx = test_context_id();
        let facts: Vec<RecoveryFact> = vec![
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: test_authority_id(1),
                guardian_ids: vec![],
                threshold: 2,
                initiated_at_ms: 0,
            },
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: test_authority_id(2),
                accepted_at_ms: 0,
            },
            RecoveryFact::RecoveryCompleted {
                context_id: ctx,
                account_id: test_authority_id(3),
                evidence_hash: test_hash(1),
                completed_at_ms: 0,
            },
        ];

        for fact in facts {
            assert_eq!(fact.type_id(), RECOVERY_FACT_TYPE_ID);
        }
    }

    #[test]
    fn test_sub_type_uniqueness() {
        let ctx = test_context_id();
        let sub_types: Vec<&str> = vec![
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: test_authority_id(1),
                guardian_ids: vec![],
                threshold: 2,
                initiated_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::GuardianInvitationSent {
                context_id: ctx,
                guardian_id: test_authority_id(1),
                invitation_hash: test_hash(1),
                sent_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: test_authority_id(1),
                accepted_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::GuardianDeclined {
                context_id: ctx,
                guardian_id: test_authority_id(1),
                declined_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::GuardianSetupCompleted {
                context_id: ctx,
                guardian_ids: vec![],
                threshold: 2,
                completed_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::GuardianSetupFailed {
                context_id: ctx,
                reason: "test".to_string(),
                failed_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::MembershipChangeProposed {
                context_id: ctx,
                proposer_id: test_authority_id(1),
                change_type: MembershipChangeType::UpdateThreshold { new_threshold: 3 },
                proposal_hash: test_hash(1),
                proposed_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::MembershipVoteCast {
                context_id: ctx,
                voter_id: test_authority_id(1),
                proposal_hash: test_hash(1),
                approved: true,
                voted_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::MembershipChangeCompleted {
                context_id: ctx,
                proposal_hash: test_hash(1),
                new_guardian_ids: vec![],
                new_threshold: 2,
                completed_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::MembershipChangeRejected {
                context_id: ctx,
                proposal_hash: test_hash(1),
                reason: "test".to_string(),
                rejected_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: test_authority_id(1),
                request_hash: test_hash(1),
                initiated_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::RecoveryShareSubmitted {
                context_id: ctx,
                guardian_id: test_authority_id(1),
                share_hash: test_hash(1),
                submitted_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::RecoveryDisputeFiled {
                context_id: ctx,
                disputer_id: test_authority_id(1),
                reason: "test".to_string(),
                filed_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::RecoveryCompleted {
                context_id: ctx,
                account_id: test_authority_id(1),
                evidence_hash: test_hash(1),
                completed_at_ms: 0,
            }
            .sub_type(),
            RecoveryFact::RecoveryFailed {
                context_id: ctx,
                account_id: test_authority_id(1),
                reason: "test".to_string(),
                failed_at_ms: 0,
            }
            .sub_type(),
        ];

        // All sub-types should be unique
        let unique_count = sub_types
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();
        assert_eq!(
            unique_count,
            sub_types.len(),
            "All sub-types must be unique"
        );
    }

    #[test]
    fn test_membership_change_type_serialization() {
        let changes = vec![
            MembershipChangeType::AddGuardian {
                guardian_id: test_authority_id(1),
            },
            MembershipChangeType::RemoveGuardian {
                guardian_id: test_authority_id(2),
            },
            MembershipChangeType::UpdateThreshold { new_threshold: 3 },
        ];

        for change in changes {
            let bytes = serde_json::to_vec(&change).unwrap();
            let restored: MembershipChangeType = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(restored, change);
        }
    }
}
