//! Proposal facts for deferred operations
//!
//! Pure fact types for proposal state changes in the effect policy system.
//! These facts are defined here (Layer 2) and committed by higher layers.
//!
//! Proposals are created when an operation requires approval before its effect
//! can be applied. This implements the "Deferred" execution mode from the effect
//! policy system.
//!
//! # Proposal Lifecycle
//!
//! ```text
//! Created → (Approved | Rejected | Expired | Withdrawn)
//!         ↓
//!     (threshold met?)
//!         ↓
//!     Completed or Failed
//! ```
//!
//! # Architecture
//!
//! Following Layer 2 constraints:
//! - Pure fact types with no external dependencies beyond aura-core
//! - Reducers for deriving state from facts
//! - Higher layers (aura-protocol, etc.) handle journal integration
//!
//! # Example
//!
//! ```rust
//! use aura_authorization::proposals::{ProposalFact, ProposalState, PROPOSAL_FACT_TYPE_ID};
//! use aura_authorization::effect_policy::{OperationType, ApprovalThreshold};
//! use aura_core::identifiers::{ContextId, AuthorityId};
//!
//! // Create a proposal for removing a channel member
//! let fact = ProposalFact::created(
//!     ContextId::new_from_entropy([1u8; 32]),
//!     "prop-123".to_string(),
//!     AuthorityId::new_from_entropy([2u8; 32]),
//!     OperationType::RemoveChannelMember,
//!     b"{}".to_vec(),
//!     ApprovalThreshold::Any,
//!     1000,
//!     Some(2000),
//!     Some("Remove inactive member".to_string()),
//! );
//!
//! // Build state from fact
//! let state = ProposalState::from_created(&fact).unwrap();
//! assert!(state.is_pending());
//! ```

use crate::effect_policy::{ApprovalThreshold, OperationType};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::PhysicalTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Type identifier for proposal facts
pub const PROPOSAL_FACT_TYPE_ID: &str = "proposal/v1";

/// Proposal domain fact types
///
/// These facts represent proposal-related state changes.
/// They are stored via the journal system and reduced by `ProposalFactReducer`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalFact {
    /// Proposal created for a deferred operation
    Created {
        /// Relational context for the proposal
        context_id: ContextId,
        /// Unique proposal identifier
        proposal_id: String,
        /// Authority creating the proposal
        proposer_id: AuthorityId,
        /// Type of operation being proposed
        operation_type: OperationType,
        /// Serialized operation data (JSON)
        operation_data: Vec<u8>,
        /// Required approval threshold
        approval_requirement: ApprovalThreshold,
        /// Timestamp when proposal was created
        created_at: PhysicalTime,
        /// Optional expiration timestamp
        expires_at: Option<PhysicalTime>,
        /// Human-readable description
        description: Option<String>,
    },

    /// Approval cast for a proposal
    Approved {
        /// Proposal being approved
        proposal_id: String,
        /// Authority approving the proposal
        approver_id: AuthorityId,
        /// Timestamp when approval was cast
        approved_at: PhysicalTime,
        /// Optional comment with approval
        comment: Option<String>,
    },

    /// Rejection cast for a proposal
    Rejected {
        /// Proposal being rejected
        proposal_id: String,
        /// Authority rejecting the proposal
        rejector_id: AuthorityId,
        /// Timestamp when rejection was cast
        rejected_at: PhysicalTime,
        /// Reason for rejection
        reason: Option<String>,
    },

    /// Proposal withdrawn by proposer
    Withdrawn {
        /// Proposal being withdrawn
        proposal_id: String,
        /// Authority withdrawing (must be proposer)
        withdrawer_id: AuthorityId,
        /// Timestamp when proposal was withdrawn
        withdrawn_at: PhysicalTime,
        /// Reason for withdrawal
        reason: Option<String>,
    },

    /// Proposal completed successfully (threshold met)
    Completed {
        /// Proposal that completed
        proposal_id: String,
        /// Timestamp when proposal completed
        completed_at: PhysicalTime,
        /// List of approvers who approved
        approvers: Vec<AuthorityId>,
        /// Result of executing the operation (if any)
        result_data: Option<Vec<u8>>,
    },

    /// Proposal failed (rejected or expired)
    Failed {
        /// Proposal that failed
        proposal_id: String,
        /// Timestamp when failure was recorded
        failed_at: PhysicalTime,
        /// Reason for failure
        failure_reason: ProposalFailureReason,
    },
}

