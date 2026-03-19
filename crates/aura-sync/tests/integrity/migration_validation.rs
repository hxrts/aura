//! Migration validation tests for aura-sync refactoring
//!
//! These tests ensure that the new unified core modules (errors, messages, config,
//! metrics, session) provide equivalent or better functionality compared to the
//! scattered patterns they replace. Tests validate API compatibility, performance,
//! and behavioral equivalence during the refactoring process.

use aura_core::time::PhysicalTime;
use aura_core::{AuraError, DeviceId, SessionId};
use aura_sync::core::{
    config::{RetryConfig, SyncConfig},
    errors::{
        sync_authorization_error, sync_biscuit_authorization_error, sync_consistency_error,
        sync_network_error, sync_protocol_error, sync_protocol_with_peer, sync_resource_with_limit,
        sync_serialization_error, sync_session_error, sync_timeout_error, sync_timeout_with_peer,
        sync_validation_field_error,
    },
    messages::{
        BatchMessage, ProgressMessage, RequestMessage, ResponseMessage, SessionMessage,
        SyncResult as MessageSyncResult,
    },
    metrics::{ErrorCategory, MetricsCollector},
    session::{SessionConfig, SessionError, SessionManager, SessionResult, SessionState},
};
use aura_testkit::stateful_effects::random::MockRandomHandler;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

fn device(seed: u8) -> DeviceId {
    DeviceId::new_from_entropy([seed; 32])
}

/// Generate a UUID from random bytes for testing
fn generate_test_uuid() -> Uuid {
    // Use a deterministic approach for testing
    use std::cell::Cell;

    thread_local! {
        static COUNTER: Cell<u64> = const { Cell::new(1) };
    }

    COUNTER.with(|counter| {
        let val = counter.get();
        counter.set(val + 1);
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&val.to_le_bytes());
        Uuid::from_bytes(bytes)
    })
}

/// Create a test PhysicalTime from milliseconds
fn test_time(ts_ms: u64) -> PhysicalTime {
    PhysicalTime {
        ts_ms,
        uncertainty: None,
    }
}

/// Test protocol state for session management tests
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TestSyncProtocolState {
    phase: String,
    operations_pending: usize,
    bytes_transferred: usize,
}

// =============================================================================
// Error System Migration Validation
// =============================================================================

#[test]
fn test_unified_error_hierarchy_compatibility() {
    // Test that new error system provides all error categories needed by existing code
    // NOTE: After simplification to unified AuraError, categories map to core variants:
    // - protocol, session, timeout, resource, consistency -> "internal"
    // - authorization -> "permission_denied"
    // - validation -> "invalid"
    // - network -> "network"
    // - serialization -> "serialization"
    // Retryability: only Network and Storage errors are retryable

    // Network errors (common in sync operations)
    let network_err = sync_network_error("Connection failed");
    assert_eq!(network_err.category(), "network");
    assert!(network_err.is_retryable());
    assert!(network_err.to_string().contains("Network"));

    // Protocol errors (choreographic violations) - maps to internal
    let protocol_err = sync_protocol_error("anti_entropy", "Invalid digest");
    assert_eq!(protocol_err.category(), "internal");
    assert!(!protocol_err.is_retryable());

    // Session errors (state management) - maps to internal
    let session_err = sync_session_error("Invalid state transition");
    assert_eq!(session_err.category(), "internal");
    assert!(!session_err.is_retryable()); // Only Network/Storage are retryable

    // Authorization errors (capability violations) - maps to permission_denied
    let auth_err = sync_authorization_error("Insufficient permissions");
    assert_eq!(auth_err.category(), "permission_denied");
    assert!(!auth_err.is_retryable());

    // Biscuit authorization errors (token-based access control) - maps to permission_denied
    let biscuit_auth_err = sync_biscuit_authorization_error("Token expired", device(1));
    assert_eq!(biscuit_auth_err.category(), "permission_denied");
    assert!(!biscuit_auth_err.is_retryable());

    // Timeout errors (common in distributed systems) - maps to internal
    let timeout_err = sync_timeout_error("journal_sync", Duration::from_secs(30));
    assert_eq!(timeout_err.category(), "internal");
    assert!(!timeout_err.is_retryable()); // Only Network/Storage are retryable

    // Resource exhaustion (memory, bandwidth limits) - maps to internal
    let resource_err = sync_resource_with_limit("memory", "Buffer overflow", 1024);
    assert_eq!(resource_err.category(), "internal");
    assert!(!resource_err.is_retryable()); // Only Network/Storage are retryable

    // Validation errors (data integrity) - maps to invalid
    let validation_err = sync_validation_field_error("Invalid timestamp", "created_at");
    assert_eq!(validation_err.category(), "invalid");
    assert!(!validation_err.is_retryable());

    // Serialization errors (message format issues)
    let ser_err = sync_serialization_error("SyncMessage", "Invalid JSON");
    assert_eq!(ser_err.category(), "serialization");
    assert!(!ser_err.is_retryable());

    // Consistency errors (CRDT violations) - maps to internal
    let consistency_err = sync_consistency_error("journal_merge", "Conflicting operations");
    assert_eq!(consistency_err.category(), "internal");
    assert!(!consistency_err.is_retryable());
}

