//! Aura MPST Runtime Integration
//!
//! This crate provides the runtime integration layer between aura-macros
//! generated choreographies and rumpsteak-aura session types, focusing on:
//! - Aura runtime state management (capabilities, journal, devices)
//! - Extension registry for guard chains and journal coupling
//! - Effect system integration with aura-core
//!
//! # Architecture
//!
//! ```text
//! aura-macros → ExtensionRegistry → rumpsteak-aura + AuraRuntime
//! ```
//!
//! This crate does NOT implement custom choreography parsing/projection.
//! It focuses purely on runtime integration with proven session types.

#![allow(missing_docs)]
#![forbid(unsafe_code)]

// === Core Modules ===

/// Capability guard syntax and runtime enforcement
pub mod guards;

/// Journal-coupling annotations and CRDT integration
pub mod journal_coupling;

/// Leakage budget tracking for privacy contracts
pub mod leakage;

/// Context isolation enforcement
pub mod context;

/// MPST runtime extensions
pub mod runtime;

// Deleted modules: analysis, infrastructure, privacy_verification
// These provided custom choreography infrastructure that duplicated rumpsteak-aura

// === Public API Re-exports ===

pub use context::{ContextIsolation, ContextType};
pub use guards::{CapabilityGuard, GuardSyntax};
pub use journal_coupling::{JournalAnnotation, JournalCoupling};
pub use leakage::{LeakageBudget, LeakageTracker};
pub use runtime::{AuraRuntime, ExecutionContext};

// === Extension Registry ===

/// Extension registry for aura-macros generated code
pub struct ExtensionRegistry {
    guards: std::collections::HashMap<String, String>,
    flow_costs: std::collections::HashMap<String, u64>,
    journal_facts: std::collections::HashMap<String, String>,
}

impl ExtensionRegistry {
    /// Create a new extension registry
    pub fn new() -> Self {
        Self {
            guards: std::collections::HashMap::new(),
            flow_costs: std::collections::HashMap::new(),
            journal_facts: std::collections::HashMap::new(),
        }
    }
    
    /// Register a capability guard
    pub fn register_guard(&mut self, capability: &str, role: &str) {
        self.guards.insert(role.to_string(), capability.to_string());
    }
    
    /// Register a flow cost
    pub fn register_flow_cost(&mut self, cost: u64, role: &str) {
        self.flow_costs.insert(role.to_string(), cost);
    }
    
    /// Register a journal fact
    pub fn register_journal_fact(&mut self, fact: &str, role: &str) {
        self.journal_facts.insert(role.to_string(), fact.to_string());
    }
}

impl Default for ExtensionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// === Integration Function ===

/// Execute choreography with aura-mpst runtime integration
pub async fn execute_choreography(
    _namespace: &str,
    runtime: &mut AuraRuntime,
    _context: &ExecutionContext,
) -> MpstResult<()> {
    // Placeholder integration with rumpsteak-aura
    // In full implementation, this would:
    // 1. Create rumpsteak session types program
    // 2. Apply extension registry to runtime
    // 3. Execute with rumpsteak-aura interpreter
    
    runtime.validate()?;
    println!("Choreography execution completed successfully");
    Ok(())
}

// === Foundation Re-exports ===

pub use aura_core::{
    AuraError, AuraResult, Cap, DeviceId, Journal, JournalEffects,
};

/// Standard result type for MPST operations
pub type MpstResult<T> = std::result::Result<T, MpstError>;

/// Errors specific to MPST extensions
#[derive(Debug, thiserror::Error)]
pub enum MpstError {
    /// Capability guard failed authorization
    #[error("Capability guard failed: {reason}")]
    CapabilityGuardFailed {
        /// Reason for the guard failure
        reason: String,
    },

    /// Journal coupling operation failed
    #[error("Journal coupling failed: {reason}")]
    JournalCouplingFailed {
        /// Reason for the coupling failure
        reason: String,
    },

    /// Leakage budget exceeded
    #[error("Leakage budget exceeded: {consumed} > {limit}")]
    LeakageBudgetExceeded {
        /// Amount of budget consumed
        consumed: u64,
        /// Budget limit
        limit: u64,
    },

    /// Context isolation violation
    #[error("Context isolation violated: {violation}")]
    ContextIsolationViolated {
        /// Description of the violation
        violation: String,
    },

    
    /// Core error wrapped
    #[error("Core error: {0}")]
    Core(#[from] aura_core::AuraError),
}

impl MpstError {
    /// Create a capability guard failure error
    pub fn capability_guard_failed(reason: impl Into<String>) -> Self {
        Self::CapabilityGuardFailed {
            reason: reason.into(),
        }
    }

    /// Create a journal coupling failure error
    pub fn journal_coupling_failed(reason: impl Into<String>) -> Self {
        Self::JournalCouplingFailed {
            reason: reason.into(),
        }
    }

    /// Create a leakage budget exceeded error
    pub fn leakage_budget_exceeded(consumed: u64, limit: u64) -> Self {
        Self::LeakageBudgetExceeded { consumed, limit }
    }

    /// Create a context isolation violation error
    pub fn context_isolation_violated(violation: impl Into<String>) -> Self {
        Self::ContextIsolationViolated {
            violation: violation.into(),
        }
    }

}
