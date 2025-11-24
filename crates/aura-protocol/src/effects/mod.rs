//! Layer 4: Protocol-Specific Effect Traits
//!
//! Effect traits extending core capabilities (aura-core Layer 1) with protocol-level concerns.
//! Maintains strict separation between core (Layer 1) and protocol (Layer 4) effect hierarchies.
//!
//! **Effect Hierarchy** (per docs/001_system_architecture.md, docs/106_effect_system_and_runtime.md):
//! - **Core effects** (aura-core): Network, Crypto, Storage, Time, Random, Console, Random
//!   - Layer 1, foundational, no dependencies
//! - **Protocol effects** (aura-protocol): Choreographic, EffectAPI, Tree, Semilattice, Sync
//!   - Layer 4, compose core effects for distributed coordination
//! - **Domain effects** (aura-journal, aura-wot, etc.): Journal, Authorization, Capability
//!   - Layer 2, domain-specific application effects
//! - **UI effects** (aura-cli): CLI-specific effects for command-line interface
//!   - Layer 7, user interface abstractions
//!
//! **Key Protocol Effect Traits**:
//! - **Choreographic**: Multi-party protocol coordination, choreographic projections
//! - **EffectAPI**: Event sourcing, audit trail, authorization tracking (transaction log)
//! - **Tree**: Commitment tree operations, group key management
//! - **Semilattice**: CRDT coordination with mathematical invariants (⊔, ⊓)
//! - **Sync**: Anti-entropy, state reconciliation, gossip protocols
//!
//! **Guard Chain Integration**: Each protocol message expands through effects in order
//! (per docs/109_authorization.md): Authorization → FlowBudget → Leakage → Journal → Transport.
//! use crate::effects::{NetworkEffects, CryptoEffects, PhysicalTimeEffects};
//!
//! // Pure protocol function that accepts effects
//! async fn execute_protocol_phase<E>(
//!     state: ProtocolState,
//!     effects: &E,
//! ) -> Result<ProtocolState, ProtocolError>
//! where
//!     E: NetworkEffects + CryptoEffects + PhysicalTimeEffects,
//! {
//!     // Use effects for side-effect operations
//!     let signature = effects.ed25519_sign(&data, &key).await?;
//!     effects.send_to_peer(peer_id, message).await?;
//!
//!     // Pure logic using the effect results
//!     Ok(state.with_signature(signature))
//! }
//! ```

// Effect trait definitions
// NOTE: Agent effect traits moved to aura-core (Layer 1) - foundational capability definitions
pub mod choreographic;
pub mod effect_api;
pub mod params;
pub mod semilattice;
pub mod sync;
pub mod tree;

// Re-export agent effect traits from aura-core (moved to Layer 1)
pub use aura_core::effects::{
    AgentEffects, AgentHealthStatus, AuthMethod, AuthenticationEffects, AuthenticationResult,
    BiometricType, ConfigError, ConfigValidationError, ConfigurationEffects, CredentialBackup,
    DeviceConfig, DeviceInfo, DeviceStorageEffects, HealthStatus, SessionHandle, SessionInfo,
    SessionManagementEffects, SessionMessage, SessionRole, SessionStatus, SessionType,
};
pub use choreographic::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent,
    ChoreographyMetrics,
};
// Import core effects from aura-core
pub use aura_core::effects::{
    ConsoleEffects, CryptoEffects, CryptoError, JournalEffects, NetworkAddress, NetworkEffects,
    NetworkError, PeerEvent, PeerEventStream, RandomEffects, StorageEffects, StorageError,
    StorageLocation, StorageStats, SystemEffects, SystemError, TimeError, TimeoutHandle,
    WakeCondition,
};
// Domain-specific time traits (unified time system)
pub use aura_core::effects::{LogicalClockEffects, OrderClockEffects, PhysicalTimeEffects};

// Import crypto-specific types from crypto module
pub use aura_core::effects::crypto::{FrostSigningPackage, KeyDerivationContext};
// Note: Removed duplicate re-exports to avoid conflicts with aura_core imports
// Only re-export types that are protocol-specific and don't conflict with aura-core

pub use effect_api::{EffectApiEffects, EffectApiError, EffectApiEvent, EffectApiEventStream};
pub use params::*; // Re-export all parameter types
pub use semilattice::{
    CausalContext, CmHandler, CvHandler, DeliveryConfig, DeliveryEffect, DeliveryGuarantee,
    DeltaHandler, GossipStrategy, TopicId,
};
pub use sync::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError};
pub use tree::TreeEffects;

// Re-export unified error system
pub use aura_core::{AuraError, AuraResult};

// SystemEffects trait and SystemError now imported from aura-core
// (was previously in system_traits module, now moved to Layer 1)

// NOTE: Runtime infrastructure has been moved to aura-composition (Layer 3) and aura-agent (Layer 6)
// Handler composition available from aura_composition::
// - EffectBuilder, EffectRegistry, HandlerContainer
// Runtime assembly available from aura_agent::runtime:
// - AuraEffectSystem, EffectSystemConfig, LifecycleManager
// - Context management, services, optimizations
//
/// Composite trait that combines all effect traits
///
/// This trait combines all individual effect traits into a single trait object
/// that can be used by middleware and other components that need access to
/// multiple effect categories.
pub trait AuraEffects:
    CryptoEffects
    + NetworkEffects
    + StorageEffects
    + PhysicalTimeEffects
    + LogicalClockEffects
    + OrderClockEffects
    + RandomEffects
    + ConsoleEffects
    + JournalEffects
    + EffectApiEffects
    + TreeEffects
    + ChoreographicEffects
    + SystemEffects
    + Send
    + Sync
{
    /// Get the execution mode of this effects implementation
    fn execution_mode(&self) -> aura_core::effects::ExecutionMode;
}

// NOTE: Effect composition moved to aura-composition (Layer 3), runtime moved to aura-agent (Layer 6)
// Handler composition available from aura_composition::
// - EffectRegistry: Builder pattern for effect composition
// - EffectBuilder: Compile-time type-safe effect building
// Runtime assembly available from aura_agent::runtime:
// - AuraEffectSystem: Concrete effect system type
// - EffectSystemConfig: System configuration

/// Protocol requirement specification stub
///
/// This trait provides a placeholder for protocol requirements.
/// For full implementation, use aura_composition::ProtocolRequirements.
pub trait ProtocolRequirements {
    /// Type-level specification of required effects
    type Requirements;
}

// Note: AuraEffectSystem concrete type and implementations moved to aura-agent runtime
