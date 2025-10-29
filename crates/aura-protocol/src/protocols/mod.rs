//! Unified Protocol Lifecycle Implementations
//!
//! This module contains protocol lifecycle implementations following the
//! unified protocol-core architecture. All protocols use the LifecycleScheduler
//! for execution with session type safety and deterministic coordination.
//!
//! ## Architecture
//!
//! Protocols are implemented as lifecycle state machines using **protocol-core**:
//! - **Type Safety**: Session types ensure correct state transitions
//! - **Deterministic**: Injectable effects for reproducible execution
//! - **Composable**: Protocol capabilities provided through dependency injection
//! - **Testable**: Pure state machines with effects injected at boundaries
//!
//! ## Usage
//!
//! Execute protocols through LifecycleScheduler:
//! ```rust,ignore
//! let scheduler = LifecycleScheduler::with_effects(effects);
//! let result = scheduler.execute_dkd(session_id, account_id, device_id, ...).await?;
//! ```

// ========== Protocol Lifecycle Modules ==========
pub mod counter_lifecycle;
pub mod dkd_lifecycle;
pub mod frost_lifecycle;
pub mod group_lifecycle;
pub mod locking_lifecycle;
pub mod recovery_lifecycle;
pub mod resharing_lifecycle;
pub mod storage_lifecycle;

// ========== Supporting Modules ==========
pub mod base;
pub mod capability_helper;
pub mod protocol_traits;
pub mod traits;
pub mod wrapper;

// ========== Utility Protocols ==========
pub mod rendezvous;

// ========== Protocol Lifecycle Exports ==========
pub use counter_lifecycle::CounterLifecycle;
pub use dkd_lifecycle::DkdLifecycle;
pub use frost_lifecycle::FrostSigningLifecycle;
pub use group_lifecycle::GroupLifecycle;
pub use locking_lifecycle::LockingLifecycle;
pub use recovery_lifecycle::RecoveryLifecycle;
pub use resharing_lifecycle::ResharingLifecycle;
pub use storage_lifecycle::{
    BlobMetadata, StorageInput, StorageLifecycle, StorageOperationType, StorageOutput, StorageState,
};

// ========== Error Type Aliases ==========
pub type CounterLifecycleError = aura_types::AuraError;
pub type DkdLifecycleError = aura_types::AuraError;
pub type FrostLifecycleError = aura_types::AuraError;
pub type GroupLifecycleError = aura_types::AuraError;
pub type LockingLifecycleError = aura_types::AuraError;
pub type RecoveryLifecycleError = aura_types::AuraError;
pub type ResharingLifecycleError = aura_types::AuraError;
pub type StorageLifecycleError = aura_types::AuraError;

// ========== Supporting Exports ==========
pub use capability_helper::CapabilityProofBuilder;
pub use traits::*;
pub use wrapper::{ProtocolWrapper, ProtocolWrapperBuilder, ProtocolWrapperError};

// ========== Utility Protocol Exports ==========
// Message types re-exported from aura-messages
pub use aura_messages::protocol::{
    AuthenticationPayload, HandshakeResult, HandshakeTranscript, PayloadKind, PskHandshakeConfig,
    RendezvousEnvelope, StorageCapabilityAnnouncement, TransportDescriptor, TransportKind,
    TransportOfferPayload,
};

// Protocol implementation types re-exported from rendezvous module
pub use rendezvous::{RendezvousError, RendezvousProtocol};
