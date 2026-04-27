//! Authentication Guard Types
//!
//! Guard chain integration for authentication operations.
//! All operations flow through the guard chain and return outcomes
//! for the caller to execute effects.
//!
//! # Architecture
//!
//! Guard evaluation is pure and synchronous over a prepared `GuardSnapshot`.
//! The evaluation returns `EffectCommand` data that an async interpreter executes.
//! No guard performs I/O directly.
//!
//! This module owns reusable pure guard predicates such as capability checks,
//! flow-budget checks, and recovery policy checks. `crate::service::AuthService`
//! is the canonical owner of feature-level workflow shaping and journal fact
//! construction.
//!
//! ```text
//! ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
//! │  GuardSnapshot  │ --> │  Guard Eval     │ --> │  GuardOutcome   │
//! │  (prepared      │     │  (pure, sync)   │     │  (decision +    │
//! │   async)        │     │                 │     │   effect cmds)  │
//! └─────────────────┘     └─────────────────┘     └─────────────────┘
//!                                                          │
//!                                                          v
//!                                                 ┌─────────────────┐
//!                                                 │ Effect Executor │
//!                                                 │ (async)         │
//!                                                 └─────────────────┘
//! ```

use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::DeviceId;
use aura_core::FlowCost;
use aura_guards::types;
use aura_signature::session::SessionScope;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use crate::capabilities::{
    AuthenticationCapability, GuardianAuthCapability, RecoveryAuthorizationCapability,
};

// =============================================================================
// Guard Cost Constants
// =============================================================================

/// Guard cost and capability constants for authentication operations
pub mod costs {
    use aura_core::FlowCost;

    // -------------------------------------------------------------------------
    // Flow Costs
    // -------------------------------------------------------------------------

    /// Flow cost for requesting an authentication challenge
    pub const CHALLENGE_REQUEST_COST: FlowCost = FlowCost::new(1);

    /// Flow cost for submitting an authentication proof
    pub const PROOF_SUBMISSION_COST: FlowCost = FlowCost::new(2);

    /// Flow cost for verifying an authentication proof
    pub const PROOF_VERIFICATION_COST: FlowCost = FlowCost::new(2);

    /// Flow cost for creating a session ticket
    pub const SESSION_CREATION_COST: FlowCost = FlowCost::new(2);

    /// Flow cost for guardian approval request
    pub const GUARDIAN_APPROVAL_REQUEST_COST: FlowCost = FlowCost::new(3);

    /// Flow cost for submitting guardian approval decision
    pub const GUARDIAN_APPROVAL_DECISION_COST: FlowCost = FlowCost::new(2);
}

// =============================================================================
// Guard Snapshot
// =============================================================================

/// Snapshot of guard-relevant state for evaluation.
///
/// This is prepared asynchronously before guard evaluation,
/// allowing the evaluation itself to be pure and synchronous.
#[derive(Debug, Clone)]
pub struct GuardSnapshot {
    /// Authority performing the operation
    pub authority_id: AuthorityId,

    /// Context for the operation (if applicable)
    pub context_id: Option<ContextId>,

    /// Device performing the operation (for device-level auth)
    pub device_id: Option<DeviceId>,

    /// Current flow budget remaining
    pub flow_budget_remaining: FlowCost,

    /// Capabilities held by the authority
    pub capabilities: Vec<types::CapabilityId>,

    /// Current epoch
    pub epoch: u64,

    /// Current timestamp in milliseconds
    pub now_ms: u64,

    /// Owner-provided freshness nonce for request/session identifiers.
    pub freshness_nonce: [u8; 16],

    /// Pending challenge/session identifiers reserved by the caller.
    pub pending_session_ids: HashSet<String>,

    /// Active session identifiers already issued by the caller.
    pub active_session_ids: HashSet<String>,

    /// Pending guardian approval request identifiers reserved by the caller.
    pub pending_guardian_request_ids: HashSet<String>,

    /// Trusted runtime emergency state.
    ///
    /// This is prepared by the runtime owner. Caller-supplied
    /// `RecoveryContext::is_emergency` values are request context only and must
    /// not be treated as authorization.
    pub is_emergency: bool,
}