/// Reasons a proposal can fail
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalFailureReason {
    /// Proposal expired before threshold was met
    Expired,
    /// Proposal received enough rejections to fail
    Rejected,
    /// Proposal was vetoed (if veto mechanism enabled)
    Vetoed { vetoer_id: AuthorityId },
    /// Context no longer exists
    ContextGone,
    /// Proposer lost required permissions
    PermissionLost,
    /// Operation is no longer valid (e.g., target already deleted)
    OperationInvalid { reason: String },
}

impl ProposalFact {
    /// Extract the proposal_id from any variant
    pub fn proposal_id(&self) -> &str {
        match self {
            ProposalFact::Created { proposal_id, .. } => proposal_id,
            ProposalFact::Approved { proposal_id, .. } => proposal_id,
            ProposalFact::Rejected { proposal_id, .. } => proposal_id,
            ProposalFact::Withdrawn { proposal_id, .. } => proposal_id,
            ProposalFact::Completed { proposal_id, .. } => proposal_id,
            ProposalFact::Failed { proposal_id, .. } => proposal_id,
        }
    }

    /// Get the timestamp in milliseconds
    pub fn timestamp_ms(&self) -> u64 {
        match self {
            ProposalFact::Created { created_at, .. } => created_at.ts_ms,
            ProposalFact::Approved { approved_at, .. } => approved_at.ts_ms,
            ProposalFact::Rejected { rejected_at, .. } => rejected_at.ts_ms,
            ProposalFact::Withdrawn { withdrawn_at, .. } => withdrawn_at.ts_ms,
            ProposalFact::Completed { completed_at, .. } => completed_at.ts_ms,
            ProposalFact::Failed { failed_at, .. } => failed_at.ts_ms,
        }
    }

    /// Get the fact type name for journal keying
    pub fn fact_type(&self) -> &'static str {
        match self {
            ProposalFact::Created { .. } => "proposal_created",
            ProposalFact::Approved { .. } => "proposal_approved",
            ProposalFact::Rejected { .. } => "proposal_rejected",
            ProposalFact::Withdrawn { .. } => "proposal_withdrawn",
            ProposalFact::Completed { .. } => "proposal_completed",
            ProposalFact::Failed { .. } => "proposal_failed",
        }
    }

    /// Get the context ID if this is a Created fact
    pub fn context_id(&self) -> Option<ContextId> {
        match self {
            ProposalFact::Created { context_id, .. } => Some(*context_id),
            _ => None,
        }
    }

    /// Get the primary authority ID associated with this fact
    pub fn authority_id(&self) -> Option<AuthorityId> {
        match self {
            ProposalFact::Created { proposer_id, .. } => Some(*proposer_id),
            ProposalFact::Approved { approver_id, .. } => Some(*approver_id),
            ProposalFact::Rejected { rejector_id, .. } => Some(*rejector_id),
            ProposalFact::Withdrawn { withdrawer_id, .. } => Some(*withdrawer_id),
            ProposalFact::Completed { .. } => None,
            ProposalFact::Failed { .. } => None,
        }
    }

    /// Create a Created fact with millisecond timestamps
    #[allow(clippy::too_many_arguments)]
    pub fn created(
        context_id: ContextId,
        proposal_id: String,
        proposer_id: AuthorityId,
        operation_type: OperationType,
        operation_data: Vec<u8>,
        approval_requirement: ApprovalThreshold,
        created_at_ms: u64,
        expires_at_ms: Option<u64>,
        description: Option<String>,
    ) -> Self {
        Self::Created {
            context_id,
            proposal_id,
            proposer_id,
            operation_type,
            operation_data,
            approval_requirement,
            created_at: PhysicalTime {
                ts_ms: created_at_ms,
                uncertainty: None,
            },
            expires_at: expires_at_ms.map(|ts_ms| PhysicalTime {
                ts_ms,
                uncertainty: None,
            }),
            description,
        }
    }

    /// Create an Approved fact
    pub fn approved(
        proposal_id: String,
        approver_id: AuthorityId,
        approved_at_ms: u64,
        comment: Option<String>,
    ) -> Self {
        Self::Approved {
            proposal_id,
            approver_id,
            approved_at: PhysicalTime {
                ts_ms: approved_at_ms,
                uncertainty: None,
            },
            comment,
        }
    }

    /// Create a Rejected fact
    pub fn rejected(
        proposal_id: String,
        rejector_id: AuthorityId,
        rejected_at_ms: u64,
        reason: Option<String>,
    ) -> Self {
        Self::Rejected {
            proposal_id,
            rejector_id,
            rejected_at: PhysicalTime {
                ts_ms: rejected_at_ms,
                uncertainty: None,
            },
            reason,
        }
    }

    /// Create a Withdrawn fact
    pub fn withdrawn(
        proposal_id: String,
        withdrawer_id: AuthorityId,
        withdrawn_at_ms: u64,
        reason: Option<String>,
    ) -> Self {
        Self::Withdrawn {
            proposal_id,
            withdrawer_id,
            withdrawn_at: PhysicalTime {
                ts_ms: withdrawn_at_ms,
                uncertainty: None,
            },
            reason,
        }
    }

    /// Create a Completed fact
    pub fn completed(
        proposal_id: String,
        completed_at_ms: u64,
        approvers: Vec<AuthorityId>,
        result_data: Option<Vec<u8>>,
    ) -> Self {
        Self::Completed {
            proposal_id,
            completed_at: PhysicalTime {
                ts_ms: completed_at_ms,
                uncertainty: None,
            },
            approvers,
            result_data,
        }
    }

    /// Create a Failed fact
    pub fn failed(
        proposal_id: String,
        failed_at_ms: u64,
        failure_reason: ProposalFailureReason,
    ) -> Self {
        Self::Failed {
            proposal_id,
            failed_at: PhysicalTime {
                ts_ms: failed_at_ms,
                uncertainty: None,
            },
            failure_reason,
        }
    }

    /// Check if this fact indicates the proposal is terminal (completed, failed, or withdrawn)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            ProposalFact::Completed { .. }
                | ProposalFact::Failed { .. }
                | ProposalFact::Withdrawn { .. }
        )
    }
}

