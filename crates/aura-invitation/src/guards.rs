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

use aura_core::types::identifiers::{AuthorityId, ContextId, InvitationId};
use aura_core::FlowCost;
use aura_guards::types;

use crate::facts::InvitationFact;
use crate::{InvitationOperation, InvitationType};

// =============================================================================
// Guard Cost Constants
// =============================================================================

/// Guard cost constants for invitation operations
pub mod costs {
    use aura_core::FlowCost;

    /// Flow cost for sending an invitation
    pub const INVITATION_SEND_COST: FlowCost = FlowCost::new(1);

    /// Flow cost for accepting an invitation
    pub const INVITATION_ACCEPT_COST: FlowCost = FlowCost::new(1);

    /// Flow cost for declining an invitation
    pub const INVITATION_DECLINE_COST: FlowCost = FlowCost::new(1);

    /// Flow cost for cancelling an invitation
    pub const INVITATION_CANCEL_COST: FlowCost = FlowCost::new(1);
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
    pub flow_budget_remaining: FlowCost,

    /// Capabilities held by the authority
    pub capabilities: Vec<types::CapabilityId>,

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
        flow_budget_remaining: FlowCost,
        capabilities: Vec<types::CapabilityId>,
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
    pub fn has_capability(&self, cap: &types::CapabilityId) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }

    /// Check if snapshot has sufficient flow budget
    pub fn has_budget(&self, cost: FlowCost) -> bool {
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
        invitation_type: Box<InvitationType>,
        expires_at_ms: Option<u64>,
    },

    /// Accepting an invitation
    AcceptInvitation { invitation_id: InvitationId },

    /// Declining an invitation
    DeclineInvitation { invitation_id: InvitationId },

    /// Cancelling an invitation
    CancelInvitation { invitation_id: InvitationId },
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
#[allow(clippy::large_enum_variant)] // InvitationFact in JournalAppend variant is inherently large
pub enum EffectCommand {
    /// Append fact to journal
    JournalAppend {
        /// The invitation fact to append
        fact: InvitationFact,
    },

    /// Charge flow budget
    ChargeFlowBudget {
        /// Cost to charge
        cost: FlowCost,
    },

    /// Notify peer about invitation
    NotifyPeer {
        /// Peer to notify
        peer: AuthorityId,
        /// Invitation ID
        invitation_id: InvitationId,
    },

    /// Record receipt for operation
    RecordReceipt {
        /// Typed invitation operation
        operation: InvitationOperation,
        /// Peer involved (if any)
        peer: Option<AuthorityId>,
    },
}

/// Outcome type shared across Layer 5 feature crates.
pub type GuardOutcome = types::GuardOutcome<EffectCommand>;

const INVITATION_GUARD_ACCEPT_EXECUTION_PLAN_CAPABILITY: &str =
    "invitation_guard_accept_execution_plan";
const INVITATION_GUARD_SEND_EXECUTION_PLAN_CAPABILITY: &str =
    "invitation_guard_send_execution_plan";

/// Pure execution plan derived from an invitation guard outcome.
///
/// Layer 6 runtimes interpret this plan, but the partitioning itself is
/// invitation-domain logic and belongs with the invitation guard surface.
#[derive(Debug, Clone)]
pub struct InvitationEffectExecutionPlan {
    /// Effects the runtime executes on the authoritative local path immediately.
    pub local_effects: Vec<EffectCommand>,
    /// Effects intentionally deferred to later network/timeout handling.
    pub deferred_network_effects: Vec<EffectCommand>,
}

impl InvitationEffectExecutionPlan {
    #[must_use]
    pub fn new(
        local_effects: Vec<EffectCommand>,
        deferred_network_effects: Vec<EffectCommand>,
    ) -> Self {
        Self {
            local_effects,
            deferred_network_effects,
        }
    }
}

/// Typed guard rejection for consistent error reporting.
#[derive(Debug, Clone, Copy)]
pub struct GuardReject {
    pub code: &'static str,
    pub category: &'static str,
    pub message: &'static str,
}

impl std::fmt::Display for GuardReject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}:{}] {}", self.category, self.code, self.message)
    }
}

fn deny(reject: GuardReject) -> GuardOutcome {
    GuardOutcome::denied(types::GuardViolation::other(reject.to_string()))
}

/// Human-readable denial reason extracted from a guard outcome.
#[must_use]
pub fn denial_reason(outcome: &GuardOutcome) -> String {
    outcome
        .decision
        .denial_reason()
        .map(ToString::to_string)
        .unwrap_or_else(|| "Operation denied".to_string())
}

// =============================================================================
// Guard Helpers
// =============================================================================

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
            category: "invitation",
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
            category: "invitation",
            message: "Flow budget insufficient",
        }))
    }
}

/// Accept keeps flow-budget charging and receipt recording local so the
/// authoritative accept settlement stays atomic even if the peer notification is
/// deferred or fails later.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "invitation_guard_accept_execution_plan",
    family = "runtime_helper"
)]
pub fn plan_accept_execution(
    outcome: GuardOutcome,
) -> Result<InvitationEffectExecutionPlan, String> {
    let _ = INVITATION_GUARD_ACCEPT_EXECUTION_PLAN_CAPABILITY;
    if outcome.is_denied() {
        return Err(denial_reason(&outcome));
    }

    let mut local_effects = Vec::new();
    let mut deferred_network_effects = Vec::new();
    for command in outcome.effects {
        match command {
            EffectCommand::NotifyPeer { .. } => deferred_network_effects.push(command),
            EffectCommand::ChargeFlowBudget { .. }
            | EffectCommand::JournalAppend { .. }
            | EffectCommand::RecordReceipt { .. } => local_effects.push(command),
        }
    }

    Ok(InvitationEffectExecutionPlan::new(
        local_effects,
        deferred_network_effects,
    ))
}

