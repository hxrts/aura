//! Invitation Guard Types
//!
//! Guard chain integration for invitation operations.
//! All operations flow through the guard chain and return outcomes
//! for the caller to execute effects.
//!
//! # Architecture
//!
//! Guard evaluation is pure and synchronous over a prepared `GuardSnapshot`.
//! The evaluation returns `EffectCommand` data that an async interpreter executes.
//! No guard performs I/O directly.
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

use aura_core::identifiers::{AuthorityId, ContextId};
use serde::{Deserialize, Serialize};

use crate::facts::InvitationFact;

// =============================================================================
// Guard Cost Constants
// =============================================================================

/// Guard cost and capability constants for invitation operations
pub mod costs {
    /// Flow cost for sending an invitation
    pub const INVITATION_SEND_COST: u32 = 1;

    /// Flow cost for accepting an invitation
    pub const INVITATION_ACCEPT_COST: u32 = 1;

    /// Flow cost for declining an invitation
    pub const INVITATION_DECLINE_COST: u32 = 1;

    /// Flow cost for cancelling an invitation
    pub const INVITATION_CANCEL_COST: u32 = 1;

    /// Required capability for sending invitations
    pub const CAP_INVITATION_SEND: &str = "invitation:send";

    /// Required capability for accepting invitations
    pub const CAP_INVITATION_ACCEPT: &str = "invitation:accept";

    /// Required capability for declining invitations
    pub const CAP_INVITATION_DECLINE: &str = "invitation:decline";

    /// Required capability for cancelling invitations
    pub const CAP_INVITATION_CANCEL: &str = "invitation:cancel";

    /// Required capability for guardian invitations specifically
    pub const CAP_GUARDIAN_INVITE: &str = "invitation:guardian";

    /// Required capability for channel invitations specifically
    pub const CAP_CHANNEL_INVITE: &str = "invitation:channel";
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

    /// Context for the operation
    pub context_id: ContextId,

    /// Current flow budget remaining
    pub flow_budget_remaining: u32,

    /// Capabilities held by the authority
    pub capabilities: Vec<String>,

    /// Current epoch
    pub epoch: u64,

    /// Current timestamp in milliseconds
    pub now_ms: u64,
}

impl GuardSnapshot {
    /// Create a new guard snapshot
    pub fn new(
        authority_id: AuthorityId,
        context_id: ContextId,
        flow_budget_remaining: u32,
        capabilities: Vec<String>,
        epoch: u64,
        now_ms: u64,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            flow_budget_remaining,
            capabilities,
            epoch,
            now_ms,
        }
    }

    /// Check if snapshot has a specific capability
    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }

    /// Check if snapshot has sufficient flow budget
    pub fn has_budget(&self, cost: u32) -> bool {
        self.flow_budget_remaining >= cost
    }
}

// =============================================================================
// Guard Request
// =============================================================================

/// Request to be evaluated by guards
#[derive(Debug, Clone)]
pub enum GuardRequest {
    /// Sending an invitation
    SendInvitation {
        receiver_id: AuthorityId,
        invitation_type: String,
        expires_at_ms: Option<u64>,
    },

    /// Accepting an invitation
    AcceptInvitation { invitation_id: String },

    /// Declining an invitation
    DeclineInvitation { invitation_id: String },

    /// Cancelling an invitation
    CancelInvitation { invitation_id: String },
}

// =============================================================================
// Guard Decision
// =============================================================================

/// Decision from guard evaluation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GuardDecision {
    /// Operation is allowed
    Allow,

    /// Operation is denied with reason
    Deny {
        /// Reason for denial
        reason: String,
    },
}

impl GuardDecision {
    /// Create an allow decision
    pub fn allow() -> Self {
        GuardDecision::Allow
    }

    /// Create a deny decision with reason
    pub fn deny(reason: impl Into<String>) -> Self {
        GuardDecision::Deny {
            reason: reason.into(),
        }
    }

    /// Check if decision allows the operation
    pub fn is_allowed(&self) -> bool {
        matches!(self, GuardDecision::Allow)
    }

    /// Check if decision denies the operation
    pub fn is_denied(&self) -> bool {
        !self.is_allowed()
    }

    /// Get denial reason if denied
    pub fn denial_reason(&self) -> Option<&str> {
        match self {
            GuardDecision::Deny { reason } => Some(reason),
            GuardDecision::Allow => None,
        }
    }
}

// =============================================================================
// Effect Command
// =============================================================================

/// Effect command to be executed after guard approval.
///
/// These commands are produced by pure guard evaluation and
/// executed asynchronously by the effect system.
#[derive(Debug, Clone)]
pub enum EffectCommand {
    /// Append fact to journal
    JournalAppend {
        /// The invitation fact to append
        fact: InvitationFact,
    },

    /// Charge flow budget
    ChargeFlowBudget {
        /// Cost to charge
        cost: u32,
    },

    /// Notify peer about invitation
    NotifyPeer {
        /// Peer to notify
        peer: AuthorityId,
        /// Invitation ID
        invitation_id: String,
    },

