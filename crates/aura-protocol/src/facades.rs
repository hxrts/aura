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
    clippy::disallowed_methods, // Orchestration layer coordinates time/random effects
    deprecated // Deprecated time/random functions used intentionally for effect coordination
)]
//! High-level facade traits for common aura-protocol usage patterns
//!
//! This module provides simplified interfaces for common aura-protocol operations,
//! making it easier to use the protocol orchestration capabilities without
//! needing to understand all the implementation details.
//!
//! ## Architecture Note
//!
//! This module contains only trait definitions appropriate for Layer 4.
//! Concrete implementations belong in aura-agent (Layer 6) where they can
//! depend on the full effect system infrastructure. This maintains proper
//! layering while providing useful abstractions.

use async_trait::async_trait;
use aura_core::identifiers::DeviceId;

/// High-level protocol orchestrator for executing distributed protocols
///
/// This facade simplifies protocol execution by providing a unified interface
/// for common choreography and coordination patterns.
///
/// # Example
///
/// ```rust,ignore
/// use aura_protocol::facades::ProtocolOrchestrator;
///
/// let orchestrator = MyOrchestrator::new(effect_system);
/// let result = orchestrator.execute_choreography(my_protocol).await?;
/// ```
///
/// # Implementation Note
///
/// Implementations of this trait should be provided in aura-agent (Layer 6)
/// where concrete effect system types are available.
#[async_trait]
pub trait ProtocolOrchestrator {
    /// The type of protocol this orchestrator can execute
    type Protocol;
    /// The output type produced by protocol execution
    type Output;
    /// Error type for protocol execution failures
    type Error: From<aura_core::AuraError>;

    /// Execute a choreographic protocol with proper coordination
    ///
    /// This method handles:
    /// - Session establishment and coordination
    /// - Effect system integration
    /// - Error handling and recovery
    /// - Resource cleanup
    async fn execute_choreography(
        &self,
        protocol: Self::Protocol,
    ) -> Result<Self::Output, Self::Error>;

    /// Get the device ID for this orchestrator
    fn device_id(&self) -> DeviceId;
}

/// Standard patterns for common protocol coordination scenarios
///
/// This trait provides high-level interfaces for proven coordination patterns,
/// eliminating the need to implement these common scenarios from scratch.
///
/// # Example
///
/// ```rust,ignore
/// use aura_protocol::facades::StandardPatterns;
///
/// let patterns = MyStandardPatterns::new(effect_system);
/// let result = patterns.anti_entropy_sync(peers).await?;
/// ```
///
/// # Implementation Note
///
/// Implementations of this trait should be provided in aura-agent (Layer 6)
/// where concrete effect system types are available.
#[async_trait]
pub trait StandardPatterns {
    /// Error type for pattern execution failures
    type Error: From<aura_core::AuraError>;

    /// Execute anti-entropy synchronization with a set of peers
    ///
    /// This handles:
    /// - Peer discovery and connection management
    /// - State comparison and reconciliation
    /// - Conflict resolution using semilattice operations
    async fn anti_entropy_sync(&self, peers: Vec<DeviceId>) -> Result<(), Self::Error>;

    /// Coordinate a threshold ceremony with participants
    ///
    /// This handles:
    /// - Participant coordination and communication
    /// - Key generation or signing ceremony execution
    /// - Result collection and verification
    async fn threshold_ceremony<T, R>(
        &self,
        ceremony_type: T,
        participants: Vec<DeviceId>,
    ) -> Result<R, Self::Error>
    where
        T: Send + Sync + 'static,
        R: Send + Sync + 'static;

    /// Manage a multi-party session with lifecycle handling
    ///
    /// This handles:
    /// - Session establishment and teardown
    /// - Participant join/leave coordination
    /// - Session state synchronization
    async fn multi_party_session<P, R>(
        &self,
        protocol: P,
        participants: Vec<DeviceId>,
    ) -> Result<R, Self::Error>
    where
        P: Send + Sync + 'static,
        R: Send + Sync + 'static;

    /// Get the device ID for this pattern coordinator
    fn device_id(&self) -> DeviceId;
}

// NOTE: Concrete implementations (DefaultProtocolOrchestrator, etc.) live
// in aura-agent. This file contains only trait definitions that are
// appropriate for Layer 4 (aura-protocol).
//
// To use these traits:
// 1. Import the traits from aura-protocol::facades
// 2. Use implementations from aura-agent::facades (or implement your own)
//
// Example:
// ```rust,ignore
// use aura_protocol::facades::ProtocolOrchestrator;
// use aura_agent::facades::DefaultProtocolOrchestrator;
//
// let orchestrator = DefaultProtocolOrchestrator::new(device_id, effect_system);
// orchestrator.execute_choreography(protocol).await?;
// ```
//
// NOTE: Concrete implementations require Layer 6 runtime assembly, so they live
// in aura-agent. This maintains proper layer boundaries while providing the
// traits needed for protocol coordination at Layer 4.
