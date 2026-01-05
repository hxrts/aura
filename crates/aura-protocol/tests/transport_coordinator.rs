//! Transport Coordinator Integration Tests
//!
//! Tests for multi-party connection management and retry logic.

use aura_effects::transport::{NonZeroDuration, TransportConfig};
use aura_protocol::handlers::transport_coordinator::{
    CoordinationResult, RetryingTransportManager, TransportCoordinationConfig,
    TransportCoordinationError,
};
use std::num::NonZeroUsize;
use std::time::Duration;

// ============================================================================
// TransportCoordinationConfig Tests
// ============================================================================

#[test]
fn default_config_has_reasonable_values() {
    let config = TransportCoordinationConfig::default();

    assert!(
        config.max_connections > 0,
        "Should allow at least one connection"
    );
    assert!(
        config.max_connections <= 1000,
        "Should have reasonable max connections"
    );
    assert!(
        config.connection_timeout > Duration::ZERO,
        "Timeout should be positive"
    );
    assert!(config.max_retries > 0, "Should have at least one retry");
    assert!(
        !config.default_capabilities.is_empty(),
        "Should have default capabilities"
    );
}

#[test]
fn custom_config_construction() {
    let config = TransportCoordinationConfig {
        max_connections: 50,
        connection_timeout: Duration::from_secs(10),
        max_retries: 5,
        default_capabilities: vec!["secure".to_string(), "encrypted".to_string()],
    };

    assert_eq!(config.max_connections, 50);
    assert_eq!(config.connection_timeout, Duration::from_secs(10));
    assert_eq!(config.max_retries, 5);
    assert_eq!(config.default_capabilities.len(), 2);
}

#[test]
fn config_is_clone() {
    let config = TransportCoordinationConfig::default();
    let cloned = config.clone();

    assert_eq!(config.max_connections, cloned.max_connections);
    assert_eq!(config.max_retries, cloned.max_retries);
}

// ============================================================================
// TransportCoordinationError Tests
// ============================================================================

#[test]
fn error_codes_are_unique() {
    use aura_core::ProtocolErrorCode;

    let errors: Vec<TransportCoordinationError> = vec![
        TransportCoordinationError::ProtocolFailed("test".to_string()),
        TransportCoordinationError::CapabilityCheckFailed("test".to_string()),
        TransportCoordinationError::FlowBudgetExceeded("test".to_string()),
        TransportCoordinationError::Effect("test".to_string()),
    ];

    let codes: std::collections::HashSet<_> = errors.iter().map(|e| e.code()).collect();

    // Check that we have distinct codes for distinct error types
    assert!(codes.len() >= 4, "Error codes should be distinct");
}

#[test]
fn protocol_failed_error_message() {
    let err = TransportCoordinationError::ProtocolFailed("handshake timeout".to_string());
    let msg = format!("{}", err);

    assert!(msg.contains("Protocol execution failed"));
    assert!(msg.contains("handshake timeout"));
}

#[test]
fn capability_check_failed_error_message() {
    let err = TransportCoordinationError::CapabilityCheckFailed("missing encryption".to_string());
    let msg = format!("{}", err);

    assert!(msg.contains("Capability check failed"));
    assert!(msg.contains("missing encryption"));
}

#[test]
fn flow_budget_exceeded_error_message() {
    let err = TransportCoordinationError::FlowBudgetExceeded("daily limit reached".to_string());
    let msg = format!("{}", err);

    assert!(msg.contains("Flow budget exceeded"));
    assert!(msg.contains("daily limit reached"));
}

#[test]
fn effect_error_message() {
    let err = TransportCoordinationError::Effect("storage unavailable".to_string());
    let msg = format!("{}", err);

    assert!(msg.contains("Effect error"));
    assert!(msg.contains("storage unavailable"));
}

// ============================================================================
// RetryingTransportManager Tests
// ============================================================================

#[test]
fn retrying_manager_construction() {
    let config = TransportConfig {
        connect_timeout: NonZeroDuration::from_secs(5).expect("non-zero"),
        read_timeout: NonZeroDuration::from_secs(30).expect("non-zero"),
        write_timeout: NonZeroDuration::from_secs(30).expect("non-zero"),
        buffer_size: NonZeroUsize::new(8192).expect("non-zero"),
    };

    let manager = RetryingTransportManager::new(config, 3);

    // Manager should be creatable with valid config
    assert!(std::mem::size_of_val(&manager) > 0);
}

#[test]
fn retrying_manager_is_clone() {
    let config = TransportConfig {
        connect_timeout: NonZeroDuration::from_secs(10).expect("non-zero"),
        read_timeout: NonZeroDuration::from_secs(30).expect("non-zero"),
        write_timeout: NonZeroDuration::from_secs(30).expect("non-zero"),
        buffer_size: NonZeroUsize::new(4096).expect("non-zero"),
    };

    let manager = RetryingTransportManager::new(config, 5);
    let _cloned = manager.clone();

    // Should compile and clone successfully
}

// ============================================================================
// Configuration Validation Tests
// ============================================================================

#[test]
fn zero_max_connections_is_valid_but_useless() {
    // This is a design choice - we allow the config but it means no connections
    let config = TransportCoordinationConfig {
        max_connections: 0,
        ..Default::default()
    };

    assert_eq!(config.max_connections, 0);
}

#[test]
fn large_retry_count_is_allowed() {
    let config = TransportCoordinationConfig {
        max_retries: 100,
        ..Default::default()
    };

    assert_eq!(config.max_retries, 100);
}

#[test]
fn empty_capabilities_is_allowed() {
    let config = TransportCoordinationConfig {
        default_capabilities: vec![],
        ..Default::default()
    };

    assert!(config.default_capabilities.is_empty());
}

// ============================================================================
// Error Conversion Tests
// ============================================================================

#[test]
fn transport_error_converts_to_coordination_error() {
    use aura_effects::transport::TransportError;

    let transport_err = TransportError::ConnectionFailed("peer unreachable".to_string());
    let coord_err: TransportCoordinationError = transport_err.into();

    assert!(matches!(
        coord_err,
        TransportCoordinationError::Transport(_)
    ));
}

// ============================================================================
// CoordinationResult Type Tests
// ============================================================================

#[test]
fn coordination_result_ok() {
    let result: CoordinationResult<u32> = Ok(42);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 42);
}

#[test]
fn coordination_result_err() {
    let result: CoordinationResult<u32> = Err(TransportCoordinationError::ProtocolFailed(
        "test".to_string(),
    ));

    assert!(result.is_err());
}

// ============================================================================
// Config Debug/Clone Tests
// ============================================================================

#[test]
fn config_debug_format() {
    let config = TransportCoordinationConfig::default();
    let debug = format!("{:?}", config);

    assert!(debug.contains("TransportCoordinationConfig"));
    assert!(debug.contains("max_connections"));
}

#[test]
fn error_debug_format() {
    let err = TransportCoordinationError::ProtocolFailed("test failure".to_string());
    let debug = format!("{:?}", err);

    assert!(debug.contains("ProtocolFailed"));
    assert!(debug.contains("test failure"));
}
