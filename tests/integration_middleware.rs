//! Integration tests for cross-layer middleware composition
//!
//! This module tests the interaction between middleware from different layers
//! to ensure they work together correctly and maintain the expected architectural
//! invariants.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

// Import middleware from different layers
use aura_transport::middleware::{
    TransportMiddlewareStack, RateLimitingMiddleware, CircuitBreakerMiddleware,
    MonitoringMiddleware, CompressionMiddleware, handler::{BaseTransportHandler, NetworkAddress, TransportOperation, TransportResult}
};

use aura_journal::middleware::{
    JournalMiddlewareStack, ValidationMiddleware, AuthorizationMiddleware,
    ObservabilityMiddleware, RetryMiddleware, handler::BaseJournalHandler
};
use aura_journal::{DeviceMetadata, DeviceType, JournalOperation, JournalContext};

use aura_protocol::effects::{DefaultEffects, EffectsBuilder};
use aura_types::{DeviceId, MiddlewareContext};
use aura_crypto::{Ed25519VerifyingKey, AccountId};

/// Test that transport and journal middleware can be composed together
/// in a realistic end-to-end scenario
#[tokio::test]
async fn test_cross_layer_middleware_composition() {
    // Setup test environment with deterministic effects
    let effects = EffectsBuilder::new()
        .with_device_id(DeviceId::new("test-device"))
        .with_simulation_mode()
        .build();

    // === Transport Layer Setup ===
    let local_address = NetworkAddress::Memory("test-node".to_string());
    let transport_handler = BaseTransportHandler::new(local_address);

    let transport_stack = TransportMiddlewareStack::new(Arc::new(transport_handler))
        .with_middleware(Arc::new(RateLimitingMiddleware::new()))
        .with_middleware(Arc::new(CircuitBreakerMiddleware::new()))
        .with_middleware(Arc::new(MonitoringMiddleware::new()))
        .with_middleware(Arc::new(CompressionMiddleware::new()));

    // === Journal Layer Setup ===
    let account_id = AccountId::new_from_device(&effects.device_id(), &effects);
    let group_key = effects.random_bytes(32);
    let group_key_array: [u8; 32] = group_key.try_into().unwrap();
    let public_key = Ed25519VerifyingKey::from_bytes(&group_key_array).unwrap();
    
    let journal_handler = BaseJournalHandler::new(account_id, public_key);
    
    let journal_stack = JournalMiddlewareStack::new(Arc::new(journal_handler))
        .with_middleware(Arc::new(ValidationMiddleware::new()))
        .with_middleware(Arc::new(ObservabilityMiddleware::new()));

    // === Test Cross-Layer Operation ===
    let context = MiddlewareContext {
        operation_name: "cross_layer_test".to_string(),
        metadata: HashMap::new(),
        start_time: effects.current_timestamp(),
    };

    // 1. Test Transport Layer Operation
    let transport_operation = TransportOperation::Send {
        destination: NetworkAddress::Memory("peer-node".to_string()),
        data: b"test message for cross-layer integration".to_vec(),
        metadata: HashMap::from([
            ("operation_type".to_string(), "journal_sync".to_string()),
            ("session_id".to_string(), "test_session_123".to_string()),
        ]),
    };

    let transport_result = transport_stack
        .process(transport_operation, &context, &*effects)
        .await;

    assert!(transport_result.is_ok());
    
    if let Ok(TransportResult::Sent { destination, bytes_sent }) = transport_result {
        assert_eq!(destination, NetworkAddress::Memory("peer-node".to_string()));
        assert!(bytes_sent > 0);
    } else {
        panic!("Unexpected transport result: {:?}", transport_result);
    }

    // 2. Test Journal Layer Operation
    let device_metadata = DeviceMetadata {
        device_id: DeviceId::new("new-device"),
        device_name: "Test Device".to_string(),
        device_type: DeviceType::Native,
        public_key,
        added_at: effects.current_timestamp(),
        last_seen: effects.current_timestamp(),
        dkd_commitment_proofs: std::collections::BTreeMap::new(),
        next_nonce: 1,
        used_nonces: std::collections::BTreeSet::new(),
        key_share_epoch: 1,
    };

    let journal_operation = JournalOperation::AddDevice {
        device: device_metadata,
    };

    let journal_context = JournalContext {
        device_id: effects.device_id(),
        operation_id: "cross_layer_test_op".to_string(),
        session_id: None,
        metadata: HashMap::from([
            ("transport_session".to_string(), "test_session_123".to_string()),
            ("middleware_layer".to_string(), "journal".to_string()),
        ]),
    };

    let journal_result = journal_stack
        .handle(journal_operation, &journal_context);

    assert!(journal_result.is_ok());
    
    let result_json = journal_result.unwrap();
    assert!(result_json.get("success").and_then(|v| v.as_bool()).unwrap_or(false));

    println!("✅ Cross-layer middleware composition test passed!");
}

