//! Test Protocol Utilities
//!
//! Factory functions for creating test protocol contexts and related structures.
//! This addresses the complex protocol setup patterns found in test files.

use std::collections::BTreeMap;
use uuid::Uuid;

/// Create test protocol configuration
///
/// Basic protocol setup for testing.
pub fn test_protocol_config() -> TestProtocolConfig {
    TestProtocolConfig {
        session_id: test_session_id(),
        threshold: Some(2),
        timeout_ms: 5000,
    }
}

/// Basic protocol configuration for testing
pub struct TestProtocolConfig {
    /// Session identifier
    pub session_id: Uuid,
    /// Threshold for protocol operations
    pub threshold: Option<u16>,
    /// Timeout in milliseconds
    pub timeout_ms: u64,
}

/// Create deterministic test session ID
///
/// For tests that need predictable session IDs.
pub fn test_session_id() -> Uuid {
    Uuid::from_bytes([1u8; 16])
}

/// Create deterministic test device ID
///
/// For tests that need predictable device IDs.
pub fn test_device_id() -> Uuid {
    Uuid::from_bytes([2u8; 16])
}

/// Create multiple test device IDs
///
/// Creates sequential device IDs for multi-device tests.
///
/// # Arguments
/// * `count` - Number of device IDs to create
pub fn test_device_ids(count: usize) -> Vec<Uuid> {
    (0..count)
        .map(|i| {
            let mut bytes = [0u8; 16];
            bytes[0] = (i + 1) as u8;
            Uuid::from_bytes(bytes)
        })
        .collect()
}

/// Create test participants list
///
/// Standard pattern for creating participant lists in protocol tests.
///
/// # Arguments
/// * `count` - Number of participants
pub fn test_participants(count: usize) -> Vec<TestParticipant> {
    test_device_ids(count)
        .into_iter()
        .enumerate()
        .map(|(i, device_id)| TestParticipant {
            device_id,
            name: format!("Participant {}", i + 1),
        })
        .collect()
}

/// Test participant structure
pub struct TestParticipant {
    /// Device identifier
    pub device_id: Uuid,
    /// Participant name
    pub name: String,
}

/// Create test context parameters
///
/// This consolidates the parameter setup patterns found in protocol tests.
pub fn test_context_params() -> TestContextParams {
    TestContextParams {
        session_id: test_session_id(),
        device_id: test_device_id(),
        participants: test_device_ids(3),
        threshold: Some(2),
        metadata: test_context_metadata(),
    }
}

/// Test context parameters structure
pub struct TestContextParams {
    /// Session identifier
    pub session_id: Uuid,
    /// Device identifier
    pub device_id: Uuid,
    /// List of participant identifiers
    pub participants: Vec<Uuid>,
    /// Threshold for operations
    pub threshold: Option<u16>,
    /// Additional metadata
    pub metadata: BTreeMap<String, String>,
}

/// Create test context metadata
///
/// Standard metadata for protocol contexts.
pub fn test_context_metadata() -> BTreeMap<String, String> {
    let mut metadata = BTreeMap::new();
    metadata.insert("test_name".to_string(), "protocol_test".to_string());
    metadata.insert("version".to_string(), "1.0".to_string());
    metadata
}

/// Create test DKD context ID
///
/// For DKD protocol tests that need a context identifier.
pub fn test_dkd_context_id() -> Vec<u8> {
    vec![1, 2, 3, 4, 5, 6, 7, 8]
}

/// Create test operation type
///
/// For protocol tests that need operation identifiers.
pub fn test_operation_type() -> String {
    "test_operation".to_string()
}
