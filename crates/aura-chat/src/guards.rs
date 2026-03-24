//! Chat Guard Types
//!
//! Guard chain integration for fact-first chat operations.
//! Guard evaluation is pure and synchronous over a prepared `GuardSnapshot`,
//! producing an explicit list of `EffectCommand` values for an async interpreter.

use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::FlowCost;
pub use aura_guards::types;

use crate::facts::ChatFact;

// =============================================================================
// Guard Cost Constants
// =============================================================================

/// Guard cost and capability constants for chat operations.
pub mod costs {
    use aura_core::FlowCost;

    /// Flow cost for creating a channel.
    pub const CHAT_CHANNEL_CREATE_COST: FlowCost = FlowCost::new(1);

    /// Flow cost for sending a message.
    pub const CHAT_MESSAGE_SEND_COST: FlowCost = FlowCost::new(1);

}

// =============================================================================
// Guard Snapshot
// =============================================================================

/// Snapshot of guard-relevant state for evaluation.
#[derive(Debug, Clone)]
pub struct GuardSnapshot {
    /// Authority performing the operation.
    pub authority_id: AuthorityId,

    /// Context for the operation.
    pub context_id: ContextId,

    /// Current flow budget remaining.
    pub flow_budget_remaining: FlowCost,

    /// Capabilities held by the authority.
    pub capabilities: Vec<types::CapabilityId>,

    /// Current timestamp in milliseconds.
    pub now_ms: u64,

    /// Sender is currently banned in this context/channel.
    pub sender_is_banned: bool,

    /// Sender is currently muted in this context/channel.
    pub sender_is_muted: bool,
}

impl GuardSnapshot {
    /// Construct a guard snapshot for chat guard evaluation.
    pub fn new(
        authority_id: AuthorityId,
        context_id: ContextId,
        flow_budget_remaining: FlowCost,
        capabilities: Vec<types::CapabilityId>,
        now_ms: u64,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            flow_budget_remaining,
            capabilities,
            now_ms,
            sender_is_banned: false,
            sender_is_muted: false,
        }
    }

    /// Attach authoritative moderation status to the prepared guard snapshot.
    #[must_use]
    pub fn with_moderation_status(mut self, sender_is_banned: bool, sender_is_muted: bool) -> Self {
        self.sender_is_banned = sender_is_banned;
        self.sender_is_muted = sender_is_muted;
        self
    }

    /// Returns `true` if the snapshot contains the given capability.
    pub fn has_capability(&self, cap: &types::CapabilityId) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }
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

/// Decision type shared across Layer 5 feature crates.
pub type GuardDecision = types::GuardDecision;

// =============================================================================
// Effect Command
// =============================================================================

/// Effect command to be executed after guard approval.
#[derive(Debug, Clone)]
pub enum EffectCommand {
    /// Append a chat fact to the journal.
    JournalAppend {
        /// The chat fact to append.
        fact: ChatFact,
    },

    /// Charge flow budget.
    ChargeFlowBudget {
        /// Cost to charge from the current context budget.
        cost: FlowCost,
    },
}

/// Outcome type shared across Layer 5 feature crates.
pub type GuardOutcome = types::GuardOutcome<EffectCommand>;

// =============================================================================
// Guard Helpers
// =============================================================================

/// Check capability and return a denied outcome if missing.
pub fn check_capability(
    snapshot: &GuardSnapshot,
    required_cap: &types::CapabilityId,
) -> Option<GuardOutcome> {
    types::check_capability(snapshot, required_cap)
}

/// Check flow budget and return a denied outcome if insufficient.
pub fn check_flow_budget(
    snapshot: &GuardSnapshot,
    required_cost: FlowCost,
) -> Option<GuardOutcome> {
    types::check_flow_budget(snapshot, required_cost)
}

/// Deny chat send/join effects when authoritative moderation status blocks the
/// sender in this context/channel.
pub fn check_moderation(snapshot: &GuardSnapshot) -> Option<GuardOutcome> {
    if snapshot.sender_is_banned {
        return Some(GuardOutcome::denied(types::GuardViolation::other(
            "authoritative moderation denied chat operation: sender is banned",
        )));
    }
    if snapshot.sender_is_muted {
        return Some(GuardOutcome::denied(types::GuardViolation::other(
            "authoritative moderation denied chat operation: sender is muted",
        )));
    }
    None
}