/// Current state of a proposal (derived from facts)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposalStatus {
    /// Proposal is pending approval
    Pending {
        /// Current approval count
        approvals: u16,
        /// Current rejection count
        rejections: u16,
        /// Required threshold
        required: ApprovalThreshold,
    },
    /// Proposal was approved and completed
    Approved,
    /// Proposal was rejected
    Rejected,
    /// Proposal expired before reaching threshold
    Expired,
    /// Proposal was withdrawn by proposer
    Withdrawn,
}

/// Aggregated proposal state derived from facts
#[derive(Debug, Clone)]
pub struct ProposalState {
    /// Proposal identifier
    pub proposal_id: String,
    /// Context this proposal belongs to
    pub context_id: ContextId,
    /// Authority that created the proposal
    pub proposer_id: AuthorityId,
    /// Operation being proposed
    pub operation_type: OperationType,
    /// Serialized operation data
    pub operation_data: Vec<u8>,
    /// Required approval threshold
    pub approval_requirement: ApprovalThreshold,
    /// When the proposal was created
    pub created_at: PhysicalTime,
    /// When the proposal expires (if any)
    pub expires_at: Option<PhysicalTime>,
    /// Description of the proposal
    pub description: Option<String>,
    /// Map of approvals: approver_id -> (timestamp, comment)
    pub approvals: HashMap<AuthorityId, (PhysicalTime, Option<String>)>,
    /// Map of rejections: rejector_id -> (timestamp, reason)
    pub rejections: HashMap<AuthorityId, (PhysicalTime, Option<String>)>,
    /// Current status
    pub status: ProposalStatus,
}

impl ProposalState {
    /// Create a new proposal state from a Created fact
    pub fn from_created(fact: &ProposalFact) -> Option<Self> {
        match fact {
            ProposalFact::Created {
                context_id,
                proposal_id,
                proposer_id,
                operation_type,
                operation_data,
                approval_requirement,
                created_at,
                expires_at,
                description,
            } => Some(Self {
                proposal_id: proposal_id.clone(),
                context_id: *context_id,
                proposer_id: *proposer_id,
                operation_type: operation_type.clone(),
                operation_data: operation_data.clone(),
                approval_requirement: approval_requirement.clone(),
                created_at: created_at.clone(),
                expires_at: expires_at.clone(),
                description: description.clone(),
                approvals: HashMap::new(),
                rejections: HashMap::new(),
                status: ProposalStatus::Pending {
                    approvals: 0u16,
                    rejections: 0u16,
                    required: approval_requirement.clone(),
                },
            }),
            _ => None,
        }
    }

