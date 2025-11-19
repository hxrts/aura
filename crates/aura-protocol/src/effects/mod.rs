//! Protocol Effects Module
//!
//! This module contains pure trait definitions for all side-effect operations used by protocols.
//! Following the algebraic effects pattern, this module defines what effects can be performed,
//! while the handlers module defines how those effects are implemented.
//!
//! ## Architecture Principles
//!
//! 1. **Pure Traits**: This module contains only trait definitions, no implementations
//! 2. **Effect Isolation**: All side effects are abstracted through these interfaces
//! 3. **Algebraic Effects**: Designed to work with handlers that interpret these effects
//! 4. **Composability**: Effects can be combined and decorated with middleware
//!
//! ## Effect Categories
//!
//! This module contains protocol-specific effect traits that extend the core Aura effect system:
//!
//! - **Agent Effects**: Device authentication, session management, configuration
//! - **Choreographic Effects**: Multi-party protocol coordination, session types
//! - **Ledger Effects**: Event sourcing, audit trail, device authorization
//! - **Sync Effects**: Anti-entropy coordination, state reconciliation
//! - **Tree Effects**: Ratchet tree operations, group key management
//! - **Tree Coordination Effects**: Complex tree protocol orchestration
//!
//! For basic effects (Network, Storage, Crypto, Time, Console, Random), see `aura-core`.
//! This separation ensures proper layer boundaries in the 8-layer architecture.
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use crate::effects::{NetworkEffects, CryptoEffects, TimeEffects};
//!
//! // Pure protocol function that accepts effects
//! async fn execute_protocol_phase<E>(
//!     state: ProtocolState,
//!     effects: &E,
//! ) -> Result<ProtocolState, ProtocolError>
//! where
//!     E: NetworkEffects + CryptoEffects + TimeEffects,
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
pub mod cli;
pub mod ledger;
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
pub use cli::{
    CliConfig, CliEffectHandler, CliEffects, ConfigEffects, LoggingConfig, NetworkConfig,
    OutputEffectHandler, OutputEffects, OutputFormat,
};
// Import core effects from aura-core
pub use aura_core::effects::{
    ConsoleEffects, CryptoEffects, CryptoError, JournalEffects, NetworkAddress, NetworkEffects,
    NetworkError, PeerEvent, PeerEventStream, RandomEffects, StorageEffects, StorageError,
    StorageLocation, StorageStats, SystemEffects, SystemError, TimeEffects, TimeError,
    TimeoutHandle, WakeCondition,
};

// Import crypto-specific types from crypto module
pub use aura_core::effects::crypto::{FrostSigningPackage, KeyDerivationContext};
// Note: Removed duplicate re-exports to avoid conflicts with aura_core imports
// Only re-export types that are protocol-specific and don't conflict with aura-core

pub use ledger::{DeviceMetadata, LedgerEffects, LedgerError, LedgerEvent, LedgerEventStream};
pub use params::*; // Re-export all parameter types
pub use semilattice::{
    CausalContext, CmHandler, CvHandler, DeliveryConfig, DeliveryEffect, DeliveryGuarantee,
    DeltaHandler, GossipStrategy, HandlerFactory, TopicId,
};
pub use sync::{AntiEntropyConfig, BloomDigest, SyncEffects, SyncError};
pub use tree::TreeEffects;

// Re-export unified error system
pub use aura_core::{AuraError, AuraResult};

// SystemEffects trait and SystemError now imported from aura-core
// (was previously in system_traits module, now moved to Layer 1)

// NOTE: Runtime infrastructure has been moved to aura-agent (Layer 6)
// The following types are now available from aura_agent::runtime:
// - AuraEffectSystem, EffectSystemConfig, StorageConfig
// - EffectSystemBuilder, HandlerContainer
// - EffectExecutor, LifecycleManager
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
    + TimeEffects
    + RandomEffects
    + ConsoleEffects
    + JournalEffects
    + LedgerEffects
    + TreeEffects
    + ChoreographicEffects
    + SystemEffects
    + Send
    + Sync
{
    /// Get the execution mode of this effects implementation
    fn execution_mode(&self) -> aura_core::effects::ExecutionMode;
}

// NOTE: AuraEffectSystem and factory moved to aura-agent runtime (Layer 6)
// The following types are now available from aura_agent::runtime:
// - AuraEffectSystem: Concrete effect system type
// - EffectSystemConfig: System configuration
// - EffectRegistry: Builder pattern for effect composition
// - EffectBuilder: Compile-time type-safe effect building

/// Protocol requirement specification stub
///
/// This trait provides a placeholder for protocol requirements.
/// For full implementation, use aura_agent::runtime::ProtocolRequirements.
pub trait ProtocolRequirements {
    /// Type-level specification of required effects
    type Requirements;
}

// Note: AuraEffectSystem concrete type and implementations moved to aura-agent runtime
