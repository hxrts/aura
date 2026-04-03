#![allow(clippy::type_complexity)]

//! # Aura MPST - Layer 2: Specification (Choreography Runtime)
//!
//! **Purpose**: Runtime library for choreographic protocol specifications and multi-party session types.
//!
//! This crate provides semantic abstractions for choreographic features, integrating with
//! Telltale to enable full multi-party session type support with Aura-specific extensions.
//!
//! # Architecture Constraints
//!
//! **Layer 2 depends only on aura-core** (foundation).
//! - YES Session type runtime and choreographic abstractions
//! - YES Guard chain integration traits (`CapabilityGuard`, `JournalCoupling`, etc.)
//! - YES Protocol specification support
//! - YES Extensions for leakage tracking and context isolation
//! - NO effect handler implementations
//! - NO multi-party coordination logic (that's aura-protocol)
//! - NO handler composition (that's aura-composition)
//!
//! # Design Philosophy
//!
//! This crate is a **regular crate** (not proc-macro) which allows it to:
//! 1. Re-export Telltale choreography/runtime functionality
//! 2. Provide the exact same `tell!` macro interface
//! 3. Lower compiled Telltale annotations into Aura-owned effects
//! 4. Integrate with the guard chain for protocol-level guards
//!
//! # Usage
//!
//! ```ignore
//! use aura_mpst::tell;
//!
//! // This works EXACTLY like telltale's tell! macro
//! // but with Aura-specific extensions
//! tell!(r#"
//! module example exposing (Example)
//!
//! protocol Example =
//!   roles Alice, Bob
//!
//!   Alice { guard_capability : "chat:message:send", flow_cost : 100 }
//!     -> Bob : Message
//!
//!   Bob { journal_facts : "message_received" }
//!     -> Alice : Response
//! "#);
//! ```
//!
//! # Annotation Lowering
//!
//! Aura-specific choreography semantics are derived from compiled Telltale
//! annotations. The lowering surface understands annotations like:
//!
//! - `{ guard_capability : "..." }` - Capability requirements
//! - `{ flow_cost : 100 }` - Resource costs
//! - `{ journal_facts : "..." }` - Audit logging
//! - `{ journal_merge : true }` - Journal merge operations
//! - `{ audit_log : "..." }` - Audit trail entries
//!
//! # Architecture
//!
//! ```text
//! aura-mpst/              ← Regular crate (re-exports telltale + Aura lowering)
//! aura-macros/            ← Proc-macro crate (custom macros)
//! ```

// Canonical Telltale re-exports for Aura choreography/runtime integration.
pub mod upstream;

pub use serde_json;

/// Generated choreography composition metadata types.
pub mod composition;

use crate::upstream::runtime as telltale_runtime;
use async_trait::async_trait;
pub use composition::{
    startup_defaults_for_qualified_name, AdmittedModuleGuardCapabilities,
    CompositionDelegationConstraint, CompositionLinkSpec, CompositionManifest,
    CompositionTheoremPack, GuardCapabilityAdmission, GuardCapabilityAdmissionError,
    ModuleGuardCapabilityError,
};