    /// Apply a proposal fact to update state
    pub fn apply(&mut self, fact: &ProposalFact) {
        match fact {
            ProposalFact::Approved {
                approver_id,
                approved_at,
                comment,
                ..
            } => {
                self.approvals
                    .insert(*approver_id, (approved_at.clone(), comment.clone()));
                self.update_status();
            }
            ProposalFact::Rejected {
                rejector_id,
                rejected_at,
                reason,
                ..
            } => {
                self.rejections
                    .insert(*rejector_id, (rejected_at.clone(), reason.clone()));
                self.update_status();
            }
            ProposalFact::Withdrawn { .. } => {
                self.status = ProposalStatus::Withdrawn;
            }
            ProposalFact::Completed { .. } => {
                self.status = ProposalStatus::Approved;
            }
            ProposalFact::Failed { failure_reason, .. } => {
                self.status = match failure_reason {
                    ProposalFailureReason::Expired => ProposalStatus::Expired,
                    _ => ProposalStatus::Rejected,
                };
            }
            ProposalFact::Created { .. } => {
                // Created is the initial fact, shouldn't be applied to existing state
            }
        }
    }

    /// Update status based on current approvals/rejections
    fn update_status(&mut self) {
        if matches!(
            self.status,
            ProposalStatus::Approved
                | ProposalStatus::Rejected
                | ProposalStatus::Withdrawn
                | ProposalStatus::Expired
        ) {
            // Terminal states don't change
            return;
        }

        // Check if threshold is met
        let threshold_met = self.check_threshold_met();
        if threshold_met {
            self.status = ProposalStatus::Approved;
        } else {
            self.status = ProposalStatus::Pending {
                approvals: self.approvals.len() as u16,
                rejections: self.rejections.len() as u16,
                required: self.approval_requirement.clone(),
            };
        }
    }

    /// Check if the approval threshold has been met
    ///
    /// Note: For `Unanimous` and `Percentage` thresholds, the total eligible
    /// count must be tracked separately. This method uses the approval count
    /// as a simple check against the threshold type. For full threshold
    /// evaluation, use `ApprovalThreshold::is_met()` with the total eligible count.
    pub fn check_threshold_met(&self) -> bool {
        let approval_count = self.approvals.len();

        match &self.approval_requirement {
            ApprovalThreshold::Any => approval_count >= 1,
            ApprovalThreshold::Unanimous => {
                // Without knowing total eligible, we can't verify unanimous
                // This will be checked at completion time with full context
                false
            }
            ApprovalThreshold::Threshold { required } => (approval_count as u32) >= *required,
            ApprovalThreshold::Percentage { percent } => {
                // Without knowing total eligible, we use approvals as proxy
                // For strict evaluation, use is_met() at completion time
                let required = (approval_count as f64 * *percent as f64 / 100.0).ceil() as u32;
                (approval_count as u32) >= required
            }
        }
    }

    /// Check if the approval threshold has been met given total eligible voters
    pub fn check_threshold_met_with_total(&self, total_eligible: usize) -> bool {
        self.approval_requirement
            .is_met(self.approvals.len() as u32, total_eligible as u32)
    }

    /// Check if the proposal has expired
    pub fn is_expired(&self, current_time_ms: u64) -> bool {
        if let Some(ref expires_at) = self.expires_at {
            current_time_ms >= expires_at.ts_ms
        } else {
            false
        }
    }

    /// Check if the proposal is still pending
    pub fn is_pending(&self) -> bool {
        matches!(self.status, ProposalStatus::Pending { .. })
    }
}

/// Delta type for proposal fact application
#[derive(Debug, Clone, Default)]
pub struct ProposalFactDelta {
    /// Proposals created in this delta
    pub proposals_created: u64,
    /// Approvals cast in this delta
    pub approvals_cast: u64,
    /// Rejections cast in this delta
    pub rejections_cast: u64,
    /// Proposals withdrawn in this delta
    pub proposals_withdrawn: u64,
    /// Proposals completed in this delta
    pub proposals_completed: u64,
    /// Proposals failed in this delta
    pub proposals_failed: u64,
}