impl GuardSnapshot {
    /// Create a new guard snapshot
    pub fn new(
        authority_id: AuthorityId,
        context_id: Option<ContextId>,
        device_id: Option<DeviceId>,
        flow_budget_remaining: FlowCost,
        capabilities: Vec<types::CapabilityId>,
        epoch: u64,
        now_ms: u64,
        freshness_nonce: [u8; 16],
    ) -> Self {
        Self {
            authority_id,
            context_id,
            device_id,
            flow_budget_remaining,
            capabilities,
            epoch,
            now_ms,
            freshness_nonce,
            pending_session_ids: HashSet::new(),
            active_session_ids: HashSet::new(),
            pending_guardian_request_ids: HashSet::new(),
            is_emergency: false,
        }
    }

    /// Create a snapshot with emergency flag
    pub fn with_emergency(mut self, is_emergency: bool) -> Self {
        self.is_emergency = is_emergency;
        self
    }

    /// Check if snapshot has a specific capability
    pub fn has_capability(&self, cap: &types::CapabilityId) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }

    /// Check if snapshot has sufficient flow budget
    pub fn has_budget(&self, cost: FlowCost) -> bool {
        self.flow_budget_remaining >= cost
    }

    pub fn session_id_available(&self, session_id: &str) -> bool {
        !self.pending_session_ids.contains(session_id)
            && !self.active_session_ids.contains(session_id)
    }

    pub fn guardian_request_id_available(&self, request_id: &str) -> bool {
        !self.pending_guardian_request_ids.contains(request_id)
    }
}

// =============================================================================
// Guard Request
// =============================================================================

/// Request to be evaluated by guards
#[derive(Debug, Clone)]
pub enum GuardRequest {
    /// Request for authentication challenge
    ChallengeRequest {
        /// Scope of requested authentication
        scope: SessionScope,
        /// Explicit caller-generated challenge/session identifier
        session_id: String,
    },

    /// Submit identity proof for verification
    ProofSubmission {
        /// Session ID from challenge
        session_id: String,
        /// Hash of the proof being submitted
        proof_hash: [u8; 32],
    },

    /// Verify submitted proof and issue session
    ProofVerification {
        /// Session ID being verified
        session_id: String,
    },

    /// Create session ticket after successful auth
    SessionCreation {
        /// Session scope requested
        scope: SessionScope,
        /// Duration in seconds
        duration_seconds: u64,
        /// Explicit caller-generated session identifier
        session_id: String,
    },

    /// Request guardian approval for recovery
    GuardianApprovalRequest {
        /// Account being recovered
        account_id: AuthorityId,
        /// Type of recovery operation
        operation_type: RecoveryOperationType,
        /// Required number of guardian approvals
        required_guardians: u32,
        /// Explicit caller-generated guardian approval request identifier
        request_id: String,
    },

    /// Submit guardian approval decision
    GuardianApprovalDecision {
        /// Request ID being responded to
        request_id: String,
        /// Whether approving or denying
        approved: bool,
    },
}

/// Types of recovery operations requiring guardian approval
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryOperationType {
    /// Device key recovery
    DeviceKeyRecovery,
    /// Account access recovery
    AccountAccessRecovery,
    /// Guardian set modification
    GuardianSetModification,
    /// Emergency account freeze
    EmergencyFreeze,
    /// Account unfreezing
    AccountUnfreeze,
}

/// Recovery context for guardian authentication
///
/// Contains all the information needed to evaluate a recovery request
/// through the guard chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryContext {
    /// Recovery operation type
    pub operation_type: RecoveryOperationType,
    /// Recovery justification
    pub justification: String,
    /// Caller-declared emergency status.
    ///
    /// This field records request intent and audit context. It does not grant
    /// authorization by itself.
    pub is_emergency: bool,
    /// Recovery timestamp (ms)
    pub timestamp: u64,
}

impl RecoveryContext {
    /// Create a new recovery context
    pub fn new(
        operation_type: RecoveryOperationType,
        justification: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            operation_type,
            justification: justification.into(),
            is_emergency: false,
            timestamp,
        }
    }

    /// Create an emergency recovery context
    pub fn emergency(
        operation_type: RecoveryOperationType,
        justification: impl Into<String>,
        timestamp: u64,
    ) -> Self {
        Self {
            operation_type,
            justification: justification.into(),
            is_emergency: true,
            timestamp,
        }
    }
}

