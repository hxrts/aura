//! Shared Protocol Traits
//!
//! This module defines common traits and interfaces used across
//! all protocol implementations for consistency and interoperability.

use aura_types::{DeviceId};
use aura_journal::{Event};

/// Factory function type for creating new protocol instances
pub type ProtocolFactory<T> = fn(DeviceId, String, String) -> Result<T, Box<dyn std::error::Error>>;

/// Factory function type for rehydrating protocol instances from crash recovery
pub type ProtocolRehydrator<T> =
    fn(DeviceId, String, String, Vec<Event>) -> Result<T, Box<dyn std::error::Error>>;

/// Registry of available protocol implementations
pub struct ProtocolRegistry {
    // Future: Protocol factory registry for dynamic protocol loading
}

impl ProtocolRegistry {
    /// Create a new protocol registry
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ProtocolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
