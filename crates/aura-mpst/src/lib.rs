//! Aura MPST Extensions
//!
//! This crate extends the rumpsteak-aura session types framework with Aura-specific features:
//! - Capability guard syntax for authorization checks
//! - Journal-coupling annotations for CRDT operations
//! - Leakage budget tracking for privacy contracts
//! - Context isolation enforcement
//!
//! # Architecture
//!
//! This crate provides three main extensions to choreographic programming:
//!
//! ## 1. Capability Guards
//!
//! Enable authorization checks in choreographies:
//! ```ignore
//! Alice[guard: need(m) ≤ caps] -> Bob: RequestMessage;
//! ```
//!
//! ## 2. Journal Coupling
//!
//! Annotate operations that affect the Journal CRDT:
//! ```ignore
//! Alice[▷ Δfacts] -> Bob: JournalUpdate;
//! ```
//!
//! ## 3. Leakage Tracking
//!
//! Track privacy budget consumption:
//! ```ignore
//! Alice[leak: metadata] -> Relay: ForwardMessage;
//! ```

#![warn(missing_docs)]
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

/// Protocol analysis and verification
pub mod analysis;

/// Core protocol infrastructure
pub mod infrastructure;

/// Privacy contract verification system
pub mod privacy_verification;

// === Public API Re-exports ===

pub use context::{ContextBarrier, ContextIsolation, ContextType, InformationFlow};
pub use guards::{CapabilityGuard, GuardSyntax, GuardedProtocol};
pub use journal_coupling::{DeltaAnnotation, JournalAnnotation, JournalCoupling};
pub use leakage::{LeakageBudget, LeakageTracker, LeakageType, PrivacyContract};
pub use runtime::{AuraRuntime, ExecutionContext, ProtocolRequirements};

// === Foundation Re-exports ===

pub use aura_core::{
    AccountId, AuraError, AuraResult, Cap, CryptoEffects, DeviceId, Fact, Journal, JournalEffects,
    SessionId, TransportEffects,
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

    /// Protocol analysis error
    #[error("Protocol analysis error: {reason}")]
    ProtocolAnalysisError {
        /// Reason for the analysis failure
        reason: String,
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

    /// Create a protocol analysis error
    pub fn protocol_analysis_error(reason: impl Into<String>) -> Self {
        Self::ProtocolAnalysisError {
            reason: reason.into(),
        }
    }
}
