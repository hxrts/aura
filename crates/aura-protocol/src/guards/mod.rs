//! Protocol guards for capability-based authorization
//!
//! This module implements the guard infrastructure for choreographic protocols,
//! providing capability-based preconditions, delta fact application, and privacy
//! budget tracking as described in Phase 2.3 of the refactor plan.
//!
//! ## Architecture
//!
//! Guards implement the formal model's operational semantics:
//! - **Guard evaluation**: `need(σ) ≤ C` checking before protocol operations
//! - **Delta application**: `merge_facts(Δfacts)` atomic with message send
//! - **Leakage tracking**: Privacy budget enforcement with observer models
//! - **Send guard chain**: Complete `need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)` predicate enforcement
//!
//! ## Usage
//!
//! ### Send Guard Chain (Primary Interface)
//! ```rust,ignore
//! use crate::guards::{create_send_guard, SendGuardChain};
//! use aura_wot::Capability;
//!
//! // Create send guard with complete predicate enforcement
//! let send_guard = create_send_guard(
//!     Capability::send_message(),
//!     context_id,
//!     peer_device,
//!     100 // flow cost
//! ).with_operation_id("ping_send");
//!
//! // Evaluate complete guard chain: need(m) ≤ Caps(ctx) ∧ headroom(ctx, cost)
//! let result = send_guard.evaluate(&effect_system).await?;
//! if result.authorized {
//!     // Proceed with send using receipt for anti-replay protection
//!     transport.send_with_receipt(message, result.receipt.unwrap()).await?;
//! }
//! ```
//!
//! ### Advanced Protocol Guards
//! ```rust,ignore
//! use crate::guards::{ProtocolGuard, GuardedExecution};
//! use aura_wot::capability::Capability;
//!
//! // Define guard requirements for complex protocol steps
//! let guard = ProtocolGuard::new("complex_operation")
//!     .require_capability(Capability::send_message())
//!     .delta_facts(vec![fact1, fact2])
//!     .leakage_budget(LeakageBudget::new(1, 2, 0));
//!
//! // Execute with guards
//! let result = guard.execute_with_effects(effect_system, |effects| async move {
//!     // Protocol execution here, using `effects` if needed
//!     Ok(protocol_result)
//! }).await?;
//! ```

// pub mod capability; // Removed - replaced by biscuit_evaluator
pub mod deltas;
pub mod effect_system_bridge;
pub mod effect_system_trait;
// pub mod evaluation; // Disabled - needs Capability type rewrite
pub mod execution;
pub mod flow;
pub mod journal_coupler;
pub mod privacy;
// pub mod send_guard; // Disabled - needs Capability type rewrite

// Biscuit-based guards (new implementation)
pub mod biscuit_evaluator;
pub mod capability_guard; // Authority-based capability guards

pub use effect_system_trait::GuardEffectSystem;
pub use flow::{FlowBudgetEffects, FlowGuard, FlowHint};
pub use journal_coupler::{
    CouplingMetrics, JournalCoupler, JournalCouplerBuilder, JournalCouplingResult, JournalOperation,
};
// pub use send_guard::{create_send_guard, SendGuardChain, SendGuardResult}; // Disabled

use crate::wot::EffectSystemInterface;
use aura_core::AuraResult;
// use aura_wot::Capability; // Removed
use std::future::Future;

/// Protocol execution guard combining capability checking, delta application, and privacy tracking
#[derive(Debug, Clone)]
pub struct ProtocolGuard {
    /// Required capabilities for this operation (temporarily disabled)
    // pub required_capabilities: Vec<Capability>,
    /// Facts to be merged into the journal after successful execution
    pub delta_facts: Vec<serde_json::Value>, // Placeholder for actual fact types
    /// Privacy leakage budget for this operation
    pub leakage_budget: LeakageBudget,
    /// Operation identifier for logging and metrics
    pub operation_id: String,
}

/// Privacy leakage budget tracking across adversary classes
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LeakageBudget {
    /// External adversary leakage (bits)
    pub external: u32,
    /// Neighbor adversary leakage (bits)
    pub neighbor: u32,
    /// In-group adversary leakage (bits)
    pub in_group: u32,
}

/// Result of guarded protocol execution
#[derive(Debug)]
pub struct GuardedExecutionResult<T> {
    /// The protocol execution result
    pub result: T,
    /// Whether all guards passed
    pub guards_passed: bool,
    /// Applied delta facts
    pub applied_deltas: Vec<serde_json::Value>,
    /// Consumed leakage budget
    pub consumed_budget: LeakageBudget,
    /// Execution metrics
    pub metrics: ExecutionMetrics,
}