/// Test middleware error propagation across layers
#[tokio::test]
async fn test_error_propagation_across_layers() {
    let effects = EffectsBuilder::new()
        .with_device_id(DeviceId::new("error-test-device"))
        .with_simulation_mode()
        .build();

    // Create transport stack with aggressive rate limiting
    let local_address = NetworkAddress::Memory("error-test-node".to_string());
    let transport_handler = BaseTransportHandler::new(local_address);

    let rate_config = aura_transport::middleware::RateLimitConfig {
        requests_per_second: 1,  // Very low rate limit
        burst_size: 1,           // Small burst
        window_size_ms: 1000,
    };

    let transport_stack = TransportMiddlewareStack::new(Arc::new(transport_handler))
        .with_middleware(Arc::new(RateLimitingMiddleware::with_config(rate_config)))
        .with_middleware(Arc::new(MonitoringMiddleware::new()));

    let context = MiddlewareContext {
        operation_name: "error_test".to_string(),
        metadata: HashMap::new(),
        start_time: effects.current_timestamp(),
    };

    // Send multiple rapid requests to trigger rate limiting
    for i in 0..3 {
        let operation = TransportOperation::Send {
            destination: NetworkAddress::Memory("target".to_string()),
            data: format!("message {}", i).into_bytes(),
            metadata: HashMap::new(),
        };

        let result = transport_stack.process(operation, &context, &*effects).await;
        
        if i >= 1 {
            // Should hit rate limit
            assert!(result.is_err());
            println!("✅ Rate limiting error correctly propagated for request {}", i);
        }
    }

    println!("✅ Error propagation test passed!");
}

/// Test performance characteristics of middleware stacks
#[tokio::test]
async fn test_middleware_performance_characteristics() {
    let effects = EffectsBuilder::new()
        .with_device_id(DeviceId::new("perf-test-device"))
        .with_simulation_mode()
        .build();

    let start_time = effects.current_timestamp();

    // Create a full middleware stack
    let local_address = NetworkAddress::Memory("perf-test-node".to_string());
    let transport_handler = BaseTransportHandler::new(local_address);

    let transport_stack = TransportMiddlewareStack::new(Arc::new(transport_handler))
        .with_middleware(Arc::new(RateLimitingMiddleware::new()))
        .with_middleware(Arc::new(CircuitBreakerMiddleware::new()))
        .with_middleware(Arc::new(MonitoringMiddleware::new()))
        .with_middleware(Arc::new(CompressionMiddleware::new()));

    let context = MiddlewareContext {
        operation_name: "performance_test".to_string(),
        metadata: HashMap::new(),
        start_time,
    };

    // Process multiple operations and measure timing
    const NUM_OPERATIONS: usize = 100;
    let mut total_duration = 0u64;

    for i in 0..NUM_OPERATIONS {
        let op_start = effects.current_timestamp();
        
        let operation = TransportOperation::Send {
            destination: NetworkAddress::Memory("peer".to_string()),
            data: vec![0u8; 1024], // 1KB payload
            metadata: HashMap::new(),
        };

        let result = transport_stack.process(operation, &context, &*effects).await;
        assert!(result.is_ok());

        let op_end = effects.current_timestamp();
        total_duration += op_end - op_start;

        if i % 10 == 0 {
            println!("Processed {} operations...", i);
        }
    }

    let avg_duration = total_duration / NUM_OPERATIONS as u64;
    println!("✅ Average operation duration: {}ms", avg_duration);
    
    // Ensure performance is reasonable (this is a simulation, so expect low latency)
    assert!(avg_duration < 1000, "Average duration too high: {}ms", avg_duration);

    println!("✅ Performance characteristics test passed!");
}

