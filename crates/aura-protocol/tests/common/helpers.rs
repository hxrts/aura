//! Test helper utilities for aura-protocol tests
//!
//! This module provides common test utilities to reduce duplication across
//! test modules and standardize test patterns.

use aura_protocol::{
    handlers::CompositeHandler,
    middleware::{MiddlewareConfig, create_standard_stack},
    runtime::{ExecutionContext, ContextBuilder},
    effects::ProtocolEffects,
};
use aura_types::DeviceId;
use uuid::Uuid;

/// Create a test DeviceId
pub fn create_test_device_id() -> DeviceId {
    DeviceId::from(Uuid::from_u128(12345))
}

/// Create a second test DeviceId for multi-device tests
pub fn create_test_device_id_2() -> DeviceId {
    DeviceId::from(Uuid::from_u128(67890))
}

/// Create a test session ID
pub fn create_test_session_id() -> Uuid {
    Uuid::from_u128(99999)
}

/// Create a test execution context for testing
pub fn create_test_execution_context() -> ExecutionContext {
    let device_id = create_test_device_id();
    let session_id = create_test_session_id();
    let participants = vec![device_id, create_test_device_id_2()];
    
    ContextBuilder::new()
        .with_device_id(device_id)
        .with_session_id(session_id)
        .with_participants(participants)
        .with_threshold(2)
        .build_for_testing()
}

/// Create a test execution context for simulation
pub fn create_test_simulation_context() -> ExecutionContext {
    let device_id = create_test_device_id();
    let session_id = create_test_session_id();
    let participants = vec![device_id, create_test_device_id_2()];
    
    ContextBuilder::new()
        .with_device_id(device_id)
        .with_session_id(session_id)
        .with_participants(participants)
        .with_threshold(2)
        .build_for_simulation()
}

/// Create a composite handler for testing
pub fn create_test_handler() -> CompositeHandler {
    CompositeHandler::for_testing(create_test_device_id().into())
}

/// Create a composite handler for simulation
pub fn create_simulation_handler() -> CompositeHandler {
    CompositeHandler::for_simulation(create_test_device_id().into())
}

/// Create a middleware config for testing
pub fn create_test_middleware_config() -> MiddlewareConfig {
    MiddlewareConfig {
        device_name: "test-device".to_string(),
        enable_observability: false, // Disable for cleaner test output
        enable_capabilities: true,
        enable_error_recovery: true,
        observability_config: None,
        error_recovery_config: None,
    }
}

/// Create a handler with middleware for testing
pub fn create_test_handler_with_middleware() -> impl ProtocolEffects {
    let handler = create_test_handler();
    let config = create_test_middleware_config();
    create_standard_stack(handler, config)
}

/// Create deterministic UUIDs for testing
pub fn create_deterministic_uuid(seed: u128) -> Uuid {
    Uuid::from_u128(seed)
}

/// Create a set of participants for multi-party tests
pub fn create_test_participants(count: usize) -> Vec<DeviceId> {
    (0..count)
        .map(|i| DeviceId::from(create_deterministic_uuid(1000 + i as u128)))
        .collect()
}

/// Create test data for protocol operations
pub fn create_test_data(size: usize) -> Vec<u8> {
    (0..size).map(|i| (i % 256) as u8).collect()
}

/// Create test key material for crypto operations
pub fn create_test_key_bytes() -> [u8; 32] {
    [0x42u8; 32] // Deterministic test key
}

/// Helper to run async tests with timeout
#[macro_export]
macro_rules! test_with_timeout {
    ($test:expr) => {
        tokio::time::timeout(std::time::Duration::from_secs(10), $test)
            .await
            .expect("Test timed out")
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_test_device_id() {
        let id1 = create_test_device_id();
        let id2 = create_test_device_id();
        assert_eq!(id1, id2); // Should be deterministic
    }

    #[test]
    fn test_create_test_participants() {
        let participants = create_test_participants(3);
        assert_eq!(participants.len(), 3);
        assert_ne!(participants[0], participants[1]);
        assert_ne!(participants[1], participants[2]);
    }

    #[test]
    fn test_create_test_data() {
        let data = create_test_data(5);
        assert_eq!(data, vec![0, 1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_create_test_execution_context() {
        let context = create_test_execution_context();
        assert_eq!(context.device_id, create_test_device_id());
        assert_eq!(context.session_id, create_test_session_id());
        assert_eq!(context.participant_count(), 2);
        assert!(context.is_simulation);
    }
}