#[test]
fn test_error_context_preservation() {
    // Test that error context (peer, operation, etc.) is preserved
    let peer_id = device(2);

    // Network errors are retryable (only Network and Storage are retryable)
    let network_err = sync_network_error("Invalid protocol version");
    assert!(network_err.is_retryable());

    let protocol_err = sync_protocol_with_peer("sync", "Message out of order", peer_id);
    assert!(protocol_err.to_string().contains(&peer_id.to_string()));

    let timeout_err = sync_timeout_with_peer("handshake", Duration::from_secs(10), peer_id);
    assert!(timeout_err.to_string().contains("10s"));
    assert!(timeout_err.to_string().contains(&peer_id.to_string()));
}

// =============================================================================
// Message System Migration Validation
// =============================================================================

#[test]
fn test_unified_message_patterns() {
    // Test session-scoped messages
    let session_id = SessionId::new_from_entropy([21u8; 32]);
    let payload = String::from("test data");
    let session_msg = SessionMessage::new(session_id, payload.clone());

    assert_eq!(session_msg.session_id, session_id);
    assert_eq!(session_msg.payload(), &payload);
    assert_eq!(session_msg.into_payload(), payload);

    // Test mapped messages maintain session context
    let mapped = SessionMessage::new(session_id, 42u64).map(|x| x * 2);
    assert_eq!(mapped.session_id, session_id);
    assert_eq!(mapped.payload, 84);
}

#[test]
fn test_request_response_correlation() {
    // Test request/response pattern used throughout sync protocols
    let from = device(3);
    let to = device(4);
    let payload = String::from("ping");

    let request = RequestMessage::new(from, to, payload, generate_test_uuid());
    let response = ResponseMessage::success(&request, String::from("pong"));

    // Verify correlation
    assert_eq!(response.request_id, request.request_id);
    assert_eq!(response.from, to); // Swapped
    assert_eq!(response.to, from); // Swapped
    assert!(response.is_success());
    assert_eq!(response.into_success(), Some(String::from("pong")));

    // Test error response
    let error_response: ResponseMessage<String> =
        ResponseMessage::error(&request, String::from("Service unavailable"));
    assert!(!error_response.is_success());
    assert_eq!(error_response.into_success(), None);
}

#[test]
fn test_sync_result_pattern() {
    // Test the common sync result pattern used across protocols
    let success_result = MessageSyncResult::success(100, Some(String::from("metadata")), 5000);
    assert!(success_result.success);
    assert_eq!(success_result.operations_synced, 100);
    assert_eq!(success_result.duration_ms, 5000);
    assert!(success_result.data.is_some());

    let failure_result: MessageSyncResult<String> =
        MessageSyncResult::failure(String::from("Network timeout"), 3000);
    assert!(!failure_result.success);
    assert_eq!(failure_result.operations_synced, 0);
    assert_eq!(failure_result.duration_ms, 3000);
    assert!(failure_result.error.is_some());

    let partial_result: MessageSyncResult<String> =
        MessageSyncResult::partial(75, String::from("Incomplete transfer"), 8000);
    assert!(!partial_result.success);
    assert_eq!(partial_result.operations_synced, 75);
    assert!(partial_result.error.is_some());
}

