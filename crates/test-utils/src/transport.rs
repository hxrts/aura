//! Test Transport Utilities
//!
//! Factory functions for creating test transport instances.
//! Consolidates transport setup patterns found in test files.

use std::sync::Arc;

/// Create a default memory transport for testing
///
/// Standard pattern for creating transport in tests.
/// This assumes the MemoryTransport is available from aura_transport.
pub fn test_transport_memory() -> Arc<dyn TestTransport> {
    Arc::new(MemoryTransportImpl)
}

/// Create a memory transport with specific configuration
///
/// For tests that need to configure transport behavior.
pub fn test_transport_configured() -> Arc<dyn TestTransport> {
    // This would be implemented based on actual MemoryTransport capabilities
    Arc::new(MemoryTransportImpl)
}

/// Trait to abstract over transport implementations for testing
pub trait TestTransport: Send + Sync {
    /// Returns the name of the transport implementation
    fn name(&self) -> &str;
}

/// Basic memory implementation - this would be replaced with actual MemoryTransport
#[derive(Default)]
pub struct MemoryTransportImpl;

impl TestTransport for MemoryTransportImpl {
    fn name(&self) -> &str {
        "memory"
    }
}

/// Create test envelope for network fabric testing
///
/// This matches patterns found in simulator tests.
pub fn test_envelope() -> TestEnvelope {
    TestEnvelope {
        id: "test-envelope".to_string(),
        data: vec![1, 2, 3, 4],
    }
}

/// Basic test envelope structure
pub struct TestEnvelope {
    /// Envelope identifier
    pub id: String,
    /// Envelope data payload
    pub data: Vec<u8>,
}

/// Create test network fabric configuration
///
/// For integration tests that need network setup.
pub fn test_network_config() -> TestNetworkConfig {
    TestNetworkConfig {
        max_peers: 10,
        timeout_ms: 1000,
    }
}

/// Basic network configuration for testing
pub struct TestNetworkConfig {
    /// Maximum number of peers
    pub max_peers: usize,
    /// Timeout in milliseconds
    pub timeout_ms: u64,
}
