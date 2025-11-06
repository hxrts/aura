//! Test basic handler creation and functionality
//!
//! Minimal tests to verify that handlers can be created and basic operations work

#![allow(clippy::disallowed_methods)]

mod common;

use aura_types::{
    handlers::{erased::AuraHandlerFactory, AuraContext, ExecutionMode},
    identifiers::DeviceId,
};
use uuid::Uuid;

/// Test basic handler creation
#[tokio::test]
async fn test_composite_handler_creation() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let handler = AuraHandlerFactory::for_testing(device_id);
    let _ctx = AuraContext::for_testing(device_id);

    // Test that handler can be created and has correct execution mode
    assert_eq!(handler.execution_mode(), ExecutionMode::Testing);
    
    // Test that handler can report supported effects (current stub returns empty)
    let supported_effects = handler.supported_effects();
    // Current stub implementation supports no effects - this is expected for now
    assert!(supported_effects.is_empty());
}

/// Test effect support
#[tokio::test]
async fn test_effect_support() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let handler = AuraHandlerFactory::for_testing(device_id);

    // Test that handler can report supported effects
    let supported_effects = handler.supported_effects();
    
    // Current stub implementation supports no effects - this is expected for now
    // In a real implementation, handlers would support specific effect types
    assert!(supported_effects.is_empty(), "Current stub implementation supports no effects");
}

/// Test execution mode
#[tokio::test]
async fn test_execution_mode() {
    let device_id = DeviceId::from(Uuid::new_v4());
    let handler = AuraHandlerFactory::for_testing(device_id);

    // Test execution mode is correct for testing
    assert_eq!(handler.execution_mode(), ExecutionMode::Testing);
    assert!(handler.execution_mode().is_deterministic());
    assert!(!handler.execution_mode().is_production());
}