#[test]
fn test_batch_message_functionality() {
    // Test batching for large sync operations
    let items = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let batches = BatchMessage::create_batches(items, 3, generate_test_uuid());

    assert_eq!(batches.len(), 4); // 3 + 3 + 3 + 1
    assert_eq!(batches[0].items, vec![1, 2, 3]);
    assert_eq!(batches[1].items, vec![4, 5, 6]);
    assert_eq!(batches[2].items, vec![7, 8, 9]);
    assert_eq!(batches[3].items, vec![10]);
    assert!(batches[3].is_final);

    // All batches should have same ID and total count
    let batch_id = batches[0].batch_id;
    for batch in &batches {
        assert_eq!(batch.batch_id, batch_id);
        assert_eq!(batch.total_items, 10);
    }
}

#[test]
fn test_progress_message_tracking() {
    // Test progress tracking for long-running sync operations
    let operation_id = generate_test_uuid();
    let progress = ProgressMessage::new(operation_id, 0.5, String::from("Processing"))
        .with_eta(300)
        .with_metadata("items", "100");

    assert_eq!(progress.operation_id, operation_id);
    assert_eq!(progress.progress, 0.5);
    assert!(!progress.is_complete());
    assert_eq!(progress.eta_seconds, Some(300));
    assert_eq!(progress.metadata.get("items"), Some(&String::from("100")));

    let complete_progress = ProgressMessage::new(operation_id, 1.0, String::from("Complete"));
    assert!(complete_progress.is_complete());
}

// =============================================================================
// Configuration System Migration Validation
// =============================================================================

#[test]
fn test_unified_configuration_system() {
    // Test that unified config covers all sync protocol needs
    let config = SyncConfig::default();

    // Network configuration
    assert!(config.network.base_sync_interval > Duration::ZERO);
    assert!(config.network.sync_timeout > config.network.min_sync_interval);
    assert!(config.network.cleanup_interval > Duration::ZERO);

    // Retry configuration
    assert!(config.retry.max_retries > 0);
    assert!(config.retry.base_delay > Duration::ZERO);
    assert!(config.retry.max_delay > config.retry.base_delay);
    assert!(config.retry.jitter_factor >= 0.0 && config.retry.jitter_factor <= 1.0);

    // Batch configuration
    assert!(config.batching.default_batch_size > 0);
    assert!(config.batching.max_operations_per_round >= config.batching.default_batch_size);
    assert!(config.batching.min_batch_size <= config.batching.default_batch_size);

    // Peer management
    assert!(config.peer_management.max_concurrent_syncs > 0);
    assert!(config.peer_management.min_priority_threshold > 0);

    // Performance limits
    assert!(config.performance.max_cpu_usage <= 100);
    assert!(config.performance.max_network_bandwidth > 0);
    assert!(config.performance.memory_limit > 0);
}

#[test]
fn test_environment_specific_configs() {
    // Test configs optimized for different environments
    let test_config = SyncConfig::for_testing();
    assert!(test_config.network.base_sync_interval < Duration::from_secs(1)); // Fast for tests
    assert!(test_config.retry.max_retries <= 3); // Quick failure in tests
    assert!(!test_config.batching.enable_compression); // Simple for tests
    assert_eq!(test_config.retry.jitter_factor, 0.0); // Predictable for tests

    let prod_config = SyncConfig::for_production();
    assert!(prod_config.network.base_sync_interval >= Duration::from_secs(30)); // Conservative
    assert!(prod_config.retry.max_retries >= 3); // Resilient in production
    assert!(prod_config.performance.max_cpu_usage <= 80); // Resource conscious
}