/// Decision type shared across Layer 5 feature crates.
pub type GuardDecision = types::GuardDecision;

// =============================================================================
// Effect Command
// =============================================================================

/// Effect command to be executed after guard approval.
///
/// These commands are produced by pure guard evaluation and
/// executed asynchronously by the effect system.
#[derive(Debug, Clone)]
pub enum EffectCommand {
    /// Generate a cryptographic challenge
    GenerateChallenge {
        /// Session ID for the challenge
        session_id: String,
        /// Challenge expiration timestamp (ms)
        expires_at_ms: u64,
    },

    /// Sign a message with authority key
    SignMessage {
        /// Message to sign
        message: Vec<u8>,
        /// Context for the signature
        context: String,
    },

    /// Verify a signature
    VerifySignature {
        /// Message that was signed
        message: Vec<u8>,
        /// Signature to verify
        signature: Vec<u8>,
        /// Public key for verification
        public_key: Vec<u8>,
    },

    /// Issue a session ticket
    IssueSessionTicket {
        /// Session ID
        session_id: String,
        /// Session scope
        scope: SessionScope,
        /// Expiration timestamp (ms)
        expires_at_ms: u64,
    },

    /// Charge flow budget
    ChargeFlowBudget {
        /// Cost to charge
        cost: FlowCost,
    },

    /// Append fact to journal
    JournalAppend {
        /// Fact type identifier
        fact_type: String,
        /// Serialized fact data
        fact_data: Vec<u8>,
    },

    /// Notify peer about authentication event
    NotifyPeer {
        /// Peer to notify
        peer: AuthorityId,
        /// Event type
        event_type: String,
        /// Event data
        event_data: Vec<u8>,
    },

    /// Record receipt for audit
    RecordReceipt {
        /// Operation name
        operation: String,
        /// Peer involved (if any)
        peer: Option<AuthorityId>,
        /// Timestamp
        timestamp_ms: u64,
    },

    /// Send guardian challenge
    SendGuardianChallenge {
        /// Guardian to challenge
        guardian_id: AuthorityId,
        /// Request ID
        request_id: String,
        /// Challenge bytes
        challenge: Vec<u8>,
        /// Expiration timestamp (ms)
        expires_at_ms: u64,
    },

    /// Aggregate guardian approvals
    AggregateGuardianApprovals {
        /// Request ID
        request_id: String,
        /// Required threshold
        threshold: u32,
    },
}

/// Outcome type shared across Layer 5 feature crates.
pub type GuardOutcome = types::GuardOutcome<EffectCommand>;

/// Typed guard rejection for consistent error reporting.
#[derive(Debug, Clone, Copy)]
pub struct GuardReject {
    pub code: &'static str,
    pub category: &'static str,
    pub message: &'static str,
}

const DUPLICATE_SESSION_ID: GuardReject = GuardReject {
    code: "duplicate-session-id",
    category: "auth",
    message: "Session or challenge identifier is already pending or active",
};

const DUPLICATE_GUARDIAN_REQUEST_ID: GuardReject = GuardReject {
    code: "duplicate-guardian-request-id",
    category: "auth",
    message: "Guardian approval request identifier is already pending",
};

impl std::fmt::Display for GuardReject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}:{}] {}", self.category, self.code, self.message)
    }
}

fn deny(reject: GuardReject) -> GuardOutcome {
    GuardOutcome::denied(types::GuardViolation::other(reject.to_string()))
}

// =============================================================================
// Guard Helpers
// =============================================================================

impl types::CapabilitySnapshot for GuardSnapshot {
    fn has_capability(&self, cap: &types::CapabilityId) -> bool {
        GuardSnapshot::has_capability(self, cap)
    }
}

impl types::FlowBudgetSnapshot for GuardSnapshot {
    fn flow_budget_remaining(&self) -> FlowCost {
        self.flow_budget_remaining
    }
}

