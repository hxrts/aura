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
pub mod group_lifecycle;
pub mod locking_lifecycle;
pub mod recovery_lifecycle;
pub mod resharing_lifecycle;

// ========== Supporting Modules ==========
pub mod base;
pub mod protocol_traits;
pub mod traits;
pub mod wrapper;

// ========== Utility Protocols ==========
pub mod rendezvous;

// ========== Protocol Lifecycle Exports ==========
pub use counter_lifecycle::{CounterLifecycle, CounterLifecycleError};
pub use dkd_lifecycle::{DkdLifecycle, DkdLifecycleError};
pub use group_lifecycle::{GroupLifecycle, GroupLifecycleError};
pub use locking_lifecycle::{LockingLifecycle, LockingLifecycleError};
pub use recovery_lifecycle::{RecoveryLifecycle, RecoveryLifecycleError};
pub use resharing_lifecycle::{ResharingLifecycle, ResharingLifecycleError};

// ========== Supporting Exports ==========
pub use traits::*;
pub use wrapper::{
    rehydrate_protocol, IntoProtocolWrapper, ProtocolWrapper, ProtocolWrapperBuilder,
    ProtocolWrapperError,
};

// ========== Utility Protocol Exports ==========
pub use rendezvous::{
    AuthenticationPayload, HandshakeResult, HandshakeTranscript, PayloadKind, PskHandshakeConfig,
    RendezvousEnvelope, RendezvousError, RendezvousProtocol, TransportDescriptor, TransportKind,
    TransportOfferPayload,
};