#[test]
fn test_config_validation() {
    // Test configuration validation catches invalid values
    let mut config = SyncConfig::default();
    assert!(config.validate().is_ok());

    // Test invalid performance limits
    config.performance.max_cpu_usage = 150;
    assert!(config.validate().is_err());

    config.performance.max_cpu_usage = 80;
    config.retry.jitter_factor = 2.0; // Invalid jitter > 1.0
    assert!(config.validate().is_err());

    config.retry.jitter_factor = 0.1;
    config.network.min_sync_interval = Duration::from_secs(100);
    config.network.base_sync_interval = Duration::from_secs(50); // min > base is invalid
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn test_retry_config_functionality() {
    // Test retry configuration behavior
    let retry_config = RetryConfig::default();
    let random = MockRandomHandler::default();

    // Test exponential backoff
    let delay1 = retry_config.delay_for_attempt(0, &random).await;
    let delay2 = retry_config.delay_for_attempt(1, &random).await;
    let delay3 = retry_config.delay_for_attempt(2, &random).await;

    assert!(delay2 >= delay1); // Should increase
    assert!(delay3 >= delay2); // Should continue increasing

    // Test max delay cap
    let long_delay = retry_config.delay_for_attempt(20, &random).await;
    assert!(long_delay <= retry_config.max_delay);

    // Test retry limit
    for attempt in 0..retry_config.max_retries {
        assert!(retry_config.should_retry(attempt));
    }
    assert!(!retry_config.should_retry(retry_config.max_retries));
}

// =============================================================================
// Metrics System Migration Validation
// =============================================================================

#[test]
fn test_unified_metrics_collection() {
    // Test metrics collection covers all sync operations
    let collector = MetricsCollector::new();

    // Test session lifecycle metrics
    let now = 1000000;
    collector.record_sync_start("test_session_1", now);
    collector.record_sync_completion("test_session_1", 50, 1024, now + 100);

    let snapshot = collector.export_snapshot(now + 100);
    assert_eq!(snapshot.operational.sync_sessions_total, 1);
    assert_eq!(snapshot.operational.sync_sessions_completed_total, 1);
    assert_eq!(snapshot.operational.sync_operations_transferred_total, 50);
    assert_eq!(snapshot.operational.sync_bytes_transferred_total, 1024);
    assert_eq!(snapshot.operational.active_sync_sessions, 0);
    assert_eq!(snapshot.operational.success_rate_percent, 100.0);
}

#[test]
fn test_error_metrics_categorization() {
    // Test error metrics by category
    let collector = MetricsCollector::new();

    collector.record_error(ErrorCategory::Network, "Connection failed");
    collector.record_error(ErrorCategory::Protocol, "Invalid message");
    collector.record_error(ErrorCategory::Timeout, "Operation timeout");
    collector.record_error(ErrorCategory::Validation, "Invalid data");

    let snapshot = collector.export_snapshot(0);
    assert_eq!(snapshot.errors.network_errors_total, 1);
    assert_eq!(snapshot.errors.protocol_errors_total, 1);
    assert_eq!(snapshot.errors.timeout_errors_total, 1);
    assert_eq!(snapshot.errors.validation_errors_total, 1);
    assert_eq!(snapshot.errors.total_errors, 4);
}

#[test]
fn test_performance_metrics() {
    // Test performance metrics collection
    let collector = MetricsCollector::new();
    let peer = device(5);

    collector.record_network_latency(peer, Duration::from_millis(50));
    collector.record_operation_processing_time("journal_merge", Duration::from_micros(1000));
    collector.record_compression_ratio(0.75);

    let snapshot = collector.export_snapshot(0);
    assert!(snapshot.performance.network_latency_stats.count > 0);
    assert!(snapshot.performance.operation_processing_stats.count > 0);
    assert!(snapshot.performance.compression_ratio_stats.count > 0);
}

#[test]
fn test_prometheus_export_format() {
    // Test Prometheus export compatibility
    let collector = MetricsCollector::new();
    let now = 1000000;
    collector.record_sync_start("test", now);
    collector.record_sync_completion("test", 10, 100, now + 50);

    let prometheus_output = collector.export_prometheus();

    // Verify Prometheus format conventions
    assert!(prometheus_output.contains("# HELP"));
    assert!(prometheus_output.contains("# TYPE"));
    assert!(prometheus_output.contains("aura_sync_sessions_total"));
    assert!(prometheus_output.contains("aura_sync_sessions_completed_total"));
    assert!(prometheus_output.contains("counter"));
    assert!(prometheus_output.contains("gauge"));
}

// =============================================================================
// Session Management Migration Validation
// =============================================================================

#[test]
fn test_unified_session_management() {
    // Test session manager handles all sync session patterns

    let config = SessionConfig::default();
    let mut manager = SessionManager::<TestSyncProtocolState>::new(config, test_time(1000000));

    // Test session creation and activation
    let participants = vec![device(6), device(7)];
    let session_id = manager
        .create_session(participants, &test_time(1000001))
        .unwrap();

    let initial_state = TestSyncProtocolState {
        phase: String::from("initialization"),
        operations_pending: 100,
        bytes_transferred: 0,
    };

    manager
        .activate_session(session_id, initial_state, &test_time(1000001))
        .unwrap();
    assert_eq!(manager.count_active_sessions(), 1);

    // Test session state transitions
    let updated_state = TestSyncProtocolState {
        phase: String::from("active"),
        operations_pending: 75,
        bytes_transferred: 1024,
    };

    manager
        .update_session(session_id, updated_state, &test_time(1000002))
        .unwrap();

    // Test successful completion
    let mut metadata = HashMap::new();
    metadata.insert(String::from("sync_type"), String::from("journal"));

    manager
        .complete_session(session_id, 100, 2048, metadata, &test_time(1000003))
        .unwrap();

    // Verify final state
    let session = manager.get_session(&session_id).unwrap();
    match session {
        SessionState::Completed(SessionResult::Success {
            operations_count,
            bytes_transferred,
            ..
        }) => {
            assert_eq!(*operations_count, 100);
            assert_eq!(*bytes_transferred, 2048);
        }
        _ => panic!("Session should be completed successfully"),
    }
}

#[test]
fn test_session_failure_handling() {
    // Test session failure scenarios

    let config = SessionConfig::default();
    let mut manager = SessionManager::<TestSyncProtocolState>::new(config, test_time(1000000));

    let session_id = manager
        .create_session(vec![device(8)], &test_time(1000001))
        .unwrap();
    let state = TestSyncProtocolState {
        phase: String::from("test"),
        operations_pending: 50,
        bytes_transferred: 0,
    };
    manager
        .activate_session(session_id, state, &test_time(1000001))
        .unwrap();

    // Test failure with partial results
    let partial_results = aura_sync::core::session::PartialResults {
        operations_completed: 25,
        bytes_transferred: 512,
        completed_participants: vec![device(9)],
        last_successful_operation: Some(String::from("journal_append")),
    };

    let error = SessionError::ProtocolViolation {
        constraint: String::from("operation ordering"),
    };

    manager
        .fail_session(
            session_id,
            error,
            Some(partial_results),
            &test_time(1000002),
        )
        .unwrap();

    // Verify failure handling
    let session = manager.get_session(&session_id).unwrap();
    match session {
        SessionState::Completed(SessionResult::Failure {
            partial_results: Some(partial),
            ..
        }) => {
            assert_eq!(partial.operations_completed, 25);
            assert_eq!(partial.bytes_transferred, 512);
        }
        _ => panic!("Session should be completed with failure"),
    }
}

#[test]
fn test_session_resource_limits() {
    // Test session resource management

    let config = SessionConfig {
        max_concurrent_sessions: 2,
        max_participants: 3,
        ..SessionConfig::default()
    };
    let mut manager = SessionManager::<TestSyncProtocolState>::new(config, test_time(1000000));

    // Test participant limit
    let too_many_participants = vec![device(10); 5];
    let result = manager.create_session(too_many_participants, &test_time(1000001));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AuraError::Invalid { .. }));

    // Test concurrent session limit
    let state = TestSyncProtocolState {
        phase: String::from("test"),
        operations_pending: 0,
        bytes_transferred: 0,
    };

    let session1 = manager
        .create_session(vec![device(11)], &test_time(1000001))
        .unwrap();
    let session2 = manager
        .create_session(vec![device(12)], &test_time(1000002))
        .unwrap();
    manager
        .activate_session(session1, state.clone(), &test_time(1000001))
        .unwrap();
    manager
        .activate_session(session2, state, &test_time(1000002))
        .unwrap();

    // Third session should exceed limit
    let result = manager.create_session(vec![device(13)], &test_time(1000003));
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), AuraError::Internal { .. }));
}

