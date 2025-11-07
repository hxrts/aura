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
pub mod choreographic;
pub mod console;
pub mod crypto;
pub mod journal;
pub mod ledger;
pub mod network;
pub mod params;
pub mod random;
pub mod semilattice;
pub mod storage;
pub mod system;
pub mod time;

// Re-export core effect traits
pub use choreographic::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent,
    ChoreographyMetrics,
};
pub use console::{ConsoleEffects, ConsoleEvent, LogLevel};
pub use crypto::{CryptoEffects, CryptoError};
pub use journal::JournalEffects;
pub use ledger::{DeviceMetadata, LedgerEffects, LedgerError, LedgerEvent, LedgerEventStream};
pub use network::{NetworkAddress, NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
pub use params::*; // Re-export all parameter types
pub use random::RandomEffects;
pub use semilattice::{
    CausalContext, CmHandler, CvHandler, DeliveryConfig, DeliveryEffect, DeliveryGuarantee,
    DeltaHandler, GossipStrategy, HandlerFactory, TopicId,
};
pub use storage::{StorageEffects, StorageError, StorageLocation, StorageStats};
pub use time::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};

// Re-export unified error system
pub use aura_types::{AuraError, AuraResult, ErrorCode, ErrorSeverity};

// Re-export unified effect system
pub use system::{AuraEffectSystem, AuraEffectSystemFactory, AuraEffectSystemStats};

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
    + ConsoleEffects
    + RandomEffects
    + LedgerEffects
    + JournalEffects
    + ChoreographicEffects
    + Send
    + Sync
{
}

// Note: AuraEffects trait is already defined above, no need to re-export
