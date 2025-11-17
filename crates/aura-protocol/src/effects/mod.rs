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

/// Effect system type alias for compatibility
///
/// TEMPORARY: This is currently just a trait object.
/// In the future, this should be replaced with a concrete type from aura-agent.
pub type AuraEffectSystem = Box<dyn AuraEffects>;

/// Configuration for the effect system (temporary until moved to aura-agent)
#[derive(Debug, Clone)]
pub struct EffectSystemConfig {
    pub device_id: aura_core::DeviceId,
}

/// Factory functions for creating AuraEffectSystem instances
pub struct AuraEffectSystemFactory;

impl AuraEffectSystemFactory {
    /// Create a new effect system with the given configuration
    pub fn new(config: EffectSystemConfig) -> Result<AuraEffectSystem, aura_core::AuraError> {
        // For now, create a mock implementation to get compilation working
        // TODO: Replace with proper runtime composition from aura-agent
        let device_uuid = config.device_id.into(); // Convert DeviceId to Uuid
        let mock_impl = crate::handlers::CompositeHandler::for_testing(device_uuid);
        Ok(Box::new(mock_impl))
    }

    /// Create a new effect system from individual components
    pub fn from_components(
        _crypto: Box<dyn CryptoEffects>,
        _storage: Box<dyn aura_core::effects::StorageEffects>,
        _network: Box<dyn NetworkEffects>,
        _time: Box<dyn aura_core::effects::TimeEffects>,
        _console: Box<dyn ConsoleEffects>,
        _journal: Box<dyn JournalEffects>,
        _ledger: Box<dyn LedgerEffects>,
        _tree: Box<dyn TreeEffects>,
    ) -> Result<AuraEffectSystem, aura_core::AuraError> {
        // For now, just use a mock implementation
        // TODO: Implement proper component composition
        #[allow(clippy::disallowed_methods)]
        // Temporary mock implementation - will be refactored when proper component composition is implemented
        let device_uuid = uuid::Uuid::new_v4();
        let mock_impl = crate::handlers::CompositeHandler::for_testing(device_uuid);
        Ok(Box::new(mock_impl))
    }

    /// Create an effect system for testing with mock handlers
    pub fn for_testing(device_id: aura_core::DeviceId) -> AuraEffectSystem {
        let device_uuid = device_id.into(); // Convert DeviceId to Uuid
        let mock_impl = crate::handlers::CompositeHandler::for_testing(device_uuid);
        Box::new(mock_impl)
    }
}

// Trait implementations for Box<dyn AuraEffects>
// Since AuraEffects is a supertrait of all constituent traits, we need to provide
// implementations that delegate to the underlying trait object.

#[async_trait::async_trait]
impl ConsoleEffects for AuraEffectSystem {
    async fn log_debug(&self, message: &str) -> Result<(), aura_core::AuraError> {
        (**self).log_debug(message).await
    }

    async fn log_info(&self, message: &str) -> Result<(), aura_core::AuraError> {
        (**self).log_info(message).await
    }

    async fn log_warn(&self, message: &str) -> Result<(), aura_core::AuraError> {
        (**self).log_warn(message).await
    }

    async fn log_error(&self, message: &str) -> Result<(), aura_core::AuraError> {
        (**self).log_error(message).await
    }
}

#[async_trait::async_trait]
impl LedgerEffects for AuraEffectSystem {
    async fn append_event(&self, event: Vec<u8>) -> Result<(), crate::effects::LedgerError> {
        (**self).append_event(event).await
    }

    async fn current_epoch(&self) -> Result<u64, crate::effects::LedgerError> {
        LedgerEffects::current_epoch(&(**self)).await
    }

    async fn events_since(&self, epoch: u64) -> Result<Vec<Vec<u8>>, crate::effects::LedgerError> {
        (**self).events_since(epoch).await
    }

    async fn is_device_authorized(
        &self,
        device_id: aura_core::DeviceId,
        operation: &str,
    ) -> Result<bool, crate::effects::LedgerError> {
        (**self).is_device_authorized(device_id, operation).await
    }

    async fn get_device_metadata(
        &self,
        device_id: aura_core::DeviceId,
    ) -> Result<Option<crate::effects::DeviceMetadata>, crate::effects::LedgerError> {
        (**self).get_device_metadata(device_id).await
    }

    async fn update_device_activity(
        &self,
        device_id: aura_core::DeviceId,
    ) -> Result<(), crate::effects::LedgerError> {
        (**self).update_device_activity(device_id).await
    }

    async fn subscribe_to_events(
        &self,
    ) -> Result<crate::effects::LedgerEventStream, crate::effects::LedgerError> {
        (**self).subscribe_to_events().await
    }

    async fn would_create_cycle(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        new_edge: (Vec<u8>, Vec<u8>),
    ) -> Result<bool, crate::effects::LedgerError> {
        (**self).would_create_cycle(edges, new_edge).await
    }

    async fn find_connected_components(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<Vec<u8>>>, crate::effects::LedgerError> {
        (**self).find_connected_components(edges).await
    }

    async fn topological_sort(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
    ) -> Result<Vec<Vec<u8>>, crate::effects::LedgerError> {
        (**self).topological_sort(edges).await
    }

    async fn shortest_path(
        &self,
        edges: &[(Vec<u8>, Vec<u8>)],
        start: Vec<u8>,
        end: Vec<u8>,
    ) -> Result<Option<Vec<Vec<u8>>>, crate::effects::LedgerError> {
        (**self).shortest_path(edges, start, end).await
    }

    async fn generate_secret(&self, length: usize) -> Result<Vec<u8>, crate::effects::LedgerError> {
        (**self).generate_secret(length).await
    }

    async fn hash_data(&self, data: &[u8]) -> Result<[u8; 32], crate::effects::LedgerError> {
        (**self).hash_data(data).await
    }

    async fn new_uuid(&self) -> Result<uuid::Uuid, crate::effects::LedgerError> {
        (**self).new_uuid().await
    }

    async fn current_timestamp(&self) -> Result<u64, crate::effects::LedgerError> {
        LedgerEffects::current_timestamp(&(**self)).await
    }

    async fn ledger_device_id(&self) -> Result<aura_core::DeviceId, crate::effects::LedgerError> {
        (**self).ledger_device_id().await
    }
}

// Note: Additional trait implementations can be added as needed for compilation