/// Test middleware configuration and observability
#[tokio::test]
async fn test_middleware_observability() {
    let effects = EffectsBuilder::new()
        .with_device_id(DeviceId::new("obs-test-device"))
        .with_simulation_mode()
        .build();

    // Create transport stack with monitoring enabled
    let local_address = NetworkAddress::Memory("obs-test-node".to_string());
    let transport_handler = BaseTransportHandler::new(local_address);

    let monitoring_config = aura_transport::middleware::MonitoringConfig {
        enable_metrics: true,
        enable_tracing: true,
        sample_rate: 1.0,
        metrics_interval_ms: 1000,
        max_operation_history: 100,
    };

    let monitoring_middleware = Arc::new(
        MonitoringMiddleware::with_config(monitoring_config)
    );

    let transport_stack = TransportMiddlewareStack::new(Arc::new(transport_handler))
        .with_middleware(monitoring_middleware.clone())
        .with_middleware(Arc::new(CompressionMiddleware::new()));

    let context = MiddlewareContext {
        operation_name: "observability_test".to_string(),
        metadata: HashMap::new(),
        start_time: effects.current_timestamp(),
    };

    // Perform some operations
    for i in 0..5 {
        let operation = TransportOperation::Send {
            destination: NetworkAddress::Memory(format!("peer-{}", i)),
            data: format!("observation test message {}", i).into_bytes(),
            metadata: HashMap::from([
                ("test_id".to_string(), i.to_string()),
            ]),
        };

        let result = transport_stack.process(operation, &context, &*effects).await;
        assert!(result.is_ok());
    }

    // Check middleware information
    let middleware_info = monitoring_middleware.middleware_info();
    assert!(middleware_info.contains_key("total_operations"));
    assert!(middleware_info.contains_key("successful_operations"));
    assert!(middleware_info.contains_key("enable_metrics"));

    println!("📊 Middleware info: {:?}", middleware_info);
    println!("✅ Observability test passed!");
}

/// Test middleware layer isolation and communication
#[tokio::test]
async fn test_middleware_layer_isolation() {
    let effects = EffectsBuilder::new()
        .with_device_id(DeviceId::new("isolation-test-device"))
        .with_simulation_mode()
        .build();

    // Create separate middleware stacks for different concerns
    let local_address = NetworkAddress::Memory("isolation-test-node".to_string());
    let transport_handler = BaseTransportHandler::new(local_address);

    // Transport stack focused on networking concerns
    let transport_stack = TransportMiddlewareStack::new(Arc::new(transport_handler))
        .with_middleware(Arc::new(RateLimitingMiddleware::new()))
        .with_middleware(Arc::new(CircuitBreakerMiddleware::new()));

    // Journal stack focused on data concerns  
    let account_id = AccountId::new_from_device(&effects.device_id(), &effects);
    let group_key = effects.random_bytes(32);
    let group_key_array: [u8; 32] = group_key.try_into().unwrap();
    let public_key = Ed25519VerifyingKey::from_bytes(&group_key_array).unwrap();
    let journal_handler = BaseJournalHandler::new(account_id, public_key);
    
    let journal_stack = JournalMiddlewareStack::new(Arc::new(journal_handler))
        .with_middleware(Arc::new(ValidationMiddleware::new()))
        .with_middleware(Arc::new(ObservabilityMiddleware::new()));

    let context = MiddlewareContext {
        operation_name: "isolation_test".to_string(),
        metadata: HashMap::new(),
        start_time: effects.current_timestamp(),
    };

    // Test that each stack operates independently
    let transport_operation = TransportOperation::Status { address: None };
    let transport_result = transport_stack.process(transport_operation, &context, &*effects).await;
    assert!(transport_result.is_ok());

    let journal_context = JournalContext {
        device_id: effects.device_id(),
        operation_id: "isolation_test_op".to_string(),
        session_id: None,
        metadata: HashMap::new(),
    };

    let journal_operation = JournalOperation::GetEpoch;
    let journal_result = journal_stack.handle(journal_operation, &journal_context);
    assert!(journal_result.is_ok());

    println!("✅ Middleware layer isolation test passed!");
}

/// Helper function to create test effects with a specific device
fn create_test_effects(device_name: &str) -> DefaultEffects {
    EffectsBuilder::new()
        .with_device_id(DeviceId::new(device_name))
        .with_simulation_mode()
        .build()
}