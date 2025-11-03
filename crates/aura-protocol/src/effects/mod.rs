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
pub mod ledger;
pub mod network;
pub mod storage;
pub mod time;

// Re-export all effect traits
pub use choreographic::{
    ChoreographicEffects, ChoreographicRole, ChoreographyError, ChoreographyEvent, ChoreographyMetrics,
};
pub use console::{ConsoleEffect, ConsoleEffects};
pub use crypto::{CryptoEffects, CryptoError};
pub use ledger::{DeviceMetadata, LedgerEffects, LedgerError, LedgerEvent, LedgerEventStream};
pub use network::{NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
pub use storage::{StorageEffects, StorageError, StorageStats};
pub use time::{TimeEffects, TimeError, WakeCondition, TimeoutHandle};

// Re-export unified error system
pub use aura_types::{AuraError, AuraResult, ErrorCode, ErrorSeverity};

/// Combined protocol effects interface
///
/// This trait represents the union of all effects needed by most protocol operations.
/// Individual protocols can also depend on subsets of effects for more targeted implementations.
pub trait ProtocolEffects:
    NetworkEffects
    + StorageEffects
    + CryptoEffects
    + TimeEffects
    + ConsoleEffects
    + LedgerEffects
    + ChoreographicEffects
    + Send
    + Sync
{
    /// Get the device ID for this effects context
    fn device_id(&self) -> uuid::Uuid;

    /// Check if we're running in a simulated/test environment
    fn is_simulation(&self) -> bool;
}

/// Minimal effects interface for simple operations
///
/// Some protocols may only need a subset of effects. This trait provides
/// the most commonly needed effects without the full ProtocolEffects overhead.
pub trait MinimalEffects: CryptoEffects + TimeEffects + Send + Sync {
    /// Get the device ID for this effects context
    fn device_id(&self) -> uuid::Uuid;
}