/// Reducer for proposal facts
#[derive(Debug, Clone, Default)]
pub struct ProposalFactReducer;

impl ProposalFactReducer {
    /// Create a new proposal fact reducer
    pub fn new() -> Self {
        Self
    }

    /// Apply a fact to produce a delta
    pub fn apply(&self, fact: &ProposalFact) -> ProposalFactDelta {
        let mut delta = ProposalFactDelta::default();

        match fact {
            ProposalFact::Created { .. } => {
                delta.proposals_created = 1;
            }
            ProposalFact::Approved { .. } => {
                delta.approvals_cast = 1;
            }
            ProposalFact::Rejected { .. } => {
                delta.rejections_cast = 1;
            }
            ProposalFact::Withdrawn { .. } => {
                delta.proposals_withdrawn = 1;
            }
            ProposalFact::Completed { .. } => {
                delta.proposals_completed = 1;
            }
            ProposalFact::Failed { .. } => {
                delta.proposals_failed = 1;
            }
        }

        delta
    }

    /// Get the type ID this reducer handles
    pub fn handles_type(&self) -> &'static str {
        PROPOSAL_FACT_TYPE_ID
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
    fn test_proposal_fact_created() {
        let fact = ProposalFact::created(
            test_context_id(),
            "prop-123".to_string(),
            test_authority_id(1),
            OperationType::RemoveChannelMember,
            b"{}".to_vec(),
            ApprovalThreshold::Any,
            1234567890,
            Some(1234567890 + 86400000),
            Some("Remove inactive member".to_string()),
        );

        assert_eq!(fact.proposal_id(), "prop-123");
        assert_eq!(fact.timestamp_ms(), 1234567890);
        assert_eq!(fact.fact_type(), "proposal_created");
        assert_eq!(fact.context_id(), Some(test_context_id()));
        assert_eq!(fact.authority_id(), Some(test_authority_id(1)));
        assert!(!fact.is_terminal());
    }

    #[test]
    fn test_proposal_fact_approved() {
        let fact = ProposalFact::approved(
            "prop-456".to_string(),
            test_authority_id(3),
            1234567899,
            Some("LGTM".to_string()),
        );

        assert_eq!(fact.proposal_id(), "prop-456");
        assert_eq!(fact.timestamp_ms(), 1234567899);
        assert_eq!(fact.fact_type(), "proposal_approved");
        assert_eq!(fact.authority_id(), Some(test_authority_id(3)));
        assert!(!fact.is_terminal());
    }

    #[test]
    fn test_proposal_id_extraction() {
        let facts = [
            ProposalFact::created(
                test_context_id(),
                "prop-1".to_string(),
                test_authority_id(1),
                OperationType::SendMessage,
                vec![],
                ApprovalThreshold::Any,
                0,
                None,
                None,
            ),
            ProposalFact::approved("prop-2".to_string(), test_authority_id(2), 0, None),
            ProposalFact::rejected("prop-3".to_string(), test_authority_id(3), 0, None),
            ProposalFact::withdrawn("prop-4".to_string(), test_authority_id(4), 0, None),
            ProposalFact::completed("prop-5".to_string(), 0, vec![], None),
            ProposalFact::failed("prop-6".to_string(), 0, ProposalFailureReason::Expired),
        ];

        for (i, fact) in facts.iter().enumerate() {
            assert_eq!(fact.proposal_id(), format!("prop-{}", i + 1));
        }
    }

    #[test]
    fn test_timestamp_ms() {
        let timestamps = [100, 200, 300, 400, 500, 600];
        let facts = [
            ProposalFact::created(
                test_context_id(),
                "x".to_string(),
                test_authority_id(1),
                OperationType::SendMessage,
                vec![],
                ApprovalThreshold::Any,
                timestamps[0],
                None,
                None,
            ),
            ProposalFact::approved("x".to_string(), test_authority_id(2), timestamps[1], None),
            ProposalFact::rejected("x".to_string(), test_authority_id(3), timestamps[2], None),
            ProposalFact::withdrawn("x".to_string(), test_authority_id(4), timestamps[3], None),
            ProposalFact::completed("x".to_string(), timestamps[4], vec![], None),
            ProposalFact::failed(
                "x".to_string(),
                timestamps[5],
                ProposalFailureReason::Expired,
            ),
        ];

        for (i, fact) in facts.iter().enumerate() {
            assert_eq!(fact.timestamp_ms(), timestamps[i]);
        }
    }

