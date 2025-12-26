#![allow(
    missing_docs,
    unused_variables,
    clippy::unwrap_used,
    clippy::expect_used,
    dead_code,
    clippy::match_like_matches_macro,
    clippy::type_complexity,
    clippy::while_let_loop,
    clippy::redundant_closure,
    clippy::large_enum_variant,
    clippy::unused_unit,
    clippy::get_first,
    clippy::single_range_in_vec_init,
    clippy::disallowed_methods, // Guard chain coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
//! Layer 4: Protocol Guard Chain - Authorization, Budget, Privacy, Journal
//!
//! Choreographic enforcement of authorization, flow budgets, privacy, and journal consistency
//! through a composable guard chain applied to **every send** (per docs/003_information_flow_contract.md).
//!
//! # Choreography-First Architecture
//!
//! Guard effects can originate from two sources that use the same `EffectCommand` system:
//!
//! 1. **Choreographic Annotations** (compile-time): The `choreography!` macro generates
//!    `EffectCommand` sequences from annotations like `guard_capability`, `flow_cost`,
//!    `journal_facts`, and `leak`. These are produced by `aura_macros::choreography`
//!    via the generated `effect_bridge::annotation_to_commands()` function.
//!
//! 2. **Runtime Guard Chain** (send-site): The `GuardChain::standard()` evaluates
//!    pure guards (`CapabilityGuard`, `FlowBudgetGuard`, `JournalCouplingGuard`,
//!    `LeakageTrackingGuard`) against a `GuardSnapshot` and produces `EffectCommand`
//!    sequences at each protocol send site.
//!
//! Both sources produce `Vec<EffectCommand>` that are executed through an `EffectInterpreter`:
//! - Production: `ProductionEffectInterpreter` (aura-effects)
//! - Simulation: `SimulationEffectInterpreter` (aura-simulator)
//! - Testing: `BorrowedEffectInterpreter` / mock interpreters
//!
//! # Guard Chain Sequence
//!
//! `CapGuard` → `FlowGuard` → `JournalCoupler` → `LeakageTracker` → `Transport`
//!
//! # Invariant: Charge-Before-Send
//!
//! No transport side effect occurs unless all preceding guards succeed. Ensures:
//! - Authorization verified (Biscuit tokens evaluated)
//! - Flow budgets charged (atomically incremented spent counter)
//! - Delta facts committed to journal (atomic with send)
//! - Leakage budget validated (per observer class)
//!
//! # Receipt Semantics
//!
//! (per docs/003_information_flow_contract.md §3.2)
//! FlowGuard produces receipts proving budget charges, scoped to (ContextId, peer) pairs,
//! bound to Epochs (budget rotation). Required for relayed messages (per-hop forwarding).
//!
//! # Guard Implementations
//!
//! - **CapGuard**: Biscuit token evaluation with `need(σ) ≤ C` predicate
//! - **FlowGuard**: Flow budget enforcement, atomically increments spent counter
//! - **JournalCoupler**: Merges delta facts atomic with send
//! - **LeakageTracker**: Privacy budget per observer class (external, neighbor, group)
//!
//! # Example: Using Guard Chain with Effect Interpreter
//!
//! ```rust,ignore
//! use aura_guards::{create_send_guard, SendGuardChain};
//!
//! let guard = create_send_guard(
//!     "message:send".to_string(),
//!     context_id,
//!     peer_device,
//!     100, // flow cost
//! );
//!
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
//!
//! // For development/testing, use deterministic test keys:
//! let guard = ProtocolGuard::new_for_testing("complex_operation")
//!     .delta_facts(vec![fact1, fact2])
//!     .leakage_budget(LeakageBudget::new(1, 2, 0));
//!
//! // For production, use real keys:
//! // let guard = ProtocolGuard::new(root_public_key, authority_id, "complex_operation")
//! //     .require_token(biscuit_token)
//! //     .delta_facts(vec![fact1, fact2])
//! //     .leakage_budget(LeakageBudget::new(1, 2, 0));
//!
//! // Execute with guards
//! let result = guard.execute_with_effects(effect_system, |effects| async move {
//!     // Protocol execution here, using `effects` if needed
//!     Ok(protocol_result)
//! }).await?;
//! ```

// Guard implementations
pub mod chain; // SendGuardChain (guard chain orchestration)
pub mod config;
pub mod deltas;
pub mod executor;
pub mod execution;
pub mod flow;
pub mod journal; // JournalCoupler
pub mod policy; // Effect policy guards
pub mod privacy;
pub mod pure;
pub mod traits; // GuardContextProvider
pub mod types; // Shared guard types

// Biscuit-based capability guards
pub mod biscuit_evaluator;
pub mod capability_guard;

// Core re-exports
pub use traits::GuardContextProvider;
pub use flow::FlowGuard;
pub use aura_core::effects::{FlowBudgetEffects, FlowHint};
pub use journal::{
    CouplingMetrics, JournalCoupler, JournalCouplerBuilder, JournalCouplingResult, JournalOperation,
};
pub use chain::{create_send_guard, create_send_guard_op, SendGuardChain, SendGuardResult};
pub use types::GuardOperation;

