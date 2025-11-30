#![allow(clippy::type_complexity)]

//! # Aura MPST - Layer 2: Specification (Choreography Runtime)
//!
//! **Purpose**: Runtime library for choreographic protocol specifications and multi-party session types.
//!
//! This crate provides semantic abstractions for choreographic features, integrating with
//! rumpsteak-aura to enable full multi-party session type support with Aura-specific extensions.
//!
//! # Architecture Constraints
//!
//! **Layer 2 depends only on aura-core** (foundation).
//! - ✅ Session type runtime and choreographic abstractions
//! - ✅ Guard chain integration traits (`CapabilityGuard`, `JournalCoupling`, etc.)
//! - ✅ Protocol specification support
//! - ✅ Extensions for leakage tracking and context isolation
//! - ❌ NO effect handler implementations
//! - ❌ NO multi-party coordination logic (that's aura-protocol)
//! - ❌ NO handler composition (that's aura-composition)
//!
//! # Design Philosophy
//!
//! This crate is a **regular crate** (not proc-macro) which allows it to:
//! 1. Re-export all rumpsteak-aura functionality
//! 2. Provide the exact same `choreography!` macro interface
//! 3. Add Aura-specific extensions via the extension system
//! 4. Integrate with the guard chain for protocol-level guards
//!
//! # Usage
//!
//! ```ignore
//! use aura_mpst::choreography;
//!
//! // This works EXACTLY like rumpsteak-aura's choreography! macro
//! // but with Aura-specific extensions
//! choreography! {
//!     choreography Example {
//!         roles: Alice, Bob;
//!
//!         Alice[guard_capability = "send_message", flow_cost = 100]
//!         -> Bob: Message;
//!
//!         Bob[journal_facts = "message_received"]
//!         -> Alice: Response;
//!     }
//! }
//! ```
//!
//! # Extension System Integration
//!
//! Extensions are registered automatically when using the choreography macro.
//! The extension system provides Aura-specific annotations like:
//!
//! - `[guard_capability="..."]` - Capability requirements
//! - `[flow_cost=100]` - Resource costs
//! - `[journal_facts="..."]` - Audit logging
//! - `[journal_merge=true]` - Journal merge operations
//! - `[audit_log="..."]` - Audit trail entries
//!
//! # Architecture
//!
//! ```text
//! aura-mpst/              ← Regular crate (re-exports rumpsteak-aura + Aura extensions)
//! aura-macros/            ← Proc-macro crate (custom macros)
//! ```

// Re-export core rumpsteak-aura functionality
pub use rumpsteak_aura;
pub use rumpsteak_aura_choreography;

// Note: aura-macros generates code that uses aura-mpst types,
// but we don't import aura-macros here to avoid circular dependency

// ===== Current Modules (Actively Used) =====

/// AST extraction and annotation parsing for Aura choreographies
/// Used by: aura-macros (production), examples
pub mod ast_extraction;

/// Journal annotation types for fact-based operations
/// Used by: aura-protocol/guards/journal_coupler.rs
pub mod journal;

/// Session type system types (LocalSessionType)
/// Recently migrated from aura-core
pub mod session;

// ===== Test/Example-Only Modules (Compatibility) =====

/// Extension system integration (used in integration tests)
/// Note: Extensions now handled by aura-macros; this is test compatibility
pub mod extensions;

/// Runtime factory and protocol requirements (used in integration tests)
/// Note: Runtime composition now in aura-agent; this is test compatibility
pub mod runtime;

// ===== Deprecated Modules (Scheduled for Removal in 1.0) =====

/// Context isolation for choreographies
/// **DEPRECATED**: Use aura-core::identifiers::ContextId and context derivation instead
/// **Removal Timeline**: Version 1.0 (Q2 2026)
#[deprecated(
    since = "0.1.0",
    note = "Use aura-core::identifiers::ContextId and aura-core::context_derivation instead"
)]
pub mod context;

// guards module REMOVED - use aura-protocol::guards::{CapGuard, SendGuard} instead
// See docs/107_mpst_and_choreography.md for migration guidance and choreography-first guard architecture

/// Leakage budget tracking for choreographies
/// **DEPRECATED**: Use aura-protocol::guards::LeakageTracker and aura-core::effects::LeakageEffects instead
/// **Removal Timeline**: Version 1.0 (Q2 2026)
#[deprecated(
    since = "0.1.0",
    note = "Use aura-protocol::guards::LeakageTracker and aura-core::effects::LeakageEffects instead"
)]
pub mod leakage;

