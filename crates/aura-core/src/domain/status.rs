//! Category-Specific Status Types
//!
//! This module provides purpose-built status types for each operation category
//! from `docs/107_operation_categories.md`:
//!
//! - **Category A (Optimistic)**: `OptimisticStatus` - immediate effect, background tracking
//! - **Category B (Deferred)**: `DeferredStatus` - requires approval before effect
//! - **Category C (Ceremony)**: `CeremonyStatus` - blocks until ceremony completes
//!
//! These types provide rich metadata for UI display and application logic,
//! built on top of the shared primitives (Agreement, Propagation, Acknowledgment).

use super::acknowledgment::Acknowledgment;
use super::agreement::Agreement;
use super::consistency::ProposalId;
use super::propagation::Propagation;
use crate::query::ConsensusId;
use crate::time::PhysicalTime;
use crate::types::{AuthorityId, Epoch};
use crate::CeremonyId;
use crate::Hash32;
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Category A: Optimistic Status
// ─────────────────────────────────────────────────────────────────────────────

/// Status for Category A (Optimistic) operations.
///
/// Effect applied immediately; background tracking of agreement/propagation/acks.
///
/// # Use Cases
///
/// - Send message
/// - Create channel
/// - Update profile
/// - React to message
///
/// # UI Patterns
///
/// ```text
/// ◐  Sending      propagation == Local
/// ✓  Sent         propagation == Complete
/// ✓✓ Delivered    acknowledgment.count() >= expected.len()
/// ◆  Finalized    agreement == Finalized
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptimisticStatus {
    /// Has this reached A3 finalization?
    pub agreement: Agreement,

    /// Anti-entropy propagation status
    pub propagation: Propagation,

    /// Per-peer acknowledgment (only if ack-tracked)
    pub acknowledgment: Option<Acknowledgment>,
}

impl Default for OptimisticStatus {
    fn default() -> Self {
        Self {
            agreement: Agreement::Provisional,
            propagation: Propagation::Local,
            acknowledgment: None,
        }
    }
}

impl OptimisticStatus {
    /// Create a new optimistic status with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with ack tracking enabled
    #[must_use]
    pub fn with_ack_tracking() -> Self {
        Self {
            acknowledgment: Some(Acknowledgment::new()),
            ..Self::default()
        }
    }

    /// Set the agreement level
    #[must_use]
    pub fn with_agreement(mut self, agreement: Agreement) -> Self {
        self.agreement = agreement;
        self
    }

    /// Set the propagation status
    #[must_use]
    pub fn with_propagation(mut self, propagation: Propagation) -> Self {
        self.propagation = propagation;
        self
    }

    /// Set the acknowledgment
    #[must_use]
    pub fn with_acknowledgment(mut self, acknowledgment: Acknowledgment) -> Self {
        self.acknowledgment = Some(acknowledgment);
        self
    }

    /// Quick check for UI: is this finalized (A3)?
    pub fn is_finalized(&self) -> bool {
        self.agreement.is_finalized()
    }

    /// Quick check for UI: is this at least safe (A2+)?
    pub fn is_safe(&self) -> bool {
        self.agreement.is_safe()
    }

    /// Quick check for UI: has propagation completed?
    pub fn is_propagated(&self) -> bool {
        self.propagation.is_complete()
    }

    /// Quick check for UI: is this delivered to all expected peers?
    pub fn is_delivered(&self, expected: &[AuthorityId]) -> bool {
        self.acknowledgment
            .as_ref()
            .map(|ack| expected.iter().all(|p| ack.contains(p)))
            .unwrap_or(false)
    }