#[test]
fn test_session_statistics() {
    // Test session statistics collection

    let config = SessionConfig::default();
    let mut manager = SessionManager::<TestSyncProtocolState>::new(config, test_time(1000000));

    // Create various session outcomes
    let state = TestSyncProtocolState {
        phase: String::from("test"),
        operations_pending: 10,
        bytes_transferred: 0,
    };

    // Successful session
    let session1 = manager
        .create_session(vec![device(14)], &test_time(1000001))
        .unwrap();
    manager
        .activate_session(session1, state.clone(), &test_time(1000001))
        .unwrap();
    manager
        .complete_session(session1, 50, 1000, HashMap::new(), &test_time(1000002))
        .unwrap();

    // Failed session
    let session2 = manager
        .create_session(vec![device(15)], &test_time(1000002))
        .unwrap();
    manager
        .activate_session(session2, state, &test_time(1000002))
        .unwrap();
    let error = SessionError::Timeout { duration_ms: 5000 };
    manager
        .fail_session(session2, error, None, &test_time(1000003))
        .unwrap();

    let stats = manager.get_statistics();
    assert_eq!(stats.total_sessions, 2);
    assert_eq!(stats.completed_sessions, 1);
    assert_eq!(stats.failed_sessions, 1);
    assert_eq!(stats.timeout_sessions, 0); // Failed sessions != timeout sessions
    assert_eq!(stats.success_rate_percent, 50.0); // 1 of 2 successful
    assert_eq!(stats.total_operations, 50);
}

