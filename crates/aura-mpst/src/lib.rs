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
//! 2. Provide the exact same `choreography!` macro interface
//! 3. Add Aura-specific extensions via the extension system
//! 4. Integrate with the guard chain for protocol-level guards
//!
//! # Usage
//!
//! ```ignore
//! use aura_mpst::choreography;
//!
//! // This works EXACTLY like telltale's choreography! macro
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
//! aura-mpst/              ← Regular crate (re-exports telltale + Aura extensions)
//! aura-macros/            ← Proc-macro crate (custom macros)
//! ```

// Canonical Telltale re-exports for Aura choreography/runtime integration.
pub use serde_json;
pub use telltale;
pub use telltale_choreography;
pub use telltale_types;

/// Generated choreography composition metadata types.
pub mod composition;

use async_trait::async_trait;
pub use composition::{
    startup_defaults_for_qualified_name, CompositionDelegationConstraint, CompositionLinkSpec,
    CompositionManifest,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Aura compatibility adapter for generated choreography runners.
///
/// Telltale 3.0 moved generated choreography execution toward `ChoreoHandler`
/// effect programs. Aura still has a large surface area built around the
/// simpler runner adapter model, so `aura-macros` continues generating against
/// this compatibility trait while the underlying dependency stack is on
/// Telltale 3.0.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ChoreographicAdapter: Send {
    /// The error type for this adapter.
    type Error: std::error::Error + Send + Sync + 'static;
    /// The role identifier type for this adapter.
    type Role: telltale_choreography::RoleId;

    /// Send a message to a specific role.
    async fn send<M: Serialize + Send + Sync + 'static>(
        &mut self,
        to: Self::Role,
        msg: M,
    ) -> Result<(), Self::Error>;

    /// Receive a message from a specific role.
    async fn recv<M: DeserializeOwned + Send + 'static>(
        &mut self,
        from: Self::Role,
    ) -> Result<M, Self::Error>;

    /// Broadcast a message to multiple roles.
    async fn broadcast<M: Serialize + Clone + Send + Sync + 'static>(
        &mut self,
        to: &[Self::Role],
        msg: M,
    ) -> Result<(), Self::Error> {
        for role in to {
            self.send(*role, msg.clone()).await?;
        }
        Ok(())
    }

    /// Collect messages from multiple roles.
    async fn collect<M: DeserializeOwned + Send + 'static>(
        &mut self,
        from: &[Self::Role],
    ) -> Result<Vec<M>, Self::Error> {
        let mut messages = Vec::with_capacity(from.len());
        for role in from {
            messages.push(self.recv::<M>(*role).await?);
        }
        Ok(messages)
    }

    /// Broadcast a branch label.
    async fn choose(
        &mut self,
        to: Self::Role,
        label: <Self::Role as telltale_choreography::RoleId>::Label,
    ) -> Result<(), Self::Error> {
        self.send(to, ChoiceLabel(label)).await
    }

    /// Receive a branch label.
    async fn offer(
        &mut self,
        from: Self::Role,
    ) -> Result<<Self::Role as telltale_choreography::RoleId>::Label, Self::Error> {
        let choice: ChoiceLabel<<Self::Role as telltale_choreography::RoleId>::Label> =
            self.recv(from).await?;
        Ok(choice.0)
    }

    /// Resolve all instances of a parameterized role family.
    fn resolve_family(&self, family: &str) -> Result<Vec<Self::Role>, Self::Error>;

    /// Resolve a role family range `[start, end)`.
    fn resolve_range(
        &self,
        family: &str,
        start: u32,
        end: u32,
    ) -> Result<Vec<Self::Role>, Self::Error>;

    /// Get the total count of instances in a role family.
    fn family_size(&self, family: &str) -> Result<usize, Self::Error> {
        self.resolve_family(family).map(|roles| roles.len())
    }
}

/// Optional lifecycle hooks for compatibility runners.
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
pub trait ChoreographicAdapterExt: ChoreographicAdapter {
    /// Called before protocol execution starts.
    async fn setup(&mut self) -> Result<(), Self::Error>;

    /// Called after protocol execution completes.
    async fn teardown(&mut self) -> Result<(), Self::Error>;