    /// Get the ack count
    pub fn ack_count(&self) -> usize {
        self.acknowledgment.as_ref().map(|a| a.count()).unwrap_or(0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Category B: Deferred Status
// ─────────────────────────────────────────────────────────────────────────────

/// Status for Category B (Deferred) operations.
///
/// Proposal awaiting approval; effect applies when threshold reached.
///
/// # Use Cases
///
/// - Change permissions
/// - Remove member
/// - Transfer ownership
/// - Archive channel
///
/// # UI Patterns
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │ Pending: Remove Carol from #project                                      │
/// │   Approvals: 1 of 2 required                                             │
/// │     ✓ Alice (admin) - approved                                           │
/// │     ◐ Bob (admin) - pending                                              │
/// │   Expires in: 23h 45m                                    [Cancel]        │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeferredStatus {
    /// Unique identifier for this proposal
    pub proposal_id: ProposalId,

    /// Current state of the proposal
    pub state: ProposalState,

    /// Approval progress
    pub approvals: ApprovalProgress,

    /// If applied, what's its agreement level?
    pub applied_agreement: Option<Agreement>,

    /// When does this proposal expire?
    pub expires_at: PhysicalTime,
}

impl DeferredStatus {
    /// Create a new deferred status
    pub fn new(
        proposal_id: impl Into<ProposalId>,
        threshold: ApprovalThreshold,
        expires_at: PhysicalTime,
    ) -> Self {
        Self {
            proposal_id: proposal_id.into(),
            state: ProposalState::Pending,
            approvals: ApprovalProgress::new(threshold),
            applied_agreement: None,
            expires_at,
        }
    }

    /// Check if the proposal is pending
    pub fn is_pending(&self) -> bool {
        matches!(self.state, ProposalState::Pending)
    }

    /// Check if the proposal was approved
    pub fn is_approved(&self) -> bool {
        matches!(self.state, ProposalState::Approved)
    }

    /// Check if the proposal was rejected
    pub fn is_rejected(&self) -> bool {
        matches!(self.state, ProposalState::Rejected { .. })
    }

    /// Check if the proposal has expired
    pub fn is_expired(&self) -> bool {
        matches!(self.state, ProposalState::Expired)
    }

    /// Check if the proposal was superseded
    pub fn is_superseded(&self) -> bool {
        matches!(self.state, ProposalState::Superseded { .. })
    }

    /// Check if the applied effect is finalized
    pub fn is_finalized(&self) -> bool {
        self.applied_agreement
            .as_ref()
            .map(|a| a.is_finalized())
            .unwrap_or(false)
    }

    /// Get the approval count
    pub fn approval_count(&self) -> usize {
        self.approvals.approval_count()
    }

    /// Get the rejection count
    pub fn rejection_count(&self) -> usize {
        self.approvals.rejection_count()
    }
}

/// Current state of a Category B proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProposalState {
    /// Awaiting approvals
    Pending,

    /// Threshold reached, effect applied
    Approved,

    /// Explicitly rejected
    Rejected {
        /// Reason for rejection
        reason: String,
        /// Who rejected
        by: AuthorityId,
    },

    /// Timed out without reaching threshold
    Expired,

    /// Replaced by newer proposal
    Superseded {
        /// The proposal that superseded this one
        by: ProposalId,
    },
}

impl ProposalState {
    /// Check if this is a terminal state
    pub fn is_terminal(&self) -> bool {
        !matches!(self, Self::Pending)
    }
}

impl std::fmt::Display for ProposalState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pending => write!(f, "Pending"),
            Self::Approved => write!(f, "Approved"),
            Self::Rejected { reason, by } => write!(f, "Rejected by {by}: {reason}"),
            Self::Expired => write!(f, "Expired"),
            Self::Superseded { by } => write!(f, "Superseded by {by}"),
        }
    }
}

/// Approval progress tracking for Category B proposals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalProgress {
    /// What approval is required?
    pub required: ApprovalThreshold,
    /// Approvals received so far
    pub received: Vec<ApprovalRecord>,
}

impl ApprovalProgress {
    /// Create new approval progress with a threshold
    pub fn new(required: ApprovalThreshold) -> Self {
        Self {
            required,
            received: Vec::new(),
        }
    }

    /// Record an approval decision
    pub fn record(&mut self, approver: AuthorityId, decision: ApprovalDecision, at: PhysicalTime) {
        // Update if already exists, otherwise add new
        if let Some(existing) = self.received.iter_mut().find(|r| r.approver == approver) {
            existing.decision = decision;
            existing.decided_at = at;
        } else {
            self.received.push(ApprovalRecord {
                approver,
                decision,
                decided_at: at,
            });
        }
    }

    /// Get approval count
    pub fn approval_count(&self) -> usize {
        self.received
            .iter()
            .filter(|r| r.decision == ApprovalDecision::Approve)
            .count()
    }

    /// Get rejection count
    pub fn rejection_count(&self) -> usize {
        self.received
            .iter()
            .filter(|r| r.decision == ApprovalDecision::Reject)
            .count()
    }

    /// Check if threshold is met for approval
    pub fn is_threshold_met(&self, total_approvers: u32) -> bool {
        let approvals = self.approval_count() as u32;
        self.required.is_met(approvals, total_approvers)
    }

    /// Check if threshold can no longer be met (too many rejections)
    pub fn is_rejection_certain(&self, total_approvers: u32) -> bool {
        let rejections = self.rejection_count() as u32;
        let remaining = total_approvers.saturating_sub(self.received.len() as u32);
        let max_possible_approvals =
            (total_approvers - rejections).min(remaining + self.approval_count() as u32);

        !self
            .required
            .is_met(max_possible_approvals, total_approvers)
    }

