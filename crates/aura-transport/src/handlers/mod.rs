//! High-level protocol handlers that use the unified transport system
//!
//! This module demonstrates how to build handlers using the new architecture:
//! Core Transport -> Adapter -> Handler

pub mod example;
pub mod in_memory;

pub use example::*;

// Backward compatibility exports
// TODO: These should be migrated to use the new unified system
pub use crate::core::MemoryTransport as InMemoryTransport;

/// Stub NetworkHandler for backward compatibility
/// TODO: Implement using new unified transport system
pub struct NetworkHandler;

impl NetworkHandler {
    /// Create a new network handler for the given device
    pub fn new(_device_id: aura_types::DeviceId) -> Self {
        Self
    }
}

/// Backward compatibility alias
pub use NetworkHandler as InMemoryHandler;