/// Check capability and return denied outcome if missing
pub fn check_capability(
    snapshot: &GuardSnapshot,
    required_cap: &types::CapabilityId,
) -> Option<GuardOutcome> {
    if snapshot.has_capability(required_cap) {
        None
    } else {
        Some(deny(GuardReject {
            code: "capability-missing",
            category: "auth",
            message: "Required capability missing",
        }))
    }
}

/// Check flow budget and return denied outcome if insufficient
pub fn check_flow_budget(
    snapshot: &GuardSnapshot,
    required_cost: FlowCost,
) -> Option<GuardOutcome> {
    if snapshot.flow_budget_remaining >= required_cost {
        None
    } else {
        Some(deny(GuardReject {
            code: "flow-budget-insufficient",
            category: "auth",
            message: "Flow budget insufficient",
        }))
    }
}

/// Check if challenge has expired
pub fn check_challenge_expiry(
    snapshot: &GuardSnapshot,
    expires_at_ms: u64,
) -> Option<GuardOutcome> {
    if snapshot.now_ms > expires_at_ms {
        Some(deny(GuardReject {
            code: "challenge-expired",
            category: "auth",
            message: "Challenge has expired",
        }))
    } else {
        None
    }
}

/// Check if session duration is within acceptable bounds
pub fn check_session_duration(
    duration_seconds: u64,
    max_duration_seconds: u64,
) -> Option<GuardOutcome> {
    if duration_seconds > max_duration_seconds {
        Some(deny(GuardReject {
            code: "session-duration-too-long",
            category: "auth",
            message: "Session duration exceeds maximum",
        }))
    } else {
        None
    }
}

/// Check if recovery operation is allowed for the given operation type
pub fn check_recovery_operation(
    snapshot: &GuardSnapshot,
    operation_type: &RecoveryOperationType,
    caller_declared_emergency: bool,
    require_recovery_capability: bool,
) -> Option<GuardOutcome> {
    if !require_recovery_capability {
        return None;
    }

    let _ = caller_declared_emergency;
    let trusted_emergency_freeze =
        snapshot.is_emergency && matches!(operation_type, RecoveryOperationType::EmergencyFreeze);
    if trusted_emergency_freeze {
        return None;
    }

    match operation_type {
        RecoveryOperationType::GuardianSetModification => {
            // Guardian set modifications require explicit capability
            if !snapshot.has_capability(&RecoveryAuthorizationCapability::Approve.as_name()) {
                return Some(deny(GuardReject {
                    code: "guardian-set-approval-required",
                    category: "auth",
                    message: "Guardian set modification requires recovery:approve capability",
                }));
            }
        }
        RecoveryOperationType::EmergencyFreeze => {
            // Emergency freeze requires emergency flag or explicit capability
            if !snapshot.has_capability(&RecoveryAuthorizationCapability::Initiate.as_name()) {
                return Some(deny(GuardReject {
                    code: "emergency-freeze-requires-capability",
                    category: "auth",
                    message: "Emergency freeze requires recovery:initiate capability",
                }));
            }
        }
        _ => {
            // Other operations allowed with standard capabilities
        }
    }

    None
}

/// Check capability and flow budget together using the shared pure guard
/// predicates owned by this module.
pub fn check_capability_and_flow_budget(
    snapshot: &GuardSnapshot,
    required_cap: &types::CapabilityId,
    required_cost: FlowCost,
) -> Option<GuardOutcome> {
    check_capability(snapshot, required_cap).or_else(|| check_flow_budget(snapshot, required_cost))
}

// =============================================================================
// Guard Evaluator
// =============================================================================