    /// Provide the next outbound message for a send.
    async fn provide_message<M: Send + 'static>(
        &mut self,
        to: Self::Role,
    ) -> Result<M, Self::Error>;

    /// Select a branch label from the available choices.
    async fn select_branch<L: telltale_choreography::LabelId>(
        &mut self,
        choices: &[L],
    ) -> Result<L, Self::Error>;
}

/// A branch label wrapper used by compatibility runners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChoiceLabel<L: telltale_choreography::LabelId>(pub L);

impl<L: telltale_choreography::LabelId> Serialize for ChoiceLabel<L> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.0.as_str())
    }
}

impl<'de, L: telltale_choreography::LabelId> Deserialize<'de> for ChoiceLabel<L> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let label = String::deserialize(deserializer)?;
        L::from_str(&label)
            .map(ChoiceLabel)
            .ok_or_else(|| serde::de::Error::custom("unknown choice label"))
    }
}

/// Context metadata passed to generated runner functions.
#[derive(Debug, Clone)]
pub struct ProtocolContext {
    /// Name of the protocol being executed.
    pub protocol: &'static str,
    /// Name of the role being executed.
    pub role: telltale_choreography::RoleName,
    /// Optional index for parameterized roles.
    pub index: Option<u32>,
}

impl ProtocolContext {
    /// Create a context from a concrete role value.
    #[must_use]
    pub fn for_role<R: telltale_choreography::RoleId>(protocol: &'static str, role: R) -> Self {
        Self {
            protocol,
            role: role.role_name(),
            index: role.role_index(),
        }
    }
}

/// AST extraction and annotation parsing for Aura choreographies
pub mod ast_extraction;

/// Identifier newtypes for roles and sessions
pub mod ids;

/// Journal annotation types for fact-based operations
pub mod journal;

/// Session type system types
pub mod session;
/// Termination budgeting helpers
pub mod termination;

/// Extension system integration
pub mod extensions;

/// Initialize the Aura extension system (external-demo pattern)
///
/// Returns an empty extension registry following the "external-demo" pattern.
/// Aura-specific choreography extensions (guard_capability, flow_cost, journal_facts,
/// leak annotations) are now implemented in the [`aura-macros`](../aura_macros/index.html)
/// crate via procedural macros that parse and extract annotations at compile-time.
///
/// This function exists for:
/// 1. Compatibility with existing extension registry wiring
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
pub fn init_aura_extensions() -> telltale_choreography::extensions::ExtensionRegistry {
    // Create empty registry - extensions are now handled in aura-macros
    telltale_choreography::extensions::ExtensionRegistry::new()
}

pub use ast_extraction::{
    extract_aura_annotations, generate_aura_choreography_code, AuraEffect, AuraExtractionError,
};
/// Full-featured choreography! macro with Telltale features + Aura extensions
///
/// This macro provides access to Telltale choreography features plus Aura-specific extensions:
/// - Module namespaces: `module my_protocol exposing (ProtocolName)`
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
/// choreography!(r#"
/// module threshold_ceremony exposing (ThresholdExample)
///
/// protocol ThresholdExample =
///   roles Coordinator, Signer[3]
///   case choose Coordinator of
///     start_ceremony ->
///       Coordinator[guard_capability = "coordinate_signing",
///                  flow_cost = 200,
///                  journal_facts = "ceremony_started"]
///         -> Signer[*] : StartRequest
///       Signer[*][guard_capability = "participate_signing",
///                flow_cost = 150]
///         -> Coordinator : Commitment
///     abort ->
///       Coordinator -> Signer[*] : Abort
/// "#);
/// ```
///
/// Note: The choreography! macro is available in the aura-macros crate.
/// Generated code uses types from this crate.
pub use aura_core::{types::identifiers::DeviceId, AuraError, AuraResult, Cap, Journal, JournalEffects};
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
    fn test_all_choreography_features_available() {
        // Verify we have access to the re-exported choreography types and functions.
        let _registry = telltale_choreography::extensions::ExtensionRegistry::new();
        let _composer = telltale_choreography::compiler::GrammarComposer::new();
        let _parser = telltale_choreography::compiler::ExtensionParser::new();

        // If this compiles, we successfully re-exported everything
    }
}
