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
//! - **Network Effects**: Peer communication, message passing
//! - **Storage Effects**: Data persistence, key-value operations
//! - **Crypto Effects**: Cryptographic operations, random generation
//! - **Time Effects**: Scheduling, timeouts, temporal coordination
//! - **Console Effects**: Logging, debugging, visualization
//! - **Ledger Effects**: Account state, event sourcing
//! - **Choreographic Effects**: Distributed protocol coordination
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
pub mod agent;
pub mod choreographic;
pub mod ledger;
pub mod params;
pub mod semilattice;
pub mod sync;
pub mod system; // New stateless AuraEffectSystem (previously system_v2)
pub mod system_traits; // SystemEffects trait and SystemError
pub mod tree;
pub mod tree_coordination;

// Re-export core effect traits
pub use agent::{
    AgentEffects, AgentHealthStatus, AuthMethod, AuthenticationEffects, AuthenticationResult,
    BiometricType, ConfigValidationError, ConfigurationEffects, CredentialBackup, DeviceConfig,
    DeviceInfo, DeviceStorageEffects, HealthStatus, SessionHandle, SessionInfo,
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
    StorageLocation, StorageStats, TimeEffects, TimeError, TimeoutHandle, WakeCondition,
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
pub use tree_coordination::{
    ApprovalStatus, ApprovalVote, CloseReason, CoordinationConfig, CoordinationError,
    CoordinationEvent, ReconcileResult, SessionId, SessionInfo as TreeSessionInfo,
    SessionRole as TreeSessionRole, SessionStatus as TreeSessionStatus, SyncPhase, SyncProgress,
    TreeCoordinationEffects, TreeDigest, ValidationContext, ValidationResult, VoteDecision,
};

// Re-export unified error system
pub use aura_core::{AuraError, AuraResult};

// SystemEffects trait and SystemError now in system_traits module
pub use system_traits::{SystemEffects, SystemError};

// Stateless effect system components
pub mod allocations;
pub mod builder;
pub mod caching;
pub mod container;
pub mod context;
pub mod contextual;
pub mod executor;
pub mod handler_adapters;
pub mod lifecycle;
pub mod parallel_init;
pub mod propagation;
pub mod reliability;
pub mod services;

pub use allocations::{Arena, BufferPool, SmallVec, StringInterner, ZeroCopyString};
pub use builder::AuraEffectSystemBuilder;
pub use caching::{CacheKey, CachingNetworkHandler, CachingStorageHandler, EffectCache};
pub use executor::{EffectExecutor, EffectExecutorBuilder};
pub use parallel_init::{
    HandlerPool, InitializationMetrics, LazyEffectSystem, ParallelInitBuilder,
};
pub use reliability::{CircuitBreakerConfig, ReliabilityCoordinator, RetryConfig};
pub use services::{BudgetKey, ContextManager, FlowBudgetManager, ReceiptChain, ReceiptManager};
// Stateless effect system
pub use system::{AuraEffectSystem, EffectSystemConfig, StorageConfig};

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
}

// Note: AuraEffects trait is already defined above, no need to re-export