    /// Get all approvers who approved
    pub fn approvers(&self) -> impl Iterator<Item = &AuthorityId> {
        self.received
            .iter()
            .filter(|r| r.decision == ApprovalDecision::Approve)
            .map(|r| &r.approver)
    }

    /// Get all approvers who rejected
    pub fn rejecters(&self) -> impl Iterator<Item = &AuthorityId> {
        self.received
            .iter()
            .filter(|r| r.decision == ApprovalDecision::Reject)
            .map(|r| &r.approver)
    }
}

/// A single approval record from an approver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    /// Who made the decision
    pub approver: AuthorityId,
    /// The decision made
    pub decision: ApprovalDecision,
    /// When the decision was made
    pub decided_at: PhysicalTime,
}

/// Approval decision for Category B proposals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalDecision {
    /// Approve the proposal
    Approve,
    /// Reject the proposal
    Reject,
    /// Abstain from voting
    Abstain,
}

impl std::fmt::Display for ApprovalDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Approve => write!(f, "Approve"),
            Self::Reject => write!(f, "Reject"),
            Self::Abstain => write!(f, "Abstain"),
        }
    }
}

/// Approval threshold requirements for Category B proposals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalThreshold {
    /// Any single holder of the required capability
    Any,
    /// All holders must approve
    Unanimous,
    /// k-of-n approval
    Threshold {
        /// Required number of approvals
        required: u32,
        /// Total number of approvers
        total: u32,
    },
    /// Percentage of holders (0-100)
    Percentage {
        /// Required percentage (0-100)
        percent: u8,
    },
}

impl ApprovalThreshold {
    /// Check if the threshold is met
    pub fn is_met(&self, approvals: u32, total_approvers: u32) -> bool {
        match self {
            Self::Any => approvals >= 1,
            Self::Unanimous => approvals >= total_approvers,
            Self::Threshold { required, .. } => approvals >= *required,
            Self::Percentage { percent } => {
                if total_approvers == 0 {
                    return false;
                }
                let required = (total_approvers as u64 * *percent as u64).div_ceil(100);
                approvals as u64 >= required
            }
        }
    }