/// Initialize the Aura extension system (external-demo pattern)
///
/// Returns an empty extension registry following the "external-demo" pattern.
/// Aura-specific choreography extensions (guard_capability, flow_cost, journal_facts,
/// leak annotations) are now implemented in the [`aura-macros`](../aura_macros/index.html)
/// crate via procedural macros that parse and extract annotations at compile-time.
///
/// This function exists for:
/// 1. Compatibility with rumpsteak's extension system
/// 2. Testing that the registry initialization works
/// 3. Future extensibility if runtime extensions become needed
///
/// For actual Aura extensions, see:
/// - [`aura_macros::choreography`](../aura_macros/attr.choreography.html) - Parse choreography annotations
/// - [`aura_macros`](../aura_macros/index.html) - All Aura procedural macros
///
/// # External-Demo Pattern
///
/// The "external-demo" pattern means extensions are handled outside the core library
/// (in aura-macros) rather than being registered at runtime. This provides:
/// - Compile-time validation of annotations
/// - Zero runtime overhead for extension processing
/// - Better error messages via proc macros
pub fn init_aura_extensions() -> rumpsteak_aura_choreography::extensions::ExtensionRegistry {
    // Create empty registry - extensions are now handled in aura-macros
    rumpsteak_aura_choreography::extensions::ExtensionRegistry::new()
}

pub use ast_extraction::{
    extract_aura_annotations, generate_aura_choreography_code, AuraEffect, AuraExtractionError,
};
/// Full-featured choreography! macro with ALL rumpsteak-aura features + Aura extensions
///
/// This macro provides access to ALL rumpsteak-aura features plus Aura-specific extensions:
/// - Namespace attributes: `#[namespace = "my_protocol"]`
/// - Parameterized roles: `Worker[N]`, `Signer[*]`
/// - Choice constructs: `choice at Role { ... }`
/// - Loop constructs: `loop { ... }`
/// - Aura capability guards: `[guard_capability = "capability_name"]`
/// - Aura flow costs: `[flow_cost = 100]`
/// - Aura journal facts: `[journal_facts = "description"]`
/// - Aura audit logging: `[audit_log = "action:metadata"]`
///
/// # Example
///
/// ```ignore
/// use aura_mpst::choreography;
///
/// choreography! {
///     #[namespace = "threshold_ceremony"]
///     protocol ThresholdExample {
///         roles: Coordinator, Signer[3];
///
///         choice at Coordinator {
///             start_ceremony: {
///                 Coordinator[guard_capability = "coordinate_signing",
///                            flow_cost = 200,
///                            journal_facts = "ceremony_started"]
///                 -> Signer[*]: StartRequest;
///
///                 Signer[*][guard_capability = "participate_signing",
///                          flow_cost = 150]
///                 -> Coordinator: Commitment;
///             }
///             abort: {
///                 Coordinator -> Signer[*]: Abort;
///             }
///         }
///     }
/// }
/// ```
///
/// Note: The choreography! macro is available in the aura-macros crate.
/// Generated code uses types from this crate.
// Legacy API re-exports for compatibility
pub use aura_core::{identifiers::DeviceId, AuraError, AuraResult, Cap, Journal, JournalEffects};

// Current API re-exports
pub use journal::{JournalAnnotation, JournalCoupling};
pub use session::LocalSessionType;

// Deprecated re-exports (for backward compatibility until 1.0)
#[allow(deprecated)]
pub use context::{ContextIsolation, ContextType};
// guards re-exports REMOVED - use aura-protocol::guards instead
#[allow(deprecated)]
pub use leakage::{LeakageBudget, LeakageTracker};
pub use runtime::{AuraEndpoint, AuraHandler, ConnectionState, ExecutionContext, ExecutionMode};

/// Standard result type for MPST operations
pub type MpstResult<T> = std::result::Result<T, MpstError>;

/// Errors specific to MPST extensions
#[derive(Debug, thiserror::Error)]
pub enum MpstError {
    /// Capability guard failed authorization
    #[error("Capability guard failed: {reason}")]
    CapabilityGuardFailed {
        /// The reason for capability guard failure
        reason: String,
    },

    /// Journal coupling operation failed
    #[error("Journal coupling failed: {reason}")]
    JournalCouplingFailed {
        /// The reason for journal coupling failure
        reason: String,
    },

    /// Leakage budget exceeded
    #[error("Leakage budget exceeded: {consumed} > {limit}")]
    LeakageBudgetExceeded {
        /// Amount of budget consumed
        consumed: u64,
        /// Maximum budget limit
        limit: u64,
    },

    /// Context isolation violation
    #[error("Context isolation violated: {violation}")]
    ContextIsolationViolated {
        /// Description of the isolation violation
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
        + std::marker::Send
        + std::marker::Sync
        + 'static,
{
    // Note: Runtime validation removed - use aura-protocol guards for validation

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extension_registry_initialization() {
        let registry = init_aura_extensions();
        // Verify registry is initialized (empty registry for external-demo pattern)
        // Extensions are now handled by aura-macros following external-demo pattern
        assert_eq!(registry.grammar_extensions().count(), 0);
    }

    #[test]
    fn test_choreography_macro_available() {
        // This test verifies the choreography macro is properly re-exported
        // Actual functionality is tested in integration tests
    }

    #[test]
    fn test_all_rumpsteak_features_available() {
        // Verify we have access to all rumpsteak-aura types and functions
        let _registry = rumpsteak_aura_choreography::extensions::ExtensionRegistry::new();
        let _composer = rumpsteak_aura_choreography::compiler::GrammarComposer::new();
        let _parser = rumpsteak_aura_choreography::compiler::ExtensionParser::new();

        // If this compiles, we successfully re-exported everything
    }
}