/// Send defers every outwardly visible side effect except the journal append so
/// invitation creation can publish the authoritative pending fact before budget,
/// receipt, and network effects run on their own timeout policy.
#[aura_macros::capability_boundary(
    category = "capability_gated",
    capability = "invitation_guard_send_execution_plan",
    family = "runtime_helper"
)]
pub fn plan_send_execution(outcome: GuardOutcome) -> Result<InvitationEffectExecutionPlan, String> {
    let _ = INVITATION_GUARD_SEND_EXECUTION_PLAN_CAPABILITY;
    if outcome.is_denied() {
        return Err(denial_reason(&outcome));
    }

    let mut local_effects = Vec::new();
    let mut deferred_network_effects = Vec::new();
    for command in outcome.effects {
        match command {
            EffectCommand::ChargeFlowBudget { .. }
            | EffectCommand::NotifyPeer { .. }
            | EffectCommand::RecordReceipt { .. } => deferred_network_effects.push(command),
            EffectCommand::JournalAppend { .. } => local_effects.push(command),
        }
    }

    Ok(InvitationEffectExecutionPlan::new(
        local_effects,
        deferred_network_effects,
    ))
}

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

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::InvitationCapability;
    use aura_core::FlowCost;

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
            FlowCost::new(100),
            vec![
                InvitationCapability::Send.as_name(),
                InvitationCapability::Accept.as_name(),
            ],
            1,
            1000,
        )
    }

    #[test]
    fn test_guard_snapshot_has_capability() {
        let snapshot = test_snapshot();
        assert!(snapshot.has_capability(&InvitationCapability::Send.as_name()));
        assert!(snapshot.has_capability(&InvitationCapability::Accept.as_name()));
        assert!(!snapshot.has_capability(&InvitationCapability::Guardian.as_name()));
    }

    #[test]
    fn test_guard_snapshot_has_budget() {
        let snapshot = test_snapshot();
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
        let snapshot = test_snapshot();
        let result = check_capability(&snapshot, &InvitationCapability::Send.as_name());
        assert!(result.is_none()); // None means check passed
    }

    #[test]
    fn test_check_capability_failure() {
        let snapshot = test_snapshot();
        let result = check_capability(&snapshot, &InvitationCapability::Guardian.as_name());
        assert!(result.is_some());
        assert!(result.unwrap().is_denied());
    }

    #[test]
    fn test_check_flow_budget_success() {
        let snapshot = test_snapshot();
        let result = check_flow_budget(&snapshot, FlowCost::new(50));
        assert!(result.is_none()); // None means check passed
    }

    #[test]
    fn test_check_flow_budget_failure() {
        let snapshot = test_snapshot();
        let result = check_flow_budget(&snapshot, FlowCost::new(150));
        assert!(result.is_some());
        assert!(result.unwrap().is_denied());
    }

    #[test]
    fn test_guard_costs_defined() {
        assert_eq!(costs::INVITATION_SEND_COST.value(), 1);
        assert_eq!(costs::INVITATION_ACCEPT_COST.value(), 1);
        assert_eq!(
            InvitationCapability::Send.as_name().as_str(),
            "invitation:send"
        );
    }

    #[test]
    fn test_plan_accept_execution_defers_only_peer_notification() {
        let outcome = GuardOutcome::allowed(vec![
            EffectCommand::ChargeFlowBudget {
                cost: FlowCost::new(1),
            },
            EffectCommand::NotifyPeer {
                peer: AuthorityId::new_from_entropy([9; 32]),
                invitation_id: InvitationId::new("invitation-accept-plan"),
            },
        ]);

        let plan = match plan_accept_execution(outcome) {
            Ok(plan) => plan,
            Err(error) => panic!("accept execution plan: {error}"),
        };
        assert_eq!(plan.local_effects.len(), 1);
        assert_eq!(plan.deferred_network_effects.len(), 1);
        assert!(matches!(
            plan.deferred_network_effects[0],
            EffectCommand::NotifyPeer { .. }
        ));
    }

    #[test]
    fn test_plan_send_execution_keeps_only_journal_append_local() {
        let outcome = GuardOutcome::allowed(vec![
            EffectCommand::JournalAppend {
                fact: InvitationFact::Declined {
                    context_id: Some(ContextId::new_from_entropy([2; 32])),
                    invitation_id: InvitationId::new("invitation-send-plan"),
                    decliner_id: AuthorityId::new_from_entropy([4; 32]),
                    declined_at: aura_core::time::PhysicalTime {
                        ts_ms: 1,
                        uncertainty: None,
                    },
                },
            },
            EffectCommand::RecordReceipt {
                operation: InvitationOperation::DeclineInvitation,
                peer: Some(AuthorityId::new_from_entropy([5; 32])),
            },
        ]);

        let plan = match plan_send_execution(outcome) {
            Ok(plan) => plan,
            Err(error) => panic!("send execution plan: {error}"),
        };
        assert_eq!(plan.local_effects.len(), 1);
        assert_eq!(plan.deferred_network_effects.len(), 1);
        assert!(matches!(
            plan.local_effects[0],
            EffectCommand::JournalAppend { .. }
        ));
    }
}
