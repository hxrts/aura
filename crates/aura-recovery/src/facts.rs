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
//!     trace_id: None,
//!     guardian_ids: vec![guardian1, guardian2, guardian3],
//!     threshold: 2,
//!     initiated_at: PhysicalTime { ts_ms: 1234567890, uncertainty: None },
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
    time::PhysicalTime,
    Hash32,
};
use aura_journal::{
    reduction::{RelationalBinding, RelationalBindingType},
    DomainFact, FactReducer,
};
use aura_macros::DomainFact;
use serde::{Deserialize, Serialize};

/// Type identifier for recovery facts
pub const RECOVERY_FACT_TYPE_ID: &str = "recovery";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryFactKey {
    pub sub_type: &'static str,
    pub data: Vec<u8>,
}

/// Recovery domain fact types
///
/// These facts represent recovery-related state changes in the journal.
/// They track the lifecycle of recovery operations (setup, membership, key recovery).
///
/// **Note**: Core binding facts (`GuardianBinding`, `RecoveryGrant`) are protocol-level
/// facts in `aura-journal`. This enum tracks operational lifecycle only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, DomainFact)]
#[domain_fact(
    type_id = "recovery",
    schema_version = 1,
    context_fn = "get_context_id"
)]
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
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Target guardians for the setup
        guardian_ids: Vec<AuthorityId>,
        /// Required threshold for recovery
        threshold: u16,
        /// Timestamp when setup was initiated (uses unified time system)
        initiated_at: PhysicalTime,
    },

    /// Invitation sent to a guardian
    GuardianInvitationSent {
        /// Relational context for the setup
        context_id: ContextId,
        /// Guardian receiving the invitation
        guardian_id: AuthorityId,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Hash of the invitation details
        invitation_hash: Hash32,
        /// Timestamp when invitation was sent (uses unified time system)
        sent_at: PhysicalTime,
    },

    /// Guardian accepted the invitation
    GuardianAccepted {
        /// Relational context for the setup
        context_id: ContextId,
        /// Guardian that accepted
        guardian_id: AuthorityId,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Timestamp when accepted (uses unified time system)
        accepted_at: PhysicalTime,
    },

    /// Guardian declined the invitation
    GuardianDeclined {
        /// Relational context for the setup
        context_id: ContextId,
        /// Guardian that declined
        guardian_id: AuthorityId,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Timestamp when declined (uses unified time system)
        declined_at: PhysicalTime,
    },

    /// Guardian setup completed successfully
    GuardianSetupCompleted {
        /// Relational context for the setup
        context_id: ContextId,
        /// Guardians that were bound
        guardian_ids: Vec<AuthorityId>,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Final threshold for recovery
        threshold: u16,
        /// Timestamp when setup completed (uses unified time system)
        completed_at: PhysicalTime,
    },

    /// Guardian setup failed
    GuardianSetupFailed {
        /// Relational context for the setup
        context_id: ContextId,
        /// Reason for failure
        reason: String,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Timestamp when setup failed (uses unified time system)
        failed_at: PhysicalTime,
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
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Type of change being proposed
        change_type: MembershipChangeType,
        /// Hash of the proposal details
        proposal_hash: Hash32,
        /// Timestamp when proposed (uses unified time system)
        proposed_at: PhysicalTime,
    },

    /// Vote cast on membership change
    MembershipVoteCast {
        /// Relational context for the membership
        context_id: ContextId,
        /// Authority casting the vote
        voter_id: AuthorityId,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Hash of the proposal being voted on
        proposal_hash: Hash32,
        /// Whether the vote is in favor
        approved: bool,
        /// Timestamp when vote was cast (uses unified time system)
        voted_at: PhysicalTime,
    },

    /// Membership change completed
    MembershipChangeCompleted {
        /// Relational context for the membership
        context_id: ContextId,
        /// Hash of the completed proposal
        proposal_hash: Hash32,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// New guardian set after the change
        new_guardian_ids: Vec<AuthorityId>,
        /// New threshold after the change
        new_threshold: u16,
        /// Timestamp when change completed (uses unified time system)
        completed_at: PhysicalTime,
    },

    /// Membership change rejected
    MembershipChangeRejected {
        /// Relational context for the membership
        context_id: ContextId,
        /// Hash of the rejected proposal
        proposal_hash: Hash32,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Reason for rejection
        reason: String,
        /// Timestamp when rejected (uses unified time system)
        rejected_at: PhysicalTime,
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
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Hash of the recovery request
        request_hash: Hash32,
        /// Timestamp when recovery was initiated (uses unified time system)
        initiated_at: PhysicalTime,
    },

    /// Recovery share submitted by a guardian
    RecoveryShareSubmitted {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Guardian submitting the share
        guardian_id: AuthorityId,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Hash of the share (not the share itself)
        share_hash: Hash32,
        /// Timestamp when share was submitted (uses unified time system)
        submitted_at: PhysicalTime,
    },

    /// Recovery approvals reached quorum (approval ceremony complete)
    RecoveryApproved {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Account being recovered
        account_id: AuthorityId,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Hash of the approval set
        approvals_hash: Hash32,
        /// Timestamp when approvals reached quorum (uses unified time system)
        approved_at: PhysicalTime,
    },

    /// Dispute filed during recovery dispute window
    RecoveryDisputeFiled {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Authority filing the dispute
        disputer_id: AuthorityId,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Reason for the dispute
        reason: String,
        /// Timestamp when dispute was filed (uses unified time system)
        filed_at: PhysicalTime,
    },

    /// Recovery completed successfully
    RecoveryCompleted {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Account that was recovered
        account_id: AuthorityId,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Hash of the recovery evidence
        evidence_hash: Hash32,
        /// Timestamp when recovery completed (uses unified time system)
        completed_at: PhysicalTime,
    },

    /// Recovery failed
    RecoveryFailed {
        /// Relational context for the recovery
        context_id: ContextId,
        /// Account that attempted recovery
        account_id: AuthorityId,
        /// Optional trace identifier for ceremony correlation
        #[serde(default)]
        trace_id: Option<String>,
        /// Reason for failure
        reason: String,
        /// Timestamp when recovery failed (uses unified time system)
        failed_at: PhysicalTime,
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
            RecoveryFact::RecoveryApproved { context_id, .. } => *context_id,
            RecoveryFact::RecoveryDisputeFiled { context_id, .. } => *context_id,
            RecoveryFact::RecoveryCompleted { context_id, .. } => *context_id,
            RecoveryFact::RecoveryFailed { context_id, .. } => *context_id,
        }
    }

    /// Validate that this fact can be reduced under the provided context.
    pub fn validate_for_reduction(&self, context_id: ContextId) -> bool {
        self.get_context_id() == context_id
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
            RecoveryFact::RecoveryApproved { .. } => "recovery-approved",
            RecoveryFact::RecoveryDisputeFiled { .. } => "recovery-dispute-filed",
            RecoveryFact::RecoveryCompleted { .. } => "recovery-completed",
            RecoveryFact::RecoveryFailed { .. } => "recovery-failed",
        }
    }

    /// Derive the relational binding key data for this fact.
    pub fn binding_key(&self) -> RecoveryFactKey {
        let data = match self {
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
            RecoveryFact::RecoveryApproved {
                account_id,
                approvals_hash,
                ..
            } => {
                let mut data = account_id.to_bytes().to_vec();
                data.extend_from_slice(&approvals_hash.0);
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

        RecoveryFactKey {
            sub_type: self.sub_type(),
            data,
        }
    }

    /// Get the timestamp for this fact in milliseconds (backward compatibility)
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            // Guardian setup
            RecoveryFact::GuardianSetupInitiated { initiated_at, .. } => initiated_at.ts_ms,
            RecoveryFact::GuardianInvitationSent { sent_at, .. } => sent_at.ts_ms,
            RecoveryFact::GuardianAccepted { accepted_at, .. } => accepted_at.ts_ms,
            RecoveryFact::GuardianDeclined { declined_at, .. } => declined_at.ts_ms,
            RecoveryFact::GuardianSetupCompleted { completed_at, .. } => completed_at.ts_ms,
            RecoveryFact::GuardianSetupFailed { failed_at, .. } => failed_at.ts_ms,
            // Membership change
            RecoveryFact::MembershipChangeProposed { proposed_at, .. } => proposed_at.ts_ms,
            RecoveryFact::MembershipVoteCast { voted_at, .. } => voted_at.ts_ms,
            RecoveryFact::MembershipChangeCompleted { completed_at, .. } => completed_at.ts_ms,
            RecoveryFact::MembershipChangeRejected { rejected_at, .. } => rejected_at.ts_ms,
            // Key recovery
            RecoveryFact::RecoveryInitiated { initiated_at, .. } => initiated_at.ts_ms,
            RecoveryFact::RecoveryShareSubmitted { submitted_at, .. } => submitted_at.ts_ms,
            RecoveryFact::RecoveryApproved { approved_at, .. } => approved_at.ts_ms,
            RecoveryFact::RecoveryDisputeFiled { filed_at, .. } => filed_at.ts_ms,
            RecoveryFact::RecoveryCompleted { completed_at, .. } => completed_at.ts_ms,
            RecoveryFact::RecoveryFailed { failed_at, .. } => failed_at.ts_ms,
        }
    }

    // ========================================================================
    // Backward Compatibility Constructors
    // ========================================================================

    /// Create a GuardianSetupInitiated fact with millisecond timestamp (backward compatibility)
    pub fn guardian_setup_initiated_ms(
        context_id: ContextId,
        initiator_id: AuthorityId,
        guardian_ids: Vec<AuthorityId>,
        threshold: u16,
        initiated_at_ms: u64,
    ) -> Self {
        Self::GuardianSetupInitiated {
            context_id,
            initiator_id,
            trace_id: None,
            guardian_ids,
            threshold,
            initiated_at: PhysicalTime {
                ts_ms: initiated_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a GuardianInvitationSent fact with millisecond timestamp (backward compatibility)
    pub fn guardian_invitation_sent_ms(
        context_id: ContextId,
        guardian_id: AuthorityId,
        invitation_hash: Hash32,
        sent_at_ms: u64,
    ) -> Self {
        Self::GuardianInvitationSent {
            context_id,
            guardian_id,
            trace_id: None,
            invitation_hash,
            sent_at: PhysicalTime {
                ts_ms: sent_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a GuardianAccepted fact with millisecond timestamp (backward compatibility)
    pub fn guardian_accepted_ms(
        context_id: ContextId,
        guardian_id: AuthorityId,
        accepted_at_ms: u64,
    ) -> Self {
        Self::GuardianAccepted {
            context_id,
            guardian_id,
            trace_id: None,
            accepted_at: PhysicalTime {
                ts_ms: accepted_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a GuardianDeclined fact with millisecond timestamp (backward compatibility)
    pub fn guardian_declined_ms(
        context_id: ContextId,
        guardian_id: AuthorityId,
        declined_at_ms: u64,
    ) -> Self {
        Self::GuardianDeclined {
            context_id,
            guardian_id,
            trace_id: None,
            declined_at: PhysicalTime {
                ts_ms: declined_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a GuardianSetupCompleted fact with millisecond timestamp (backward compatibility)
    pub fn guardian_setup_completed_ms(
        context_id: ContextId,
        guardian_ids: Vec<AuthorityId>,
        threshold: u16,
        completed_at_ms: u64,
    ) -> Self {
        Self::GuardianSetupCompleted {
            context_id,
            guardian_ids,
            trace_id: None,
            threshold,
            completed_at: PhysicalTime {
                ts_ms: completed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a GuardianSetupFailed fact with millisecond timestamp (backward compatibility)
    pub fn guardian_setup_failed_ms(
        context_id: ContextId,
        reason: String,
        failed_at_ms: u64,
    ) -> Self {
        Self::GuardianSetupFailed {
            context_id,
            reason,
            trace_id: None,
            failed_at: PhysicalTime {
                ts_ms: failed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a MembershipChangeProposed fact with millisecond timestamp (backward compatibility)
    pub fn membership_change_proposed_ms(
        context_id: ContextId,
        proposer_id: AuthorityId,
        change_type: MembershipChangeType,
        proposal_hash: Hash32,
        proposed_at_ms: u64,
    ) -> Self {
        Self::MembershipChangeProposed {
            context_id,
            proposer_id,
            trace_id: None,
            change_type,
            proposal_hash,
            proposed_at: PhysicalTime {
                ts_ms: proposed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a MembershipVoteCast fact with millisecond timestamp (backward compatibility)
    pub fn membership_vote_cast_ms(
        context_id: ContextId,
        voter_id: AuthorityId,
        proposal_hash: Hash32,
        approved: bool,
        voted_at_ms: u64,
    ) -> Self {
        Self::MembershipVoteCast {
            context_id,
            voter_id,
            trace_id: None,
            proposal_hash,
            approved,
            voted_at: PhysicalTime {
                ts_ms: voted_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a MembershipChangeCompleted fact with millisecond timestamp (backward compatibility)
    pub fn membership_change_completed_ms(
        context_id: ContextId,
        proposal_hash: Hash32,
        new_guardian_ids: Vec<AuthorityId>,
        new_threshold: u16,
        completed_at_ms: u64,
    ) -> Self {
        Self::MembershipChangeCompleted {
            context_id,
            proposal_hash,
            trace_id: None,
            new_guardian_ids,
            new_threshold,
            completed_at: PhysicalTime {
                ts_ms: completed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a MembershipChangeRejected fact with millisecond timestamp (backward compatibility)
    pub fn membership_change_rejected_ms(
        context_id: ContextId,
        proposal_hash: Hash32,
        reason: String,
        rejected_at_ms: u64,
    ) -> Self {
        Self::MembershipChangeRejected {
            context_id,
            proposal_hash,
            trace_id: None,
            reason,
            rejected_at: PhysicalTime {
                ts_ms: rejected_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a RecoveryInitiated fact with millisecond timestamp (backward compatibility)
    pub fn recovery_initiated_ms(
        context_id: ContextId,
        account_id: AuthorityId,
        request_hash: Hash32,
        initiated_at_ms: u64,
    ) -> Self {
        Self::RecoveryInitiated {
            context_id,
            account_id,
            trace_id: None,
            request_hash,
            initiated_at: PhysicalTime {
                ts_ms: initiated_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a RecoveryShareSubmitted fact with millisecond timestamp (backward compatibility)
    pub fn recovery_share_submitted_ms(
        context_id: ContextId,
        guardian_id: AuthorityId,
        share_hash: Hash32,
        submitted_at_ms: u64,
    ) -> Self {
        Self::RecoveryShareSubmitted {
            context_id,
            guardian_id,
            trace_id: None,
            share_hash,
            submitted_at: PhysicalTime {
                ts_ms: submitted_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a RecoveryApproved fact with millisecond timestamp (backward compatibility)
    pub fn recovery_approved_ms(
        context_id: ContextId,
        account_id: AuthorityId,
        approvals_hash: Hash32,
        approved_at_ms: u64,
    ) -> Self {
        Self::RecoveryApproved {
            context_id,
            account_id,
            trace_id: None,
            approvals_hash,
            approved_at: PhysicalTime {
                ts_ms: approved_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a RecoveryDisputeFiled fact with millisecond timestamp (backward compatibility)
    pub fn recovery_dispute_filed_ms(
        context_id: ContextId,
        disputer_id: AuthorityId,
        reason: String,
        filed_at_ms: u64,
    ) -> Self {
        Self::RecoveryDisputeFiled {
            context_id,
            disputer_id,
            trace_id: None,
            reason,
            filed_at: PhysicalTime {
                ts_ms: filed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a RecoveryCompleted fact with millisecond timestamp (backward compatibility)
    pub fn recovery_completed_ms(
        context_id: ContextId,
        account_id: AuthorityId,
        evidence_hash: Hash32,
        completed_at_ms: u64,
    ) -> Self {
        Self::RecoveryCompleted {
            context_id,
            account_id,
            trace_id: None,
            evidence_hash,
            completed_at: PhysicalTime {
                ts_ms: completed_at_ms,
                uncertainty: None,
            },
        }
    }

    /// Create a RecoveryFailed fact with millisecond timestamp (backward compatibility)
    pub fn recovery_failed_ms(
        context_id: ContextId,
        account_id: AuthorityId,
        reason: String,
        failed_at_ms: u64,
    ) -> Self {
        Self::RecoveryFailed {
            context_id,
            account_id,
            trace_id: None,
            reason,
            failed_at: PhysicalTime {
                ts_ms: failed_at_ms,
                uncertainty: None,
            },
        }
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

        let fact = RecoveryFact::from_bytes(binding_data)?;
        if !fact.validate_for_reduction(context_id) {
            return None;
        }
        let _sub_type = fact.sub_type().to_string();

        let key = fact.binding_key();

        Some(RelationalBinding {
            binding_type: RelationalBindingType::Generic(key.sub_type.to_string()),
            context_id,
            data: key.data,
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

    fn pt(ts_ms: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms,
            uncertainty: None,
        }
    }

    #[test]
    fn test_recovery_fact_serialization() {
        let fact = RecoveryFact::GuardianSetupInitiated {
            context_id: test_context_id(),
            initiator_id: test_authority_id(1),
            trace_id: None,
            guardian_ids: vec![
                test_authority_id(2),
                test_authority_id(3),
                test_authority_id(4),
            ],
            threshold: 2,
            initiated_at: pt(1234567890),
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
            trace_id: None,
            evidence_hash: test_hash(99),
            completed_at: pt(1234567899),
        };

        let generic = fact.to_generic();

        let aura_journal::RelationalFact::Generic {
            binding_type,
            binding_data,
            ..
        } = generic
        else {
            panic!("Expected Generic variant");
        };
        assert_eq!(binding_type, RECOVERY_FACT_TYPE_ID);
        let restored = RecoveryFact::from_bytes(&binding_data);
        assert!(restored.is_some());
    }

    #[test]
    fn test_recovery_fact_reducer() {
        let reducer = RecoveryFactReducer;
        assert_eq!(reducer.handles_type(), RECOVERY_FACT_TYPE_ID);

        let fact = RecoveryFact::GuardianAccepted {
            context_id: test_context_id(),
            guardian_id: test_authority_id(5),
            trace_id: None,
            accepted_at: pt(1234567890),
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
    fn test_reducer_rejects_context_mismatch() {
        let reducer = RecoveryFactReducer;

        let fact = RecoveryFact::GuardianAccepted {
            context_id: test_context_id(),
            guardian_id: test_authority_id(5),
            trace_id: None,
            accepted_at: pt(1234567890),
        };

        let other_context = ContextId::new_from_entropy([7u8; 32]);
        let binding = reducer.reduce(other_context, RECOVERY_FACT_TYPE_ID, &fact.to_bytes());
        assert!(binding.is_none());
    }

    #[test]
    fn test_binding_key_derivation() {
        let fact = RecoveryFact::MembershipChangeProposed {
            context_id: test_context_id(),
            proposer_id: test_authority_id(3),
            trace_id: None,
            change_type: MembershipChangeType::UpdateThreshold { new_threshold: 3 },
            proposal_hash: test_hash(2),
            proposed_at: pt(0),
        };

        let key = fact.binding_key();
        assert_eq!(key.sub_type, "membership-change-proposed");
        assert_eq!(key.data, test_hash(2).0.to_vec());
    }

    #[test]
    fn test_reducer_idempotence() {
        let reducer = RecoveryFactReducer;
        let context_id = test_context_id();
        let fact = RecoveryFact::GuardianAccepted {
            context_id,
            guardian_id: test_authority_id(5),
            trace_id: None,
            accepted_at: pt(1234567890),
        };

        let bytes = fact.to_bytes();
        let binding1 = reducer.reduce(context_id, RECOVERY_FACT_TYPE_ID, &bytes);
        let binding2 = reducer.reduce(context_id, RECOVERY_FACT_TYPE_ID, &bytes);
        assert!(binding1.is_some());
        assert!(binding2.is_some());
        let binding1 = binding1.unwrap();
        let binding2 = binding2.unwrap();
        assert_eq!(binding1.binding_type, binding2.binding_type);
        assert_eq!(binding1.context_id, binding2.context_id);
        assert_eq!(binding1.data, binding2.data);
    }

    #[test]
    fn test_context_id_extraction() {
        let ctx = test_context_id();
        let facts = vec![
            RecoveryFact::GuardianSetupInitiated {
                context_id: ctx,
                initiator_id: test_authority_id(1),
                trace_id: None,
                guardian_ids: vec![],
                threshold: 2,
                initiated_at: pt(0),
            },
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: test_authority_id(2),
                trace_id: None,
                request_hash: test_hash(1),
                initiated_at: pt(0),
            },
            RecoveryFact::MembershipChangeProposed {
                context_id: ctx,
                proposer_id: test_authority_id(3),
                trace_id: None,
                change_type: MembershipChangeType::UpdateThreshold { new_threshold: 3 },
                proposal_hash: test_hash(2),
                proposed_at: pt(0),
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
                trace_id: None,
                guardian_ids: vec![],
                threshold: 2,
                initiated_at: pt(0),
            },
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: test_authority_id(2),
                trace_id: None,
                accepted_at: pt(0),
            },
            RecoveryFact::RecoveryCompleted {
                context_id: ctx,
                account_id: test_authority_id(3),
                trace_id: None,
                evidence_hash: test_hash(1),
                completed_at: pt(0),
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
                trace_id: None,
                guardian_ids: vec![],
                threshold: 2,
                initiated_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::GuardianInvitationSent {
                context_id: ctx,
                guardian_id: test_authority_id(1),
                trace_id: None,
                invitation_hash: test_hash(1),
                sent_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::GuardianAccepted {
                context_id: ctx,
                guardian_id: test_authority_id(1),
                trace_id: None,
                accepted_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::GuardianDeclined {
                context_id: ctx,
                guardian_id: test_authority_id(1),
                trace_id: None,
                declined_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::GuardianSetupCompleted {
                context_id: ctx,
                guardian_ids: vec![],
                trace_id: None,
                threshold: 2,
                completed_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::GuardianSetupFailed {
                context_id: ctx,
                reason: "test".to_string(),
                trace_id: None,
                failed_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::MembershipChangeProposed {
                context_id: ctx,
                proposer_id: test_authority_id(1),
                trace_id: None,
                change_type: MembershipChangeType::UpdateThreshold { new_threshold: 3 },
                proposal_hash: test_hash(1),
                proposed_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::MembershipVoteCast {
                context_id: ctx,
                voter_id: test_authority_id(1),
                trace_id: None,
                proposal_hash: test_hash(1),
                approved: true,
                voted_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::MembershipChangeCompleted {
                context_id: ctx,
                proposal_hash: test_hash(1),
                trace_id: None,
                new_guardian_ids: vec![],
                new_threshold: 2,
                completed_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::MembershipChangeRejected {
                context_id: ctx,
                proposal_hash: test_hash(1),
                trace_id: None,
                reason: "test".to_string(),
                rejected_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::RecoveryInitiated {
                context_id: ctx,
                account_id: test_authority_id(1),
                trace_id: None,
                request_hash: test_hash(1),
                initiated_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::RecoveryShareSubmitted {
                context_id: ctx,
                guardian_id: test_authority_id(1),
                trace_id: None,
                share_hash: test_hash(1),
                submitted_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::RecoveryApproved {
                context_id: ctx,
                account_id: test_authority_id(1),
                trace_id: None,
                approvals_hash: test_hash(1),
                approved_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::RecoveryDisputeFiled {
                context_id: ctx,
                disputer_id: test_authority_id(1),
                trace_id: None,
                reason: "test".to_string(),
                filed_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::RecoveryCompleted {
                context_id: ctx,
                account_id: test_authority_id(1),
                trace_id: None,
                evidence_hash: test_hash(1),
                completed_at: pt(0),
            }
            .sub_type(),
            RecoveryFact::RecoveryFailed {
                context_id: ctx,
                account_id: test_authority_id(1),
                trace_id: None,
                reason: "test".to_string(),
                failed_at: pt(0),
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

    #[test]
    fn test_timestamp_ms_backward_compat() {
        let fact = RecoveryFact::guardian_setup_initiated_ms(
            test_context_id(),
            test_authority_id(1),
            vec![test_authority_id(2), test_authority_id(3)],
            2,
            1234567890,
        );
        assert_eq!(fact.timestamp_ms(), 1234567890);

        let fact2 = RecoveryFact::recovery_completed_ms(
            test_context_id(),
            test_authority_id(1),
            test_hash(1),
            9876543210,
        );
        assert_eq!(fact2.timestamp_ms(), 9876543210);
    }
}
