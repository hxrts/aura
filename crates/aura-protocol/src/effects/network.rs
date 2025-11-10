//! Network effects trait definitions
//!
//! This module re-exports the NetworkEffects trait from aura-core to provide
//! a unified interface for network communication across the system.

// Re-export network traits and types from aura-core
pub use aura_core::effects::{
    NetworkAddress, NetworkEffects, NetworkError, PeerEvent, PeerEventStream,
};