/// Metrics for protocol execution
#[derive(Debug, Default)]
pub struct ExecutionMetrics {
    /// Guard evaluation time (microseconds)
    pub guard_eval_time_us: u64,
    /// Delta application time (microseconds)
    pub delta_apply_time_us: u64,
    /// Total execution time (microseconds)
    pub total_execution_time_us: u64,
    /// Number of capabilities checked
    pub capabilities_checked: usize,
    /// Number of facts applied
    pub facts_applied: usize,
}

impl ProtocolGuard {
    /// Create a new protocol guard with no requirements
    pub fn new(operation_id: impl Into<String>) -> Self {
        Self {
            // required_capabilities: Vec::new(),
            delta_facts: Vec::new(),
            leakage_budget: LeakageBudget::zero(),
            operation_id: operation_id.into(),
        }
    }

    /// Add a required capability to this guard (temporarily disabled)
    // pub fn require_capability(mut self, cap: Capability) -> Self {
    //     self.required_capabilities.push(cap);
    //     self
    // }

    /// Add multiple required capabilities to this guard (temporarily disabled)
    // pub fn require_capabilities(mut self, caps: Vec<Capability>) -> Self {
    //     self.required_capabilities.extend(caps);
    //     self
    // }

    /// Add delta facts to be applied after successful execution
    pub fn delta_facts(mut self, facts: Vec<serde_json::Value>) -> Self {
        self.delta_facts = facts;
        self
    }

    /// Set the leakage budget for this operation
    pub fn leakage_budget(mut self, budget: LeakageBudget) -> Self {
        self.leakage_budget = budget;
        self
    }

    /// Execute a protocol operation with full guard enforcement
    pub async fn execute_with_effects<E, T, F, Fut>(
        &self,
        effect_system: &mut E,
        operation: F,
    ) -> AuraResult<GuardedExecutionResult<T>>
    where
        E: GuardEffectSystem + EffectSystemInterface,
        F: FnOnce(&mut E) -> Fut,
        Fut: Future<Output = AuraResult<T>>,
    {
        execution::execute_guarded_operation(self, effect_system, operation).await
    }
}

impl LeakageBudget {
    /// Create a new leakage budget
    pub fn new(external: u32, neighbor: u32, in_group: u32) -> Self {
        Self {
            external,
            neighbor,
            in_group,
        }
    }

    /// Create a zero leakage budget (no privacy cost)
    pub fn zero() -> Self {
        Self::new(0, 0, 0)
    }

    /// Check if this budget is within the allowed limits
    pub fn is_within_limits(&self, limits: &LeakageBudget) -> bool {
        self.external <= limits.external
            && self.neighbor <= limits.neighbor
            && self.in_group <= limits.in_group
    }

    /// Add two budgets together
    pub fn add(&self, other: &LeakageBudget) -> Self {
        Self {
            external: self.external + other.external,
            neighbor: self.neighbor + other.neighbor,
            in_group: self.in_group + other.in_group,
        }
    }
}

/// Convenience macro for creating protocol guards (temporarily simplified)
#[macro_export]
macro_rules! guard {
    (
        operation: $op:expr,
        $(deltas: [$($delta:expr),*],)?
        $(leakage: ($ext:expr, $ngh:expr, $grp:expr),)?
    ) => {
        {
            let mut guard = $crate::guards::ProtocolGuard::new($op);

            $(
                guard = guard.delta_facts(vec![$($delta),*]);
            )?

            $(
                guard = guard.leakage_budget($crate::guards::LeakageBudget::new($ext, $ngh, $grp));
            )?

            guard
        }
    };
}

// Re-export submodules
// pub use capability::*; // Removed - replaced by biscuit_evaluator
pub use deltas::*;
pub use effect_system_bridge::*;
// pub use evaluation::*; // Disabled - needs Capability type rewrite
pub use execution::*;
// pub use middleware::*; // REMOVED: Uses deprecated JournalEffects methods
pub use privacy::*;
// pub use send_guard::*; // Disabled - needs Capability type rewrite

// Re-export Biscuit guard types
pub use biscuit_evaluator::{BiscuitGuardEvaluator, GuardError, GuardResult};
pub use capability_guard::{CapabilityGuard, CapabilityGuardExt};