// =============================================================================
// Integration Compatibility Tests
// =============================================================================

#[test]
fn test_cross_module_integration() {
    // Test that all core modules work together correctly
    let config = SyncConfig::for_testing();
    let metrics = MetricsCollector::new();

    let session_config = SessionConfig::from(&config);
    let mut session_manager = SessionManager::<TestSyncProtocolState>::with_metrics(
        session_config,
        metrics.clone(),
        test_time(1000000),
    );

    // Perform a complete sync session workflow
    let session_id = session_manager
        .create_session(vec![device(16)], &test_time(1000001))
        .unwrap();

    let state = TestSyncProtocolState {
        phase: String::from("starting"),
        operations_pending: config.batching.default_batch_size as usize,
        bytes_transferred: 0,
    };

    session_manager
        .activate_session(session_id, state, &test_time(1000001))
        .unwrap();

    // Simulate session progress
    let updated_state = TestSyncProtocolState {
        phase: String::from("syncing"),
        operations_pending: 50,
        bytes_transferred: 1024,
    };

    session_manager
        .update_session(session_id, updated_state, &test_time(1000002))
        .unwrap();

    // Complete session
    session_manager
        .complete_session(session_id, 100, 2048, HashMap::new(), &test_time(1000003))
        .unwrap();

    // Verify metrics integration
    let metrics_snapshot = metrics.export_snapshot(0);
    assert!(metrics_snapshot.operational.sync_sessions_total >= 1);
    assert!(metrics_snapshot.operational.sync_sessions_completed_total >= 1);

    // Verify session manager statistics
    let session_stats = session_manager.get_statistics();
    assert!(session_stats.completed_sessions >= 1);
    assert!(session_stats.success_rate_percent > 0.0);
}

#[test]
fn test_error_propagation_consistency() {
    // Test that errors flow correctly between modules
    let config = SyncConfig::default();
    config.validate().unwrap(); // Should not fail

    // Test session errors integrate with sync errors
    let session_error = SessionError::ProtocolViolation {
        constraint: String::from("test"),
    };

    // Verify error can be converted/wrapped appropriately
    // Note: sync_session_error maps to internal (only Network/Storage are retryable)
    let sync_error = sync_session_error(session_error.to_string());
    assert_eq!(sync_error.category(), "internal");
    assert!(!sync_error.is_retryable());
}

#[test]
fn test_backwards_compatibility_surface() {
    // Test that essential functionality remains available
    // This test should pass both before and after refactoring

    // Basic error creation
    let _error = sync_network_error("test");

    // Basic configuration
    let _config = SyncConfig::default();

    // Basic metrics
    let _metrics = MetricsCollector::new();

    // Basic session management

    let _session_manager =
        SessionManager::<TestSyncProtocolState>::new(SessionConfig::default(), test_time(1000000));

    // Basic message patterns
    let _session_msg = SessionMessage::new(SessionId::new_from_entropy([22u8; 32]), "test");
    let _request_msg = RequestMessage::new(device(17), device(18), "ping", generate_test_uuid());

    // If this test compiles and runs, basic API compatibility is maintained
}