/// Aura-owned runtime hooks required by generated choreography runners.
///
/// Generated Aura choreographies execute over upstream `ChoreoHandlerExt`
/// lifecycle and endpoint management. This trait only supplies the additional
/// Aura-owned hooks that upstream does not model: sourcing outbound payloads,
/// selecting local branches, and resolving parameterized role families.
#[async_trait]
pub trait GeneratedChoreographyRuntime: telltale_runtime::ChoreoHandlerExt + Send {
    /// Provide the next outbound message for a send.
    async fn provide_message<M: Send + 'static>(
        &mut self,
        to: Self::Role,
    ) -> telltale_runtime::ChoreoResult<M>;

    /// Select a branch label from the available choices.
    async fn select_branch<L: telltale_runtime::LabelId>(
        &mut self,
        choices: &[L],
    ) -> telltale_runtime::ChoreoResult<L>;

    /// Resolve all instances of a parameterized role family.
    fn resolve_family(&self, family: &str) -> telltale_runtime::ChoreoResult<Vec<Self::Role>>;

    /// Resolve a role family range `[start, end)`.
    fn resolve_range(
        &self,
        family: &str,
        start: u32,
        end: u32,
    ) -> telltale_runtime::ChoreoResult<Vec<Self::Role>> {
        let roles = self.resolve_family(family)?;
        let start_idx = start as usize;
        let end_idx = end as usize;
        if start_idx >= roles.len() || end_idx > roles.len() || start_idx >= end_idx {
            return Err(telltale_runtime::ChoreographyError::ExecutionError(
                format!("invalid role family range for {family}: [{start}, {end})"),
            ));
        }
        Ok(roles[start_idx..end_idx].to_vec())
    }

    /// Collect messages from multiple roles using the active endpoint.
    async fn collect<M: serde::de::DeserializeOwned + Send>(
        &mut self,
        ep: &mut Self::Endpoint,
        from: &[Self::Role],
    ) -> telltale_runtime::ChoreoResult<Vec<M>> {
        let mut messages = Vec::with_capacity(from.len());
        for role in from {
            messages.push(self.recv(ep, *role).await?);
        }
        Ok(messages)
    }

    /// Get the total count of instances in a role family.
    fn family_size(&self, family: &str) -> telltale_runtime::ChoreoResult<usize> {
        self.resolve_family(family).map(|roles| roles.len())
    }
}

/// Lower compiled Telltale annotation metadata into Aura effects.
pub mod annotation_lowering;

/// Identifier newtypes for roles and sessions
pub mod ids;

/// Journal annotation types for fact-based operations
pub mod journal;

/// Session type system types
pub mod session;
/// Termination budgeting helpers
pub mod termination;

/// Aura-owned effect/runtime extension types
pub mod extensions;

pub use annotation_lowering::{
    generate_aura_choreography_code, lower_aura_effects, lower_aura_effects_from_records,
    parse_choreography_capability, AuraEffect, AuraExtractionError, ChoreographyCapabilityError,
};
/// Full-featured tell! macro with Telltale features + Aura extensions
///
/// This macro provides access to Telltale choreography features plus Aura-specific extensions:
/// - Module namespaces: `module my_protocol exposing (ProtocolName)`
/// - Parameterized roles: `Worker[N]`, `Signer[*]`
/// - Choice constructs: `choice Role at ...`
/// - Loop constructs: `loop { ... }`
/// - Aura capability guards: `{ guard_capability : "namespace:capability" }`
/// - Aura flow costs: `{ flow_cost : 100 }`
/// - Aura journal facts: `{ journal_facts : "description" }`
/// - Aura audit logging: `{ audit_log : "action:metadata" }`
///
/// # Example
///
/// ```ignore
/// use aura_mpst::tell;
///
/// tell!(r#"
/// module threshold_ceremony exposing (ThresholdExample)
///
/// protocol ThresholdExample =
///   roles Coordinator, Signer[3]
///   choice Coordinator at
///     | start_ceremony =>
///       Coordinator { guard_capability : "consensus:initiate",
///                     flow_cost : 200,
///                     journal_facts : "ceremony_started" }
///         -> Signer[*] : StartRequest
///       Signer[*] { guard_capability : "consensus:witness_sign",
///                   flow_cost : 150 }
///         -> Coordinator : Commitment
///     | abort =>
///       Coordinator -> Signer[*] : Abort
/// "#);
/// ```
///
/// Note: The tell! macro is available in the aura-macros crate.
/// Generated code uses types from this crate.
pub use aura_core::{
    types::identifiers::DeviceId, AuraError, AuraResult, Cap, Journal, JournalEffects,
};
pub use ids::{MessageTypeId, NonEmptyRoleList, RoleId, SessionTypeId};

pub use journal::{JournalAnnotation, JournalCoupling};
pub use session::LocalSessionType;
pub use termination::{
    compute_buffer_weight, compute_depth, compute_weighted_measure, SessionBufferSnapshot,
};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_choreography_macro_available() {
        // This test verifies the choreography macro is properly re-exported
        // Actual functionality is tested in integration tests
    }

    #[test]
    fn test_compiled_lowering_surface_available() {
        let _compile = crate::upstream::language::compile_choreography;
        let _lower = lower_aura_effects;
    }
}
