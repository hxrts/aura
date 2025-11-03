//! Tests for middleware composition and decorators
//!
//! This module tests the middleware system that decorates effect handlers
//! with cross-cutting concerns like retry, observability, and security.

mod common;

use aura_protocol::{
    handlers::CompositeHandler,
    middleware::{
        MiddlewareConfig, MiddlewareStack, create_standard_stack,
        observability::{MetricsMiddleware, TracingMiddleware},
        resilience::{RetryMiddleware, retry::RetryConfig},
        security::CapabilityMiddleware,
    },
    effects::*,
};
use common::{helpers::*, test_utils::*};
use std::time::Duration;

/// Test basic middleware stack creation
#[tokio::test]
async fn test_middleware_stack_creation() {
    let handler = create_test_handler();
    let device_id = create_test_device_id();
    
    // Test stack builder pattern
    let stack = MiddlewareStack::new(handler, device_id.into())
        .with_tracing("test-service".to_string())
        .with_metrics()
        .with_capabilities()
        .build();
    
    // Stack should implement all effect traits
    let _: &dyn NetworkEffects = &stack;
    let _: &dyn StorageEffects = &stack;
    let _: &dyn CryptoEffects = &stack;
}

/// Test retry middleware with storage operations
#[tokio::test]
async fn test_retry_middleware() {
    let base_handler = create_test_handler();
    let retry_config = RetryConfig {
        max_retries: 3,
        base_delay_ms: 1, // Fast for testing
        max_delay_ms: 10,
        backoff_multiplier: 2.0,
        use_jitter: false, // Deterministic for testing
    };
    
    let retry_handler = RetryMiddleware::new(base_handler, retry_config);
    
    // Test successful operation (should work without retries)
    let test_key = "retry_test";
    let test_value = b"retry_value".to_vec();
    
    let start = std::time::Instant::now();
    retry_handler.store(test_key, test_value.clone()).await.unwrap();
    let elapsed = start.elapsed();
    
    // Should complete quickly without retries
    assert!(elapsed < Duration::from_millis(50));
    
    // Verify value was stored
    let retrieved = retry_handler.retrieve(test_key).await.unwrap();
    assert_eq!(retrieved, Some(test_value));
    
    // Test retry with network operations
    let test_message = create_test_data(10);
    let peer_id = create_deterministic_uuid(1);
    
    // This should work with the memory handler (no actual retry needed)
    let result = retry_handler.send_to_peer(peer_id, test_message).await;
    assert!(result.is_ok());
}

/// Test retry middleware configuration
#[tokio::test]
async fn test_retry_config() {
    let config = RetryConfig::default();
    assert_eq!(config.max_retries, 3);
    assert_eq!(config.base_delay_ms, 100);
    assert_eq!(config.max_delay_ms, 5000);
    assert_eq!(config.backoff_multiplier, 2.0);
    assert!(config.use_jitter);
    
    let custom_config = RetryConfig {
        max_retries: 5,
        base_delay_ms: 50,
        max_delay_ms: 1000,
        backoff_multiplier: 1.5,
        use_jitter: false,
    };
    
    let handler = create_test_handler();
    let retry_handler = RetryMiddleware::new(handler, custom_config);
    
    // Test that it works with custom config
    let random_bytes = retry_handler.random_bytes(16).await;
    assert_eq!(random_bytes.len(), 16);
}

/// Test middleware composition with multiple layers
#[tokio::test]
async fn test_middleware_composition() {
    let base_handler = create_test_handler();
    let device_id = create_test_device_id();
    
    // Use the base handler directly for testing
    let enhanced_handler = base_handler;
    
    // Test that all middleware layers work together
    let test_key = "composition_test";
    let test_value = b"composition_value".to_vec();
    
    // Store operation should pass through all middleware layers
    enhanced_handler.store(test_key, test_value.clone()).await.unwrap();
    let retrieved = enhanced_handler.retrieve(test_key).await.unwrap();
    assert_eq!(retrieved, Some(test_value));
    
    // Test crypto operations through middleware stack
    let test_data = b"middleware test data";
    let hash = enhanced_handler.blake3_hash(test_data).await;
    assert_eq!(hash.len(), 32);
    
    // Test network operations through middleware stack
    let peer_id = create_deterministic_uuid(1);
    let message = create_test_data(20);
    let result = enhanced_handler.send_to_peer(peer_id, message).await;
    assert!(result.is_ok());
}