    #[test]
    fn test_is_terminal() {
        let pending = ProposalFact::created(
            test_context_id(),
            "x".to_string(),
            test_authority_id(1),
            OperationType::SendMessage,
            vec![],
            ApprovalThreshold::Any,
            0,
            None,
            None,
        );
        assert!(!pending.is_terminal());

        let approved = ProposalFact::approved("x".to_string(), test_authority_id(2), 0, None);
        assert!(!approved.is_terminal());

        let completed = ProposalFact::completed("x".to_string(), 0, vec![], None);
        assert!(completed.is_terminal());

        let failed = ProposalFact::failed("x".to_string(), 0, ProposalFailureReason::Rejected);
        assert!(failed.is_terminal());

        let withdrawn = ProposalFact::withdrawn("x".to_string(), test_authority_id(1), 0, None);
        assert!(withdrawn.is_terminal());
    }

    #[test]
    fn test_proposal_state_any_threshold() {
        let created = ProposalFact::created(
            test_context_id(),
            "prop-1".to_string(),
            test_authority_id(1),
            OperationType::RemoveChannelMember,
            vec![],
            ApprovalThreshold::Any,
            1000,
            None,
            None,
        );

        let mut state = ProposalState::from_created(&created).unwrap();
        assert!(state.is_pending());
        assert!(!state.check_threshold_met());

        // First approval should meet threshold
        let approval =
            ProposalFact::approved("prop-1".to_string(), test_authority_id(2), 2000, None);
        state.apply(&approval);

        assert!(state.check_threshold_met());
    }

    #[test]
    fn test_proposal_state_unanimous_threshold() {
        let created = ProposalFact::created(
            test_context_id(),
            "prop-2".to_string(),
            test_authority_id(1),
            OperationType::DeleteChannel,
            vec![],
            ApprovalThreshold::Unanimous,
            1000,
            None,
            None,
        );

        let mut state = ProposalState::from_created(&created).unwrap();
        // Unanimous without total can't be checked with simple method
        assert!(!state.check_threshold_met());

        // Add approvals
        state.apply(&ProposalFact::approved(
            "prop-2".to_string(),
            test_authority_id(2),
            2000,
            None,
        ));
        state.apply(&ProposalFact::approved(
            "prop-2".to_string(),
            test_authority_id(3),
            3000,
            None,
        ));
        state.apply(&ProposalFact::approved(
            "prop-2".to_string(),
            test_authority_id(4),
            4000,
            None,
        ));

        // With total eligible = 3, unanimous is met
        assert!(state.check_threshold_met_with_total(3));
        // With total eligible = 4, unanimous is not met
        assert!(!state.check_threshold_met_with_total(4));
    }

    #[test]
    fn test_proposal_state_threshold() {
        let created = ProposalFact::created(
            test_context_id(),
            "prop-3".to_string(),
            test_authority_id(1),
            OperationType::TransferChannelOwnership,
            vec![],
            ApprovalThreshold::Threshold { required: 2 },
            1000,
            None,
            None,
        );

        let mut state = ProposalState::from_created(&created).unwrap();
        assert!(!state.check_threshold_met());

        state.apply(&ProposalFact::approved(
            "prop-3".to_string(),
            test_authority_id(2),
            2000,
            None,
        ));
        assert!(!state.check_threshold_met());

        state.apply(&ProposalFact::approved(
            "prop-3".to_string(),
            test_authority_id(3),
            3000,
            None,
        ));
        assert!(state.check_threshold_met());
    }

