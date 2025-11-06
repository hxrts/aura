//! Tests for individual effect traits and their implementations

#![allow(clippy::disallowed_methods)]

mod common;

// Note: Effects are now accessed through the unified handler interface
use aura_protocol::handlers::{AuraContext, AuraHandlerFactory, EffectType, HandlerUtils};
use aura_types::identifiers::DeviceId;
use uuid::Uuid;

/// Test unified handler interface for crypto effects
#[tokio::test]
async fn test_crypto_effects() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut handler = AuraHandlerFactory::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test that crypto effects are properly routed through the unified interface
    // Note: Current implementation is a stub that returns UnsupportedEffect

    // Test random bytes effect
    let result: Result<Vec<u8>, _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Random,
        "bytes",
        32u32,
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Test crypto hash effect
    let data = b"test data for hashing";
    let result: Result<Vec<u8>, _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Crypto,
        "blake3_hash",
        data,
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Verify handler basic functionality works
    assert_eq!(
        handler.execution_mode(),
        aura_types::handlers::ExecutionMode::Testing
    );
    assert!(!handler.supports_effect(EffectType::Crypto)); // Stub returns false
}

/// Test unified handler interface for network effects
#[tokio::test]
async fn test_network_effects() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut handler = AuraHandlerFactory::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test network effects through unified interface
    // Note: Current implementation is a stub that returns UnsupportedEffect

    // Test connected_peers effect
    let result: Result<Vec<Uuid>, _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Network,
        "connected_peers",
        (),
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Test send_to_peer effect
    let peer_id = Uuid::new_v4();
    let message = b"test message".to_vec();
    let result: Result<(), _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Network,
        "send_to_peer",
        (peer_id, message),
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Verify handler basic functionality
    assert!(!handler.supports_effect(EffectType::Network)); // Stub returns false
}

/// Test unified handler interface for storage effects
#[tokio::test]
async fn test_storage_effects() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut handler = AuraHandlerFactory::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test storage effects through unified interface
    // Note: Current implementation is a stub that returns UnsupportedEffect

    let key = "test_key";
    let value = b"test value data".to_vec();

    // Test store effect
    let result: Result<(), _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Storage,
        "store",
        (key, &value),
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Test retrieve effect
    let result: Result<Option<Vec<u8>>, _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Storage,
        "retrieve",
        key,
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Verify handler basic functionality
    assert!(!handler.supports_effect(EffectType::Storage)); // Stub returns false
}

/// Test unified handler interface for time effects
#[tokio::test]
async fn test_time_effects() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut handler = AuraHandlerFactory::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test time effects through unified interface
    // Note: Current implementation is a stub that returns UnsupportedEffect

    // Test current_epoch effect
    let result: Result<u64, _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Time,
        "current_epoch",
        (),
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Test set_timeout effect
    let result: Result<u64, _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Time,
        "set_timeout",
        100u64,
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Verify handler basic functionality
    assert!(!handler.supports_effect(EffectType::Time)); // Stub returns false
    assert_eq!(
        handler.execution_mode(),
        aura_types::handlers::ExecutionMode::Testing
    );
}

/// Test unified handler interface for console effects
#[tokio::test]
async fn test_console_effects() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let mut handler = AuraHandlerFactory::for_testing(device_id);
    let mut ctx = AuraContext::for_testing(device_id);

    // Test console effects through unified interface
    // Note: Current implementation is a stub that returns UnsupportedEffect

    let protocol_id = Uuid::new_v4();

    // Test protocol_started effect
    let result: Result<(), _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Console,
        "protocol_started",
        (protocol_id, "test_protocol"),
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Test log_info effect
    let result: Result<(), _> = HandlerUtils::execute_typed_effect(
        &mut *handler,
        EffectType::Console,
        "log_info",
        "Test info message",
        &mut ctx,
    )
    .await;

    // Current stub implementation returns UnsupportedEffect, which is expected
    assert!(result.is_err());

    // Verify handler basic functionality
    assert!(!handler.supports_effect(EffectType::Console)); // Stub returns false
}

/// Test unified handler basic functionality
#[tokio::test]
async fn test_unified_handler_functionality() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let handler = AuraHandlerFactory::for_testing(device_id);

    // Test basic handler properties
    assert_eq!(
        handler.execution_mode(),
        aura_types::handlers::ExecutionMode::Testing
    );
    assert!(handler.execution_mode().is_deterministic());
    assert!(!handler.execution_mode().is_production());

    // Test supported effects (stub implementation returns false for all)
    let supported_effects = handler.supported_effects();
    // In current stub implementation, no effects are supported
    assert!(
        supported_effects.is_empty()
            || supported_effects
                .iter()
                .all(|&effect| !handler.supports_effect(effect))
    );
}