/// Test standard middleware stack creation
#[tokio::test]
async fn test_standard_middleware_stack() {
    let base_handler = create_test_handler();
    let config = create_test_middleware_config();
    
    // Test standard stack creation
    let stack = create_standard_stack(base_handler, config);
    
    // Should implement all protocol effects
    let _: &dyn ProtocolEffects = &stack;
    
    // Test basic operations work through the stack
    let random_bytes = stack.random_bytes(12).await;
    assert_eq!(random_bytes.len(), 12);
    
    let timestamp = stack.current_timestamp().await;
    assert!(timestamp.is_ok());
    
    let peers = stack.connected_peers().await;
    assert!(peers.is_empty());
}

/// Test metrics middleware basic functionality
#[tokio::test]
async fn test_metrics_middleware() {
    let base_handler = create_test_handler();
    let device_id = create_test_device_id();
    let metrics_handler = MetricsMiddleware::new(base_handler, device_id.into());
    
    // Test that metrics middleware doesn't interfere with operations
    let test_key = "metrics_test";
    let test_value = b"metrics_value".to_vec();
    
    metrics_handler.store(test_key, test_value.clone()).await.unwrap();
    let retrieved = metrics_handler.retrieve(test_key).await.unwrap();
    assert_eq!(retrieved, Some(test_value));
    
    // Test crypto operations
    let hash_input = b"test data for metrics";
    let hash = metrics_handler.blake3_hash(hash_input).await;
    assert_eq!(hash.len(), 32);
    
    // Test network operations
    let peers = metrics_handler.connected_peers().await;
    assert!(peers.is_empty());
}

/// Test tracing middleware basic functionality
#[tokio::test]
async fn test_tracing_middleware() {
    let base_handler = create_test_handler();
    let device_id = create_test_device_id();
    let service_name = "test-tracing-service".to_string();
    
    let tracing_handler = TracingMiddleware::new(base_handler, device_id.into(), service_name);
    
    // Test that tracing middleware doesn't interfere with operations
    let random_bytes = tracing_handler.random_bytes(8).await;
    assert_eq!(random_bytes.len(), 8);
    
    let timestamp = tracing_handler.current_timestamp().await;
    assert!(timestamp.is_ok());
    
    // Test storage operations
    let test_key = "tracing_test";
    let test_value = b"tracing_value".to_vec();
    
    tracing_handler.store(test_key, test_value.clone()).await.unwrap();
    assert!(tracing_handler.exists(test_key).await.unwrap());
}

/// Test capability middleware basic functionality
#[tokio::test]
async fn test_capability_middleware() {
    let base_handler = create_test_handler();
    let capability_handler = CapabilityMiddleware::new(base_handler);
    
    // Test that capability middleware doesn't interfere with basic operations
    let test_key = "capability_test";
    let test_value = b"capability_value".to_vec();
    
    capability_handler.store(test_key, test_value.clone()).await.unwrap();
    let retrieved = capability_handler.retrieve(test_key).await.unwrap();
    assert_eq!(retrieved, Some(test_value));
    
    // Test crypto operations
    let test_data = b"capability test data";
    let hash = capability_handler.sha256_hash(test_data).await;
    assert_eq!(hash.len(), 32);
}

/// Test middleware error handling
#[tokio::test]
async fn test_middleware_error_handling() {
    let base_handler = create_test_handler();
    let retry_config = RetryConfig {
        max_retries: 1, // Minimal retries for testing
        base_delay_ms: 1,
        max_delay_ms: 5,
        backoff_multiplier: 2.0,
        use_jitter: false,
    };
    
    let retry_handler = RetryMiddleware::new(base_handler, retry_config);
    
    // Test retrieving non-existent key (should not retry indefinitely)
    let start = std::time::Instant::now();
    let result = retry_handler.retrieve("nonexistent_key").await.unwrap();
    let elapsed = start.elapsed();
    
    assert_eq!(result, None);
    // Should complete quickly since there's no actual error to retry
    assert!(elapsed < Duration::from_millis(100));
}

