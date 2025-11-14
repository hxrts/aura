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
pub mod journal;

/// Leakage budget tracking for privacy contracts
pub mod leakage;

/// Context isolation enforcement
pub mod context;

/// MPST runtime extensions
pub mod runtime;

/// Extension effects for Aura-specific annotations
pub mod extensions;

// Deleted modules: analysis, infrastructure, privacy_verification
// These provided custom choreography infrastructure that duplicated rumpsteak-aura

// === Public API Re-exports ===

pub use context::{ContextIsolation, ContextType};
pub use guards::{CapabilityGuard, GuardSyntax};
pub use journal::{JournalAnnotation, JournalCoupling};
pub use leakage::{LeakageBudget, LeakageTracker};
pub use runtime::{
    AuraEndpoint, AuraHandler, AuraRuntime, ConnectionState, ExecutionContext, ExecutionMode,
};

// === Extension Registry ===

/// Re-export rumpsteak-aura ExtensionRegistry for public API
pub use rumpsteak_aura_choreography::effects::ExtensionRegistry;

// === Integration Function ===

/// Execute choreography with aura-mpst runtime integration using rumpsteak-aura
pub async fn execute_choreography<M>(
    handler: &mut AuraHandler,
    endpoint: &mut AuraEndpoint,
    program: rumpsteak_aura_choreography::effects::Program<DeviceId, M>,
) -> MpstResult<()>
where
    M: rumpsteak_aura_choreography::effects::ProgramMessage
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>
        + Send
        + Sync
        + 'static,
{
    // Validate runtime state before execution
    handler.runtime().validate()?;

    // Execute the program through rumpsteak-aura interpreter
    rumpsteak_aura_choreography::effects::interpret_extensible(handler, endpoint, program)
        .await
        .map_err(|e| {
            MpstError::Core(aura_core::AuraError::invalid(format!(
                "Choreography execution failed: {}",
                e
            )))
        })?;

    Ok(())
}

// === Foundation Re-exports ===

pub use aura_core::{AuraError, AuraResult, Cap, DeviceId, Journal, JournalEffects};

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
