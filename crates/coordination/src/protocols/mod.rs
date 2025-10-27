//! Complete Protocol Implementations
//!
//! This module contains complete protocol implementations that combine
//! session type definitions with choreographic execution logic.
//! Each protocol is self-contained and provides both compile-time safety
//! and runtime execution.
//!
//! ## Choreographic Programming
//!
//! Protocols are implemented using **choreographic programming**, where
//! distributed protocols are written as linear async functions that look like
//! single-threaded code but coordinate across multiple devices.
//!
//! Benefits:
//! - **Global viewpoint**: Protocol described as single program
//! - **Local projection**: Each device executes its role automatically
//! - **Session types**: Communication patterns type-checked
//! - **Deadlock freedom**: Guaranteed by choreographic structure

// Core protocol modules
pub mod base;
pub mod dkd;
pub mod protocol_traits;
pub mod recovery;
pub mod resharing;
pub mod traits;
pub mod wrapper;

// Utility protocol modules
pub mod counter;
pub mod locking;
pub mod rendezvous;

// Re-export all protocol implementations
pub use dkd::{
    dkd_choreography, new_dkd_protocol, rehydrate_dkd_protocol, DkdProtocolCore, DkdProtocolState,
    DkdSessionError,
};
pub use recovery::{
    new_recovery_protocol, nudge_guardian, recovery_choreography, rehydrate_recovery_protocol,
    RecoveryProtocolCore, RecoveryProtocolState, RecoverySessionError,
};
pub use resharing::{
    new_resharing_protocol, rehydrate_resharing_protocol, resharing_choreography,
    ResharingProtocolCore, ResharingProtocolState, ResharingSessionError,
};
pub use traits::*;

// Re-export protocol wrapper utilities
pub use wrapper::{
    rehydrate_protocol, IntoProtocolWrapper, ProtocolWrapper, ProtocolWrapperBuilder,
    ProtocolWrapperError,
};

// Re-export utility protocol implementations
pub use counter::{
    counter_increment_choreography, counter_range_choreography, CounterReservationConfig,
    CounterReservationResult,
};
pub use locking::locking_choreography;
pub use rendezvous::{
    AuthenticationPayload, HandshakeResult, HandshakeTranscript, PayloadKind, PskHandshakeConfig,
    RendezvousEnvelope, RendezvousError, RendezvousProtocol, TransportDescriptor, TransportKind,
    TransportOfferPayload,
};