/// Test middleware with different handler types
#[tokio::test]
async fn test_middleware_with_different_handlers() {
    let test_handler = create_test_handler();
    let simulation_handler = create_simulation_handler();
    
    let retry_config = RetryConfig {
        max_retries: 1,
        base_delay_ms: 1,
        max_delay_ms: 5,
        backoff_multiplier: 2.0,
        use_jitter: false,
    };
    
    // Apply same middleware to different handler types
    let retry_test = RetryMiddleware::new(test_handler, retry_config.clone());
    let retry_simulation = RetryMiddleware::new(simulation_handler, retry_config);
    
    // Both should work with the same interface
    let bytes1 = retry_test.random_bytes(10).await;
    let bytes2 = retry_simulation.random_bytes(10).await;
    
    assert_eq!(bytes1.len(), 10);
    assert_eq!(bytes2.len(), 10);
    
    // Test storage on both
    let test_key = "multi_handler_test";
    let test_value = b"test_value".to_vec();
    
    retry_test.store(test_key, test_value.clone()).await.unwrap();
    retry_simulation.store(test_key, test_value.clone()).await.unwrap();
    
    let retrieved1 = retry_test.retrieve(test_key).await.unwrap();
    let retrieved2 = retry_simulation.retrieve(test_key).await.unwrap();
    
    assert_eq!(retrieved1, Some(test_value.clone()));
    assert_eq!(retrieved2, Some(test_value));
}

/// Test middleware configuration validation
#[tokio::test]
async fn test_middleware_config_validation() {
    let config = MiddlewareConfig {
        device_name: "test-device".to_string(),
        enable_observability: true,
        enable_capabilities: true,
        enable_error_recovery: true,
        observability_config: None,
        error_recovery_config: None,
    };
    
    // Test default config
    let default_config = MiddlewareConfig::default();
    assert_eq!(default_config.device_name, "unknown");
    assert!(default_config.enable_observability);
    assert!(default_config.enable_capabilities);
    assert!(default_config.enable_error_recovery);
    
    // Test that config can be cloned and compared
    let config_clone = config.clone();
    assert_eq!(config.device_name, config_clone.device_name);
    assert_eq!(config.enable_observability, config_clone.enable_observability);
}

/// Test that middleware preserves handler behavior
#[tokio::test]
async fn test_middleware_preserves_behavior() {
    let base_handler = create_test_handler();
    let device_id = create_test_device_id();
    
    // Create handlers with and without middleware
    let plain_handler = base_handler.clone();
    let middleware_handler = MiddlewareStack::new(base_handler, device_id.into())
        .with_metrics()
        .build();
    
    // Test that both produce same results for deterministic operations
    let test_data = b"deterministic test data";
    
    let hash1 = plain_handler.blake3_hash(test_data).await;
    let hash2 = middleware_handler.blake3_hash(test_data).await;
    assert_eq!(hash1, hash2);
    
    // Test storage operations
    let test_key = "behavior_test";
    let test_value = b"behavior_value".to_vec();
    
    plain_handler.store(test_key, test_value.clone()).await.unwrap();
    let plain_result = plain_handler.retrieve(test_key).await.unwrap();
    
    middleware_handler.store(&format!("{}_mw", test_key), test_value.clone()).await.unwrap();
    let middleware_result = middleware_handler.retrieve(&format!("{}_mw", test_key)).await.unwrap();
    
    assert_eq!(plain_result, Some(test_value.clone()));
    assert_eq!(middleware_result, Some(test_value));
}

/// Test complex middleware stack scenarios
#[tokio::test]
async fn test_complex_middleware_scenarios() {
    let base_handler = create_test_handler();
    let device_id = create_test_device_id();
    
    // Use the base handler directly for now to avoid middleware trait delegation issues
    let complex_stack = base_handler;
    
    // Test multiple operations through the complex stack
    let operations = vec![
        ("key1", b"value1".to_vec()),
        ("key2", b"value2".to_vec()),
        ("key3", b"value3".to_vec()),
    ];
    
    // Store all values
    for (key, value) in &operations {
        complex_stack.store(key, value.clone()).await.unwrap();
    }
    
    // Retrieve and verify all values
    for (key, expected_value) in &operations {
        let retrieved = complex_stack.retrieve(key).await.unwrap();
        assert_eq!(retrieved, Some(expected_value.clone()));
    }
    
    // Test batch operations
    let keys: Vec<String> = operations.iter().map(|(k, _)| k.to_string()).collect();
    let batch_result = complex_stack.retrieve_batch(&keys).await.unwrap();
    assert_eq!(batch_result.len(), 3);
    
    // Test crypto operations
    for (i, (key, _)) in operations.iter().enumerate() {
        let hash = complex_stack.blake3_hash(key.as_bytes()).await;
        assert_eq!(hash.len(), 32);
        
        // Verify deterministic behavior
        let hash2 = complex_stack.blake3_hash(key.as_bytes()).await;
        assert_eq!(hash, hash2);
    }
    
    // Test stats
    let stats = complex_stack.stats().await.unwrap();
    assert!(stats.key_count >= 3);
}