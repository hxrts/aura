//! Test Transport Utilities
//!
//! Re-exports transport utilities from the aura-transport crate for testing.
//! Use `MemoryTransport` for all test transport needs.

// Re-export the main transport trait and MemoryTransport for tests
pub use aura_transport::{MemoryTransport, Transport};

/// Create a default memory transport for testing
///
/// Standard pattern for creating transport in tests.
pub fn test_memory_transport() -> MemoryTransport {
    use aura_types::DeviceId;
    MemoryTransport::new(DeviceId::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_memory_transport_creation() {
        let _transport = test_memory_transport();
        // Basic smoke test - just verify we can create it
    }
}