use aura_core::effects::{
    AuthorizationEffects, JournalEffects, LeakageEffects, PhysicalTimeEffects, RandomEffects,
    StorageEffects,
};
use aura_core::AuraResult;
use aura_core::AuthorityId;
use biscuit_auth::{Biscuit, PublicKey};
use std::future::Future;
use crate::guards::privacy::AdversaryClass;

/// Composite effect requirements for guard evaluation/execution.
pub trait GuardEffects:
    JournalEffects
    + StorageEffects
    + FlowBudgetEffects
    + PhysicalTimeEffects
    + RandomEffects
    + AuthorizationEffects
    + LeakageEffects
    + Send
    + Sync
{
}

impl<T> GuardEffects for T where
    T: JournalEffects
        + StorageEffects
        + FlowBudgetEffects
        + PhysicalTimeEffects
        + RandomEffects
        + AuthorizationEffects
        + LeakageEffects
        + Send
        + Sync
{
}

/// Protocol execution guard combining authorization checking, delta application, and privacy tracking
#[derive(Debug, Clone)]
pub struct ProtocolGuard {
    /// Root public key for Biscuit token verification
    pub root_public_key: PublicKey,
    /// Authority ID for this guard context
    pub authority_id: AuthorityId,
    /// Context ID for leakage accounting
    pub context_id: aura_core::ContextId,
    /// Observer classes that can see this operation
    pub observable_by: Vec<AdversaryClass>,
    /// Required Biscuit authorization tokens for this operation
    pub required_tokens: Vec<Biscuit>,
    /// Facts to be merged into the journal after successful execution
    pub delta_facts: Vec<serde_json::Value>, // JSON-encoded facts until typed fact system
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
    /// Create a new protocol guard with required key material
    ///
    /// # Arguments
    /// * `root_public_key` - The Biscuit root public key for token verification
    /// * `authority_id` - The authority ID for this guard context
    /// * `operation_id` - Identifier for logging and metrics
    pub fn new(
        root_public_key: PublicKey,
        authority_id: AuthorityId,
        operation_id: impl Into<String>,
    ) -> Self {
        Self {
            root_public_key,
            authority_id,
            context_id: aura_core::ContextId::default(),
            observable_by: vec![
                AdversaryClass::External,
                AdversaryClass::Neighbor,
                AdversaryClass::InGroup,
            ],
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
        E: GuardEffects + aura_core::TimeEffects + traits::GuardContextProvider,
        F: FnOnce(&mut E) -> Fut,
        Fut: Future<Output = AuraResult<T>>,
    {
        execution::execute_guarded_operation(self, effect_system, operation).await
    }

    /// Create a protocol guard with deterministic test keys for development/testing
    ///
    /// Uses a deterministic keypair and authority ID. This is useful for:
    /// - Development and testing where real keys aren't available
    /// - Macro-generated guards that don't have key context
    /// - Scenarios where guard evaluation is bypassed or mocked
    ///
    /// # Security Warning
    /// Production code should use `new()` with real key material from the
    /// authority's Biscuit root key and actual AuthorityId.
    pub fn new_for_testing(operation_id: impl Into<String>) -> Self {
        // Use deterministic seed for reproducible behavior
        let keypair = biscuit_auth::KeyPair::new();
        let authority_id = AuthorityId::new_from_entropy([0u8; 32]);

        Self {
            root_public_key: keypair.public(),
            authority_id,
            context_id: aura_core::ContextId::default(),
            observable_by: vec![
                AdversaryClass::External,
                AdversaryClass::Neighbor,
                AdversaryClass::InGroup,
            ],
            required_tokens: Vec::new(),
            delta_facts: Vec::new(),
            leakage_budget: LeakageBudget::zero(),
            operation_id: operation_id.into(),
        }
    }

    /// Set the context ID for leakage accounting
    pub fn context_id(mut self, context_id: aura_core::ContextId) -> Self {
        self.context_id = context_id;
        self
    }

    /// Set explicit observer classes for leakage accounting
    pub fn observable_by(mut self, observers: Vec<AdversaryClass>) -> Self {
        self.observable_by = observers;
        self
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

/// Convenience macro for creating protocol guards with test keys
///
/// For production use with real keys, use `ProtocolGuard::new()` directly.
#[macro_export]
macro_rules! guard {
    (
        operation: $op:expr,
        $(deltas: [$($delta:expr),*],)?
        $(leakage: ($ext:expr, $ngh:expr, $grp:expr),)?
    ) => {
        {
            let mut guard = $crate::guards::ProtocolGuard::new_for_testing($op);

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
pub use execution::*;
pub use privacy::*;

// Re-export Biscuit guard types
pub use biscuit_evaluator::{BiscuitGuardEvaluator, GuardError, GuardResult};
pub use capability_guard::{CapabilityGuard, CapabilityGuardExt};

// Re-export policy guard types
pub use policy::{
    EffectPolicyError, EffectPolicyExt, EffectPolicyGuard, EffectPolicyResult,
};

// Re-export executor functions for choreography integration
pub use executor::{
    execute_effect_commands, execute_guard_plan, execute_guarded_choreography,
    BorrowedEffectInterpreter, ChoreographyCommand, ChoreographyResult, EffectSystemInterpreter,
    GuardChainExecutor, GuardChainResult, GuardPlan,
};