    #[test]
    fn test_proposal_state_percentage_threshold() {
        let created = ProposalFact::created(
            test_context_id(),
            "prop-4".to_string(),
            test_authority_id(1),
            OperationType::UpdateChannelTopic,
            vec![],
            ApprovalThreshold::Percentage { percent: 51 },
            1000,
            None,
            None,
        );

        let mut state = ProposalState::from_created(&created).unwrap();

        // Add approvals
        state.apply(&ProposalFact::approved(
            "prop-4".to_string(),
            test_authority_id(2),
            2000,
            None,
        ));
        state.apply(&ProposalFact::approved(
            "prop-4".to_string(),
            test_authority_id(3),
            3000,
            None,
        ));

        // With total eligible = 5, need 51% = 3 approvals (ceil(5 * 0.51) = 3)
        assert!(!state.check_threshold_met_with_total(5)); // Only 2 approvals

        state.apply(&ProposalFact::approved(
            "prop-4".to_string(),
            test_authority_id(4),
            4000,
            None,
        ));
        assert!(state.check_threshold_met_with_total(5)); // 3 approvals meets 51% of 5
    }

    #[test]
    fn test_proposal_expiration() {
        let created = ProposalFact::created(
            test_context_id(),
            "prop-5".to_string(),
            test_authority_id(1),
            OperationType::ArchiveChannel,
            vec![],
            ApprovalThreshold::Any,
            1000,
            Some(5000),
            None,
        );

        let state = ProposalState::from_created(&created).unwrap();

        assert!(!state.is_expired(1000)); // At creation
        assert!(!state.is_expired(4999)); // Just before expiry
        assert!(state.is_expired(5000)); // At expiry
        assert!(state.is_expired(6000)); // After expiry
    }

    #[test]
    fn test_proposal_withdrawn_terminal() {
        let created = ProposalFact::created(
            test_context_id(),
            "prop-6".to_string(),
            test_authority_id(1),
            OperationType::RemoveChannelMember,
            vec![],
            ApprovalThreshold::Any,
            1000,
            None,
            None,
        );

        let mut state = ProposalState::from_created(&created).unwrap();
        assert!(state.is_pending());

        state.apply(&ProposalFact::withdrawn(
            "prop-6".to_string(),
            test_authority_id(1),
            2000,
            Some("Changed my mind".to_string()),
        ));

        assert!(!state.is_pending());
        assert_eq!(state.status, ProposalStatus::Withdrawn);
    }

    #[test]
    fn test_failure_reasons() {
        let reasons = [
            ProposalFailureReason::Expired,
            ProposalFailureReason::Rejected,
            ProposalFailureReason::Vetoed {
                vetoer_id: test_authority_id(99),
            },
            ProposalFailureReason::ContextGone,
            ProposalFailureReason::PermissionLost,
            ProposalFailureReason::OperationInvalid {
                reason: "Target deleted".to_string(),
            },
        ];

        for reason in reasons {
            let fact = ProposalFact::failed("prop".to_string(), 0, reason);
            assert!(fact.is_terminal());
        }
    }

    #[test]
    fn test_proposal_fact_reducer() {
        let reducer = ProposalFactReducer::new();
        assert_eq!(reducer.handles_type(), PROPOSAL_FACT_TYPE_ID);

        let created = ProposalFact::created(
            test_context_id(),
            "prop".to_string(),
            test_authority_id(1),
            OperationType::SendMessage,
            vec![],
            ApprovalThreshold::Any,
            0,
            None,
            None,
        );
        let delta = reducer.apply(&created);
        assert_eq!(delta.proposals_created, 1);

        let approved = ProposalFact::approved("prop".to_string(), test_authority_id(2), 0, None);
        let delta = reducer.apply(&approved);
        assert_eq!(delta.approvals_cast, 1);

        let completed = ProposalFact::completed("prop".to_string(), 0, vec![], None);
        let delta = reducer.apply(&completed);
        assert_eq!(delta.proposals_completed, 1);
    }

    #[test]
    fn test_duplicate_approvals_idempotent() {
        let created = ProposalFact::created(
            test_context_id(),
            "prop".to_string(),
            test_authority_id(1),
            OperationType::RemoveChannelMember,
            vec![],
            ApprovalThreshold::Threshold { required: 2 },
            1000,
            None,
            None,
        );

        let mut state = ProposalState::from_created(&created).unwrap();

        // Same authority approves twice - should only count once
        state.apply(&ProposalFact::approved(
            "prop".to_string(),
            test_authority_id(2),
            2000,
            None,
        ));
        state.apply(&ProposalFact::approved(
            "prop".to_string(),
            test_authority_id(2),
            3000,
            Some("Approving again".to_string()),
        ));

        assert_eq!(state.approvals.len(), 1);
        assert!(!state.check_threshold_met()); // Still need 2 unique approvers
    }
}