    /// Get a human-readable description
    pub fn description(&self) -> String {
        match self {
            Self::Any => "any".to_string(),
            Self::Unanimous => "unanimous".to_string(),
            Self::Threshold { required, total } => format!("{required} of {total}"),
            Self::Percentage { percent } => format!("{percent}%"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Category C: Ceremony Status
// ─────────────────────────────────────────────────────────────────────────────

/// Status for Category C (Blocking) operations.
///
/// Ceremony in progress; blocks until commit or abort.
///
/// # Use Cases
///
/// - Add contact
/// - Create group
/// - Guardian rotation
/// - Device enrollment
/// - Recovery
///
/// # UI Patterns
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────────────────┐
/// │                    Adding Bob to group...                                │
/// │    ✓ Invitation sent                                                     │
/// │    ✓ Bob accepted                                                        │
/// │    ◐ Deriving group keys (2/3 responses)                                 │
/// │    ○ Committing                                                          │
/// │    ○ Ready                                                               │
/// │                      [Cancel]                                            │
/// └─────────────────────────────────────────────────────────────────────────┘
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CeremonyStatus {
    /// Unique identifier for this ceremony
    pub ceremony_id: CeremonyId,

    /// Ceremony lifecycle state
    pub state: CeremonyState,

    /// Participant responses
    pub responses: Vec<ParticipantResponse>,

    /// Prestate this ceremony is bound to
    pub prestate_hash: Hash32,

    /// If committed, the resulting agreement
    pub committed_agreement: Option<Agreement>,
}

impl CeremonyStatus {
    /// Create a new ceremony status
    pub fn new(ceremony_id: impl Into<CeremonyId>, prestate_hash: Hash32) -> Self {
        Self {
            ceremony_id: ceremony_id.into(),
            state: CeremonyState::Preparing,
            responses: Vec::new(),
            prestate_hash,
            committed_agreement: None,
        }
    }

    /// Check if the ceremony is in progress
    pub fn is_in_progress(&self) -> bool {
        matches!(
            self.state,
            CeremonyState::Preparing
                | CeremonyState::PendingEpoch { .. }
                | CeremonyState::Committing
        )
    }

    /// Check if the ceremony was committed (A3 finalized)
    pub fn is_committed(&self) -> bool {
        matches!(self.state, CeremonyState::Committed { .. })
    }

    /// Check if the ceremony was aborted
    pub fn is_aborted(&self) -> bool {
        matches!(self.state, CeremonyState::Aborted { .. })
    }

    /// Check if the ceremony was superseded
    pub fn is_superseded(&self) -> bool {
        matches!(self.state, CeremonyState::Superseded { .. })
    }

    /// Check if the ceremony has reached a terminal state
    pub fn is_terminal(&self) -> bool {
        !self.is_in_progress()
    }

    /// Get the response count
    pub fn response_count(&self) -> usize {
        self.responses
            .iter()
            .filter(|r| r.response == CeremonyResponse::Accept)
            .count()
    }

    /// Get responses that are still pending
    pub fn pending_participants<'a>(&self, expected: &'a [AuthorityId]) -> Vec<&'a AuthorityId> {
        expected
            .iter()
            .filter(|p| !self.responses.iter().any(|r| &r.participant == *p))
            .collect()
    }

    /// Record a participant response
    pub fn record_response(
        &mut self,
        participant: AuthorityId,
        response: CeremonyResponse,
        at: PhysicalTime,
    ) {
        // Update if already exists, otherwise add new
        if let Some(existing) = self
            .responses
            .iter_mut()
            .find(|r| r.participant == participant)
        {
            existing.response = response;
            existing.responded_at = at;
        } else {
            self.responses.push(ParticipantResponse {
                participant,
                response,
                responded_at: at,
            });
        }
    }
}

/// Ceremony lifecycle state for Category C operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CeremonyState {
    /// Computing prestate and preparing proposal
    Preparing,

    /// Pending epoch created, collecting responses
    PendingEpoch {
        /// The pending epoch for this ceremony
        pending_epoch: Epoch,
        /// Number of responses required
        required_responses: u16,
        /// Number of responses received
        received_responses: u16,
    },

    /// All responses received, committing
    Committing,

    /// Successfully committed (A3 finalized)
    Committed {
        /// The consensus instance that confirmed this
        consensus_id: ConsensusId,
        /// When the commit was confirmed
        committed_at: PhysicalTime,
    },

    /// Aborted, no effect
    Aborted {
        /// Reason for abortion
        reason: String,
        /// When the abortion occurred
        aborted_at: PhysicalTime,
    },

    /// Superseded by another ceremony
    Superseded {
        /// The ceremony that superseded this one
        by: CeremonyId,
        /// Reason for supersession
        reason: SupersessionReason,
    },
}

impl std::fmt::Display for CeremonyState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Preparing => write!(f, "Preparing"),
            Self::PendingEpoch {
                received_responses,
                required_responses,
                ..
            } => write!(f, "Pending ({received_responses}/{required_responses})"),
            Self::Committing => write!(f, "Committing"),
            Self::Committed { .. } => write!(f, "Committed"),
            Self::Aborted { reason, .. } => write!(f, "Aborted: {reason}"),
            Self::Superseded { by, reason } => write!(f, "Superseded by {by}: {reason}"),
        }
    }
}

/// A participant's response in a ceremony.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParticipantResponse {
    /// The participant who responded
    pub participant: AuthorityId,
    /// Their response
    pub response: CeremonyResponse,
    /// When they responded
    pub responded_at: PhysicalTime,
}

/// Response from a ceremony participant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CeremonyResponse {
    /// Participant accepts the ceremony
    Accept,
    /// Participant rejects the ceremony
    Reject {
        /// Reason for rejection
        reason: String,
    },
    /// Participant timed out
    Timeout,
}

impl std::fmt::Display for CeremonyResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Accept => write!(f, "Accept"),
            Self::Reject { reason } => write!(f, "Reject: {reason}"),
            Self::Timeout => write!(f, "Timeout"),
        }
    }
}

/// Reason for ceremony supersession.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupersessionReason {
    /// Prestate became stale
    PrestateStale,
    /// A newer request was initiated
    NewerRequest,
    /// Explicit cancellation
    ExplicitCancel,
    /// Ceremony timed out
    Timeout,
    /// Higher precedence ceremony took over
    Precedence,
}

