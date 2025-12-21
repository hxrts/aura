//! Chat Guard Types
//!
//! Guard chain integration for fact-first chat operations.
//! Guard evaluation is pure and synchronous over a prepared `GuardSnapshot`,
//! producing an explicit list of `EffectCommand` values for an async interpreter.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_protocol::guards::feature;

use crate::facts::ChatFact;

// =============================================================================
// Guard Cost Constants
// =============================================================================

/// Guard cost and capability constants for chat operations.
pub mod costs {
    /// Flow cost for creating a channel.
    pub const CHAT_CHANNEL_CREATE_COST: u32 = 1;

    /// Flow cost for sending a message.
    pub const CHAT_MESSAGE_SEND_COST: u32 = 1;

    /// Required capability for creating a channel.
    pub const CAP_CHAT_CHANNEL_CREATE: &str = "chat:channel:create";

    /// Required capability for sending a message.
    pub const CAP_CHAT_MESSAGE_SEND: &str = "chat:message:send";
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
    pub flow_budget_remaining: u32,

    /// Capabilities held by the authority.
    pub capabilities: Vec<String>,

    /// Current timestamp in milliseconds.
    pub now_ms: u64,
}

impl GuardSnapshot {
    /// Construct a guard snapshot for chat guard evaluation.
    pub fn new(
        authority_id: AuthorityId,
        context_id: ContextId,
        flow_budget_remaining: u32,
        capabilities: Vec<String>,
        now_ms: u64,
    ) -> Self {
        Self {
            authority_id,
            context_id,
            flow_budget_remaining,
            capabilities,
            now_ms,
        }
    }

    /// Returns `true` if the snapshot contains the given capability string.
    pub fn has_capability(&self, cap: &str) -> bool {
        self.capabilities.iter().any(|c| c == cap)
    }
}

impl feature::CapabilitySnapshot for GuardSnapshot {
    fn has_capability(&self, cap: &str) -> bool {
        GuardSnapshot::has_capability(self, cap)
    }
}

impl feature::FlowBudgetSnapshot for GuardSnapshot {
    fn flow_budget_remaining(&self) -> u32 {
        self.flow_budget_remaining
    }
}

/// Decision type shared across Layer 5 feature crates.
pub type GuardDecision = feature::GuardDecision;

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
        cost: u32,
    },
}

/// Outcome type shared across Layer 5 feature crates.
pub type GuardOutcome = feature::GuardOutcome<EffectCommand>;

// =============================================================================
// Guard Helpers
// =============================================================================

/// Check capability and return a denied outcome if missing.
pub fn check_capability(snapshot: &GuardSnapshot, required_cap: &str) -> Option<GuardOutcome> {
    feature::check_capability(snapshot, required_cap)
}

/// Check flow budget and return a denied outcome if insufficient.
pub fn check_flow_budget(snapshot: &GuardSnapshot, required_cost: u32) -> Option<GuardOutcome> {
    feature::check_flow_budget(snapshot, required_cost)
}
