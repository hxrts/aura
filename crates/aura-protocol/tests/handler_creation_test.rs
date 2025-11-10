//! Test basic handler creation and functionality
//!
//! Minimal tests to verify that handlers can be created and basic operations work

#![allow(clippy::disallowed_methods)]

mod common;

use aura_core::identifiers::DeviceId;
use aura_protocol::handlers::erased::AuraHandlerFactory;
use aura_protocol::handlers::{AuraContext, ExecutionMode};
use uuid::Uuid;

/// Test basic handler creation
#[tokio::test]
async fn test_composite_handler_creation() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let handler = AuraHandlerFactory::for_testing(device_id);
    let _ctx = AuraContext::for_testing(device_id);

    // Test that handler can be created and has correct execution mode
    assert_eq!(
        handler.execution_mode(),
        ExecutionMode::Simulation { seed: 0 }
    );

    // Test that handler can report supported effects
    let supported_effects = handler.supported_effects();
    // Testing handler creates CompositeHandler with full effect support
    assert!(!supported_effects.is_empty());
}

/// Test effect support
#[tokio::test]
async fn test_effect_support() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let handler = AuraHandlerFactory::for_testing(device_id);

    // Test that handler can report supported effects
    let supported_effects = handler.supported_effects();

    // Testing handler creates CompositeHandler with full effect support
    assert!(
        !supported_effects.is_empty(),
        "Testing handler should support multiple effects"
    );
}

/// Test execution mode
#[tokio::test]
async fn test_execution_mode() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let handler = AuraHandlerFactory::for_testing(device_id);

    // Test execution mode is correct for testing
    assert_eq!(
        handler.execution_mode(),
        ExecutionMode::Simulation { seed: 0 }
    );
    assert!(handler.execution_mode().is_deterministic());
    assert!(!handler.execution_mode().is_production());
}
