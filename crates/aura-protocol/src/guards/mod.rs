//! Layer 4: Protocol Guard Chain - Authorization, Budget, Privacy, Journal
//!
//! Choreographic enforcement of authorization, flow budgets, privacy, and journal consistency
//! through a composable guard chain applied to **every send** (per docs/003_information_flow_contract.md).
//!
//! **Guard Chain Sequence**:
//! `CapGuard` → `FlowGuard` → `JournalCoupler` → `LeakageTracker` → `Transport`
//!
//! **Invariant: Charge-Before-Send**:
//! No transport side effect occurs unless all preceding guards succeed. Ensures:
//! - Authorization verified (Biscuit tokens evaluated)
//! - Flow budgets charged (atomically incremented spent counter)
//! - Delta facts committed to journal (atomic with send)
//! - Leakage budget validated (per observer class)
//!
//! **Receipt Semantics** (per docs/003_information_flow_contract.md §3.2):
//! FlowGuard produces receipts proving budget charges, scoped to (ContextId, peer) pairs,
//! bound to Epochs (budget rotation). Required for relayed messages (per-hop forwarding).
//!
//! **Guard Implementations**:
//! - **CapGuard**: Biscuit token evaluation with `need(σ) ≤ C` predicate
//! - **FlowGuard**: Flow budget enforcement, atomically increments spent counter
//! - **JournalCoupler**: Merges delta facts atomic with send
//! - **LeakageTracker**: Privacy budget per observer class (external, neighbor, group)
//!
//! **Receipt Semantics**: FlowGuard produces receipts proving budget charges,
//! scoped to ContextId/peer pairs, bound to Epochs (budget rotation). Required
//! for relayed messages (per-hop forwarding).
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
// pub mod evaluation; // Legacy capability evaluation removed - use BiscuitAuthorizationBridge instead
pub mod execution;
pub mod flow;
pub mod journal_coupler;
pub mod privacy;
pub mod send_guard;

// Biscuit-based guards (new implementation)
pub mod biscuit_evaluator;
pub mod capability_guard; // Authority-based capability guards

pub use effect_system_trait::GuardEffectSystem;
// Legacy guard evaluation removed - use BiscuitAuthorizationBridge instead
pub use flow::FlowGuard;
// FlowBudgetEffects and FlowHint moved to aura-core
pub use aura_core::effects::{FlowBudgetEffects, FlowHint};
pub use journal_coupler::{
    CouplingMetrics, JournalCoupler, JournalCouplerBuilder, JournalCouplingResult, JournalOperation,
};
pub use send_guard::{create_send_guard, SendGuardChain, SendGuardResult};

// use crate::wot::EffectSystemInterface; // Legacy interface removed - use Biscuit authorization instead
use aura_core::AuraResult;
// use aura_wot::Capability; // Legacy capability removed - use Biscuit tokens instead
use biscuit_auth::Biscuit;
use std::future::Future;

/// Protocol execution guard combining authorization checking, delta application, and privacy tracking
#[derive(Debug, Clone)]
pub struct ProtocolGuard {
    /// Required Biscuit authorization tokens for this operation
    pub required_tokens: Vec<Biscuit>,
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
    /// Number of authorization checks performed
    pub authorization_checks: usize,
    /// Number of facts applied
    pub facts_applied: usize,
}

impl ProtocolGuard {
    /// Create a new protocol guard with no requirements
    pub fn new(operation_id: impl Into<String>) -> Self {
        Self {
            required_tokens: Vec::new(),
            delta_facts: Vec::new(),
            leakage_budget: LeakageBudget::zero(),
            operation_id: operation_id.into(),
        }
    }

    /// Add a required authorization token to this guard
    pub fn require_token(mut self, token: Biscuit) -> Self {
        self.required_tokens.push(token);
        self
    }

    /// Add multiple required authorization tokens to this guard
    pub fn require_tokens(mut self, tokens: Vec<Biscuit>) -> Self {
        self.required_tokens.extend(tokens);
        self
    }

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
        E: GuardEffectSystem,
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
pub use deltas::*;
pub use effect_system_bridge::*;
pub use execution::*;
pub use privacy::*;

// Re-export Biscuit guard types
pub use biscuit_evaluator::{BiscuitGuardEvaluator, GuardError, GuardResult};
pub use capability_guard::{CapabilityGuard, CapabilityGuardExt};