impl std::fmt::Display for SupersessionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PrestateStale => write!(f, "prestate stale"),
            Self::NewerRequest => write!(f, "newer request"),
            Self::ExplicitCancel => write!(f, "cancelled"),
            Self::Timeout => write!(f, "timeout"),
            Self::Precedence => write!(f, "higher precedence"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn test_authority(n: u8) -> AuthorityId {
        AuthorityId::from_uuid(Uuid::from_bytes([n; 16]))
    }

    fn test_time(millis: u64) -> PhysicalTime {
        PhysicalTime {
            ts_ms: millis,
            uncertainty: None,
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // OptimisticStatus Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_optimistic_status_default() {
        let status = OptimisticStatus::new();
        assert!(!status.is_finalized());
        assert!(!status.is_propagated());
        assert!(!status.is_delivered(&[]));
        assert_eq!(status.ack_count(), 0);
    }

    #[test]
    fn test_optimistic_status_with_ack_tracking() {
        let status = OptimisticStatus::with_ack_tracking();
        assert!(status.acknowledgment.is_some());
        assert_eq!(status.ack_count(), 0);
    }

    #[test]
    fn test_optimistic_status_is_delivered() {
        let peer1 = test_authority(1);
        let peer2 = test_authority(2);

        let ack = Acknowledgment::new()
            .add_ack(peer1, test_time(1000))
            .add_ack(peer2, test_time(2000));

        let status = OptimisticStatus::new().with_acknowledgment(ack);

        assert!(status.is_delivered(&[peer1]));
        assert!(status.is_delivered(&[peer1, peer2]));
        assert!(!status.is_delivered(&[peer1, peer2, test_authority(3)]));
    }

    // ─────────────────────────────────────────────────────────────────────────
    // DeferredStatus Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_deferred_status_new() {
        let status = DeferredStatus::new(
            "prop-1",
            ApprovalThreshold::Threshold {
                required: 2,
                total: 3,
            },
            test_time(10000),
        );

        assert!(status.is_pending());
        assert!(!status.is_approved());
        assert_eq!(status.approval_count(), 0);
    }

    #[test]
    fn test_approval_progress() {
        let mut progress = ApprovalProgress::new(ApprovalThreshold::Threshold {
            required: 2,
            total: 3,
        });

        progress.record(
            test_authority(1),
            ApprovalDecision::Approve,
            test_time(1000),
        );
        assert_eq!(progress.approval_count(), 1);
        assert!(!progress.is_threshold_met(3));

        progress.record(
            test_authority(2),
            ApprovalDecision::Approve,
            test_time(2000),
        );
        assert_eq!(progress.approval_count(), 2);
        assert!(progress.is_threshold_met(3));
    }

    #[test]
    fn test_approval_threshold_any() {
        let threshold = ApprovalThreshold::Any;
        assert!(!threshold.is_met(0, 5));
        assert!(threshold.is_met(1, 5));
    }

    #[test]
    fn test_approval_threshold_unanimous() {
        let threshold = ApprovalThreshold::Unanimous;
        assert!(!threshold.is_met(4, 5));
        assert!(threshold.is_met(5, 5));
    }

    #[test]
    fn test_approval_threshold_percentage() {
        let threshold = ApprovalThreshold::Percentage { percent: 50 };
        assert!(!threshold.is_met(2, 5)); // 40%
        assert!(threshold.is_met(3, 5)); // 60%
    }

    // ─────────────────────────────────────────────────────────────────────────
    // CeremonyStatus Tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_ceremony_status_new() {
        let status = CeremonyStatus::new("cer-1", Hash32([0; 32]));
        assert!(status.is_in_progress());
        assert!(!status.is_committed());
        assert!(!status.is_terminal());
    }

    #[test]
    fn test_ceremony_status_responses() {
        let mut status = CeremonyStatus::new("cer-1", Hash32([0; 32]));

        status.record_response(test_authority(1), CeremonyResponse::Accept, test_time(1000));
        status.record_response(test_authority(2), CeremonyResponse::Accept, test_time(2000));

        assert_eq!(status.response_count(), 2);

        let expected = [test_authority(1), test_authority(2), test_authority(3)];
        let pending = status.pending_participants(&expected);
        assert_eq!(pending.len(), 1);
        assert_eq!(*pending[0], test_authority(3));
    }

    #[test]
    fn test_ceremony_state_display() {
        assert_eq!(CeremonyState::Preparing.to_string(), "Preparing");
        assert_eq!(CeremonyState::Committing.to_string(), "Committing");

        let pending = CeremonyState::PendingEpoch {
            pending_epoch: Epoch::new(5),
            required_responses: 3,
            received_responses: 2,
        };
        assert_eq!(pending.to_string(), "Pending (2/3)");
    }

    #[test]
    fn test_supersession_reason_display() {
        assert_eq!(
            SupersessionReason::PrestateStale.to_string(),
            "prestate stale"
        );
        assert_eq!(
            SupersessionReason::NewerRequest.to_string(),
            "newer request"
        );
        assert_eq!(SupersessionReason::ExplicitCancel.to_string(), "cancelled");
    }
}