/// Evaluate a guard request against a snapshot
///
/// This request-shaped evaluator is a lightweight pure facade for tests and
/// simple callers. `crate::service::AuthService` remains the canonical owner of
/// feature workflow shaping, stable id generation, and auth fact construction.
pub fn evaluate_request(snapshot: &GuardSnapshot, request: &GuardRequest) -> GuardOutcome {
    match request {
        GuardRequest::ChallengeRequest {
            scope: _,
            session_id,
        } => {
            if let Some(outcome) = check_capability_and_flow_budget(
                snapshot,
                &AuthenticationCapability::Request.as_name(),
                costs::CHALLENGE_REQUEST_COST,
            ) {
                return outcome;
            }

            if !snapshot.session_id_available(session_id) {
                return deny(DUPLICATE_SESSION_ID);
            }
            let expires_at_ms = snapshot.now_ms + 300_000; // 5 minutes

            GuardOutcome::allowed(vec![
                EffectCommand::ChargeFlowBudget {
                    cost: costs::CHALLENGE_REQUEST_COST,
                },
                EffectCommand::GenerateChallenge {
                    session_id: session_id.clone(),
                    expires_at_ms,
                },
            ])
        }

        GuardRequest::ProofSubmission {
            session_id,
            proof_hash,
        } => {
            if let Some(outcome) = check_capability_and_flow_budget(
                snapshot,
                &AuthenticationCapability::SubmitProof.as_name(),
                costs::PROOF_SUBMISSION_COST,
            ) {
                return outcome;
            }

            GuardOutcome::allowed(vec![
                EffectCommand::ChargeFlowBudget {
                    cost: costs::PROOF_SUBMISSION_COST,
                },
                EffectCommand::JournalAppend {
                    fact_type: "auth_proof_submitted".to_string(),
                    fact_data: proof_hash.to_vec(),
                },
                EffectCommand::RecordReceipt {
                    operation: format!("proof_submission:{session_id}"),
                    peer: None,
                    timestamp_ms: snapshot.now_ms,
                },
            ])
        }

        GuardRequest::ProofVerification { session_id } => {
            if let Some(outcome) = check_capability_and_flow_budget(
                snapshot,
                &AuthenticationCapability::Verify.as_name(),
                costs::PROOF_VERIFICATION_COST,
            ) {
                return outcome;
            }

            GuardOutcome::allowed(vec![
                EffectCommand::ChargeFlowBudget {
                    cost: costs::PROOF_VERIFICATION_COST,
                },
                EffectCommand::JournalAppend {
                    fact_type: "auth_verification_started".to_string(),
                    fact_data: session_id.as_bytes().to_vec(),
                },
            ])
        }

        GuardRequest::SessionCreation {
            scope,
            duration_seconds,
            session_id,
        } => {
            if let Some(outcome) = check_capability_and_flow_budget(
                snapshot,
                &AuthenticationCapability::CreateSession.as_name(),
                costs::SESSION_CREATION_COST,
            ) {
                return outcome;
            }

            // Check duration
            if let Some(outcome) = check_session_duration(*duration_seconds, 86_400) {
                return outcome;
            }

            if !snapshot.session_id_available(session_id) {
                return deny(DUPLICATE_SESSION_ID);
            }
            let expires_at_ms = snapshot.now_ms + (duration_seconds * 1000);

            GuardOutcome::allowed(vec![
                EffectCommand::ChargeFlowBudget {
                    cost: costs::SESSION_CREATION_COST,
                },
                EffectCommand::IssueSessionTicket {
                    session_id: session_id.clone(),
                    scope: scope.clone(),
                    expires_at_ms,
                },
            ])
        }

        GuardRequest::GuardianApprovalRequest {
            account_id,
            operation_type,
            required_guardians,
            request_id,
        } => {
            if let Some(outcome) = check_capability_and_flow_budget(
                snapshot,
                &GuardianAuthCapability::RequestApproval.as_name(),
                costs::GUARDIAN_APPROVAL_REQUEST_COST,
            ) {
                return outcome;
            }

            // Check recovery operation type
            if let Some(outcome) =
                check_recovery_operation(snapshot, operation_type, snapshot.is_emergency, true)
            {
                return outcome;
            }

            if !snapshot.guardian_request_id_available(request_id) {
                return deny(DUPLICATE_GUARDIAN_REQUEST_ID);
            }

            GuardOutcome::allowed(vec![
                EffectCommand::ChargeFlowBudget {
                    cost: costs::GUARDIAN_APPROVAL_REQUEST_COST,
                },
                EffectCommand::JournalAppend {
                    fact_type: "guardian_approval_requested".to_string(),
                    fact_data: request_id.as_bytes().to_vec(),
                },
                EffectCommand::AggregateGuardianApprovals {
                    request_id: request_id.clone(),
                    threshold: *required_guardians,
                },
                EffectCommand::NotifyPeer {
                    peer: *account_id,
                    event_type: "guardian_approval_request".to_string(),
                    event_data: request_id.as_bytes().to_vec(),
                },
            ])
        }

        GuardRequest::GuardianApprovalDecision {
            request_id,
            approved,
        } => {
            if let Some(outcome) = check_capability_and_flow_budget(
                snapshot,
                &GuardianAuthCapability::Verify.as_name(),
                costs::GUARDIAN_APPROVAL_DECISION_COST,
            ) {
                return outcome;
            }

            let fact_type = if *approved {
                "guardian_approved"
            } else {
                "guardian_denied"
            };

            GuardOutcome::allowed(vec![
                EffectCommand::ChargeFlowBudget {
                    cost: costs::GUARDIAN_APPROVAL_DECISION_COST,
                },
                EffectCommand::JournalAppend {
                    fact_type: fact_type.to_string(),
                    fact_data: request_id.as_bytes().to_vec(),
                },
                EffectCommand::RecordReceipt {
                    operation: format!("guardian_decision:{request_id}:{approved}"),
                    peer: Some(snapshot.authority_id),
                    timestamp_ms: snapshot.now_ms,
                },
            ])
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{protocol_scope, standard_guard_snapshot};

    #[test]
    fn test_guard_snapshot_has_capability() {
        let snapshot = standard_guard_snapshot();
        assert!(snapshot.has_capability(&AuthenticationCapability::Request.as_name()));
        assert!(snapshot.has_capability(&AuthenticationCapability::SubmitProof.as_name()));
        assert!(!snapshot.has_capability(&GuardianAuthCapability::RequestApproval.as_name()));
    }

    #[test]
    fn test_guard_snapshot_has_budget() {
        let snapshot = standard_guard_snapshot();
        assert!(snapshot.has_budget(FlowCost::new(50)));
        assert!(snapshot.has_budget(FlowCost::new(100)));
        assert!(!snapshot.has_budget(FlowCost::new(101)));
    }

    #[test]
    fn test_guard_decision_allow() {
        let decision = GuardDecision::allow();
        assert!(decision.is_allowed());
        assert!(!decision.is_denied());
        assert!(decision.denial_reason().is_none());
    }

    #[test]
    fn test_guard_decision_deny() {
        let decision = GuardDecision::deny(types::GuardViolation::other("test reason"));
        assert!(!decision.is_allowed());
        assert!(decision.is_denied());
        assert!(matches!(
            decision.denial_reason(),
            Some(types::GuardViolation::Other(reason)) if reason == "test reason"
        ));
    }

    #[test]
    fn test_guard_outcome_allowed() {
        let outcome = GuardOutcome::allowed(vec![EffectCommand::ChargeFlowBudget {
            cost: FlowCost::new(10),
        }]);
        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 1);
    }

    #[test]
    fn test_guard_outcome_denied() {
        let outcome = GuardOutcome::denied(types::GuardViolation::other("no budget"));
        assert!(outcome.is_denied());
        assert!(outcome.effects.is_empty());
    }

    #[test]
    fn test_check_capability_success() {
        let snapshot = standard_guard_snapshot();
        let result = check_capability(&snapshot, &AuthenticationCapability::Request.as_name());
        assert!(result.is_none());
    }

    #[test]
    fn test_check_capability_failure() {
        let snapshot = standard_guard_snapshot();
        let result = check_capability(
            &snapshot,
            &GuardianAuthCapability::RequestApproval.as_name(),
        );
        assert!(result.is_some());
        assert!(result.unwrap().is_denied());
    }

    #[test]
    fn test_check_flow_budget_success() {
        let snapshot = standard_guard_snapshot();
        let result = check_flow_budget(&snapshot, FlowCost::new(50));
        assert!(result.is_none());
    }

    #[test]
    fn test_check_flow_budget_failure() {
        let snapshot = standard_guard_snapshot();
        let result = check_flow_budget(&snapshot, FlowCost::new(150));
        assert!(result.is_some());
        assert!(result.unwrap().is_denied());
    }

    /// Challenge request succeeds with required capability and budget.
    #[test]
    fn test_evaluate_challenge_request() {
        let snapshot = standard_guard_snapshot();
        let request = GuardRequest::ChallengeRequest {
            scope: protocol_scope("test"),
            session_id: "challenge_session_1".to_string(),
        };

        let outcome = evaluate_request(&snapshot, &request);
        assert!(outcome.is_allowed());
        assert!(!outcome.effects.is_empty());
    }

    /// Session creation denied when requested duration exceeds maximum —
    /// prevents unbounded session lifetimes.
    #[test]
    fn test_evaluate_session_creation_duration_exceeded() {
        let snapshot = standard_guard_snapshot();
        let request = GuardRequest::SessionCreation {
            scope: protocol_scope("test"),
            duration_seconds: 100_000, // > 24 hours
            session_id: "session_1".to_string(),
        };

        let outcome = evaluate_request(&snapshot, &request);
        assert!(outcome.is_denied());
    }

    #[test]
    fn test_evaluate_challenge_request_rejects_duplicate_session_id() {
        let mut snapshot = standard_guard_snapshot();
        snapshot
            .pending_session_ids
            .insert("challenge_session_1".to_string());
        let request = GuardRequest::ChallengeRequest {
            scope: protocol_scope("test"),
            session_id: "challenge_session_1".to_string(),
        };

        let outcome = evaluate_request(&snapshot, &request);
        assert!(outcome.is_denied());
    }

    #[test]
    fn test_evaluate_guardian_request_rejects_duplicate_request_id() {
        let mut snapshot = standard_guard_snapshot();
        snapshot
            .capabilities
            .push(GuardianAuthCapability::RequestApproval.as_name());
        snapshot
            .pending_guardian_request_ids
            .insert("guardian_req_1".to_string());
        let request = GuardRequest::GuardianApprovalRequest {
            account_id: aura_testkit::test_builders::authority_id(4),
            operation_type: RecoveryOperationType::DeviceKeyRecovery,
            required_guardians: 2,
            request_id: "guardian_req_1".to_string(),
        };

        let outcome = evaluate_request(&snapshot, &request);
        assert!(outcome.is_denied());
    }

    #[test]
    fn caller_declared_emergency_does_not_bypass_recovery_capability() {
        let snapshot = standard_guard_snapshot();

        let outcome = check_recovery_operation(
            &snapshot,
            &RecoveryOperationType::EmergencyFreeze,
            true,
            true,
        );

        assert!(outcome.is_some_and(|outcome| outcome.is_denied()));
    }

    #[test]
    fn trusted_emergency_snapshot_allows_emergency_freeze_without_capability() {
        let snapshot = standard_guard_snapshot().with_emergency(true);

        let outcome = check_recovery_operation(
            &snapshot,
            &RecoveryOperationType::EmergencyFreeze,
            false,
            true,
        );

        assert!(outcome.is_none());
    }

    #[test]
    fn trusted_emergency_snapshot_does_not_bypass_other_recovery_operations() {
        let snapshot = standard_guard_snapshot().with_emergency(true);

        let outcome = check_recovery_operation(
            &snapshot,
            &RecoveryOperationType::GuardianSetModification,
            false,
            true,
        );

        assert!(outcome.is_some_and(|outcome| outcome.is_denied()));
    }

    /// Expired challenges are rejected — prevents replay of stale challenges.
    #[test]
    fn test_check_challenge_expiry() {
        let snapshot = standard_guard_snapshot();

        // Not expired
        let result = check_challenge_expiry(&snapshot, 2000);
        assert!(result.is_none());

        // Expired
        let result = check_challenge_expiry(&snapshot, 500);
        assert!(result.is_some());
        assert!(result.unwrap().is_denied());
    }

    #[test]
    fn test_guard_costs_defined() {
        assert_eq!(costs::CHALLENGE_REQUEST_COST.value(), 1);
        assert_eq!(costs::PROOF_SUBMISSION_COST.value(), 2);
        assert_eq!(
            AuthenticationCapability::Request.as_name().as_str(),
            "auth:request"
        );
    }
}
