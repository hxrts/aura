//! Core Effect Trait Definitions
//!
//! This module contains pure trait definitions for all side-effect operations used by protocols.
//! Following the algebraic effects pattern, this module defines what effects can be performed,
//! while the protocol handlers define how those effects are implemented.
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
//! - **Time Effects**: Scheduling, timeouts, temporal coordination
//! - **Crypto Effects**: Basic cryptographic operations, random generation
//! - **Storage Effects**: Basic data persistence, key-value operations
//!
//! ## Usage Pattern
//!
//! ```rust,ignore
//! use aura_core::effects::{TimeEffects, CryptoEffects};
//!
//! // Pure protocol function that accepts effects
//! async fn execute_protocol_phase<E>(
//!     state: ProtocolState,
//!     effects: &E,
//! ) -> Result<ProtocolState, AuraError>
//! where
//!     E: TimeEffects + CryptoEffects,
//! {
//!     // Use effects for side-effect operations
//!     let timestamp = effects.current_timestamp().await;
//!     let random_data = effects.random_bytes_32().await;
//!
//!     // Pure logic using the effect results
//!     Ok(state.with_timestamp(timestamp))
//! }
//! ```

// Core effect trait definitions
pub mod authorization;
pub mod chaos;
pub mod console;
pub mod crypto;
pub mod journal;
pub mod migration; // Empty module - migration complete
pub mod network;
pub mod random;
pub mod reliability;
pub mod storage;
pub mod testing;
pub mod time;

// Re-export core effect traits
pub use authorization::{AuthorizationEffects, AuthorizationError};
pub use chaos::{ChaosEffects, ChaosError, CorruptionType, ByzantineType, ResourceType};
pub use console::ConsoleEffects;
pub use crypto::{CryptoEffects, CryptoError};
pub use journal::JournalEffects;
#[allow(deprecated)]
// Migration utilities removed - middleware transition complete
pub use network::{NetworkAddress, NetworkEffects, NetworkError, PeerEvent, PeerEventStream};
pub use random::RandomEffects;
pub use reliability::{ReliabilityEffects, ReliabilityError};
pub use storage::{StorageEffects, StorageError, StorageLocation, StorageStats};
pub use testing::{TestingEffects, TestingError};
pub use time::{TimeEffects, TimeError, TimeoutHandle, WakeCondition};

// Re-export unified error system
pub use crate::AuraError;