    /// Record receipt for operation
    RecordReceipt {
        /// Operation name
        operation: String,
        /// Peer involved (if any)
        peer: Option<AuthorityId>,
    },
}

// =============================================================================
// Guard Outcome
// =============================================================================

/// Outcome of guard evaluation.
///
/// Contains the decision and effect commands to execute if allowed.
#[derive(Debug, Clone)]
pub struct GuardOutcome {
    /// The decision (allow/deny)
    pub decision: GuardDecision,

    /// Effect commands to execute if allowed
    pub effects: Vec<EffectCommand>,
}

impl GuardOutcome {
    /// Create an allowed outcome with effects
    pub fn allowed(effects: Vec<EffectCommand>) -> Self {
        Self {
            decision: GuardDecision::Allow,
            effects,
        }
    }

    /// Create a denied outcome
    pub fn denied(reason: impl Into<String>) -> Self {
        Self {
            decision: GuardDecision::Deny {
                reason: reason.into(),
            },
            effects: vec![],
        }
    }

    /// Check if outcome allows the operation
    pub fn is_allowed(&self) -> bool {
        self.decision.is_allowed()
    }

    /// Check if outcome denies the operation
    pub fn is_denied(&self) -> bool {
        self.decision.is_denied()
    }
}

// =============================================================================
// Guard Helpers
// =============================================================================

/// Check capability and return denied outcome if missing
pub fn check_capability(snapshot: &GuardSnapshot, required_cap: &str) -> Option<GuardOutcome> {
    if !snapshot.has_capability(required_cap) {
        Some(GuardOutcome::denied(format!(
            "Missing capability: {}",
            required_cap
        )))
    } else {
        None
    }
}

/// Check flow budget and return denied outcome if insufficient
pub fn check_flow_budget(snapshot: &GuardSnapshot, required_cost: u32) -> Option<GuardOutcome> {
    if !snapshot.has_budget(required_cost) {
        Some(GuardOutcome::denied(format!(
            "Insufficient flow budget: need {}, have {}",
            required_cost, snapshot.flow_budget_remaining
        )))
    } else {
        None
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_context() -> ContextId {
        ContextId::new_from_entropy([2u8; 32])
    }

    fn test_snapshot() -> GuardSnapshot {
        GuardSnapshot::new(
            test_authority(),
            test_context(),
            100,
            vec![
                costs::CAP_INVITATION_SEND.to_string(),
                costs::CAP_INVITATION_ACCEPT.to_string(),
            ],
            1,
            1000,
        )
    }

    #[test]
    fn test_guard_snapshot_has_capability() {
        let snapshot = test_snapshot();
        assert!(snapshot.has_capability(costs::CAP_INVITATION_SEND));
        assert!(snapshot.has_capability(costs::CAP_INVITATION_ACCEPT));
        assert!(!snapshot.has_capability(costs::CAP_GUARDIAN_INVITE));
    }

    #[test]
    fn test_guard_snapshot_has_budget() {
        let snapshot = test_snapshot();
        assert!(snapshot.has_budget(50));
        assert!(snapshot.has_budget(100));
        assert!(!snapshot.has_budget(101));
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
        let decision = GuardDecision::deny("test reason");
        assert!(!decision.is_allowed());
        assert!(decision.is_denied());
        assert_eq!(decision.denial_reason(), Some("test reason"));
    }

    #[test]
    fn test_guard_outcome_allowed() {
        let outcome = GuardOutcome::allowed(vec![EffectCommand::ChargeFlowBudget { cost: 10 }]);
        assert!(outcome.is_allowed());
        assert_eq!(outcome.effects.len(), 1);
    }

    #[test]
    fn test_guard_outcome_denied() {
        let outcome = GuardOutcome::denied("no budget");
        assert!(outcome.is_denied());
        assert!(outcome.effects.is_empty());
    }

    #[test]
    fn test_check_capability_success() {
        let snapshot = test_snapshot();
        let result = check_capability(&snapshot, costs::CAP_INVITATION_SEND);
        assert!(result.is_none()); // None means check passed
    }

    #[test]
    fn test_check_capability_failure() {
        let snapshot = test_snapshot();
        let result = check_capability(&snapshot, costs::CAP_GUARDIAN_INVITE);
        assert!(result.is_some());
        assert!(result.unwrap().is_denied());
    }

    #[test]
    fn test_check_flow_budget_success() {
        let snapshot = test_snapshot();
        let result = check_flow_budget(&snapshot, 50);
        assert!(result.is_none()); // None means check passed
    }

    #[test]
    fn test_check_flow_budget_failure() {
        let snapshot = test_snapshot();
        let result = check_flow_budget(&snapshot, 150);
        assert!(result.is_some());
        assert!(result.unwrap().is_denied());
    }

    #[test]
    fn test_guard_costs_defined() {
        assert_eq!(costs::INVITATION_SEND_COST, 1);
        assert_eq!(costs::INVITATION_ACCEPT_COST, 1);
        assert_eq!(costs::CAP_INVITATION_SEND, "invitation:send");
    }
}
