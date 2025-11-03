//! Tests for runtime execution context and session management
//!
//! This module tests the runtime components that manage execution environments
//! and session contexts for protocol operations.

mod common;

use aura_protocol::{
    runtime::{ExecutionContext, ContextBuilder, SessionManager, SessionStatus},
    middleware::MiddlewareConfig,
    effects::{ProtocolEffects, TimeEffects},
};
use aura_types::DeviceId;
use common::{helpers::*, test_utils::*};
use uuid::Uuid;

/// Test basic execution context creation
#[tokio::test]
async fn test_execution_context_creation() {
    let device_id = create_test_device_id();
    let session_id = create_test_session_id();
    let participants = create_test_participants(3);
    
    let context = ContextBuilder::new()
        .with_device_id(device_id)
        .with_session_id(session_id)
        .with_participants(participants.clone())
        .with_threshold(2)
        .build_for_testing();
    
    assert_eq!(context.device_id, device_id);
    assert_eq!(context.session_id, session_id);
    assert_eq!(context.participants, participants);
    assert_eq!(context.participant_count(), 3);
    assert_eq!(context.threshold(), Some(2));
    assert!(context.is_simulation);
    assert!(context.has_sufficient_participants());
}

/// Test different execution context modes
#[tokio::test]
async fn test_execution_context_modes() {
    let device_id = create_test_device_id();
    let participants = create_test_participants(2);
    
    // Test testing mode
    let test_context = ContextBuilder::new()
        .with_device_id(device_id)
        .with_participants(participants.clone())
        .build_for_testing();
    
    assert!(test_context.is_simulation);
    
    // Test simulation mode
    let sim_context = ContextBuilder::new()
        .with_device_id(device_id)
        .with_participants(participants.clone())
        .build_for_simulation();
    
    assert!(sim_context.is_simulation);
    
    // Test production mode
    let prod_context = ContextBuilder::new()
        .with_device_id(device_id)
        .with_participants(participants)
        .build_for_production();
    
    assert!(!prod_context.is_simulation);
}

/// Test context builder validation
#[tokio::test]
async fn test_context_builder_validation() {
    let device_id = create_test_device_id();
    
    // Test builder with minimal configuration
    let context = ContextBuilder::new()
        .with_device_id(device_id)
        .build_for_testing();
    
    assert_eq!(context.device_id, device_id);
    assert!(context.participants.is_empty());
    assert_eq!(context.threshold(), None);
    assert!(context.has_sufficient_participants()); // No threshold, so always true
}

/// Test context builder with middleware configuration
#[tokio::test]
async fn test_context_builder_with_middleware() {
    let device_id = create_test_device_id();
    let middleware_config = create_test_middleware_config();
    
    let context = ContextBuilder::new()
        .with_device_id(device_id)
        .with_middleware_config(middleware_config)
        .build_for_testing();
    
    assert_eq!(context.device_id, device_id);
    
    // Test that effects are properly configured
    let random_bytes = context.effects.random_bytes(16).await;
    assert_eq!(random_bytes.len(), 16);
    
    let timestamp = context.effects.current_timestamp().await;
    assert!(timestamp.is_ok());
}

/// Test participant management
#[tokio::test]
async fn test_participant_management() {
    let device_id = create_test_device_id();
    let other_devices = create_test_participants(3);
    let mut all_participants = vec![device_id];
    all_participants.extend(other_devices);
    
    let context = ContextBuilder::new()
        .with_device_id(device_id)
        .with_participants(all_participants.clone())
        .with_threshold(3)
        .build_for_testing();
    
    // Test participant queries
    assert_eq!(context.participant_count(), 4);
    assert!(context.is_participant(device_id));
    assert!(context.is_participant(all_participants[1]));
    assert!(!context.is_participant(DeviceId::from(Uuid::new_v4())));
    
    // Test participant index
    let index = context.participant_index();
    assert_eq!(index, Some(0)); // First participant
    
    // Test threshold validation
    assert!(context.has_sufficient_participants()); // 4 >= 3
}

/// Test threshold validation scenarios
#[tokio::test]
async fn test_threshold_validation() {
    let device_id = create_test_device_id();
    let participants = create_test_participants(2); // Only 2 participants
    
    // Test insufficient participants
    let context = ContextBuilder::new()
        .with_device_id(device_id)
        .with_participants(participants.clone())
        .with_threshold(3) // Requires 3, but only 2 participants
        .build_for_testing();
    
    assert!(!context.has_sufficient_participants());
    assert_eq!(context.threshold(), Some(3));
    assert_eq!(context.participant_count(), 2);
    
    // Test sufficient participants
    let context2 = ContextBuilder::new()
        .with_device_id(device_id)
        .with_participants(participants)
        .with_threshold(2) // Requires 2, has 2 participants
        .build_for_testing();
    
    assert!(context2.has_sufficient_participants());
}

/// Test session manager basic functionality
#[tokio::test]
async fn test_session_manager_basic() {
    use aura_protocol::runtime::SessionConfig;
    let mut session_manager = SessionManager::new(SessionConfig::default());
    let participants = create_test_participants(3);
    
    // Test session creation
    let session_id = session_manager.create_session(
        "DKD".to_string(),
        participants.clone(),
        None,
    ).unwrap();
    
    // Test session retrieval
    let session = session_manager.get_session(session_id);
    assert!(session.is_some());
    
    let session = session.unwrap();
    assert_eq!(session.protocol_type, "DKD");
    assert_eq!(session.participants, participants);
    assert_eq!(session.status, SessionStatus::Initializing);
}

/// Test session status transitions
#[tokio::test]
async fn test_session_status_transitions() {
    use aura_protocol::runtime::SessionConfig;
    let mut session_manager = SessionManager::new(SessionConfig::default());
    let participants = create_test_participants(2);
    
    let session_id = session_manager.create_session(
        "Counter".to_string(),
        participants,
        None,
    ).unwrap();
    
    // Test status transitions
    let result = session_manager.update_session_status(session_id, SessionStatus::Active);
    assert!(result.is_ok());
    let session = session_manager.get_session(session_id).unwrap();
    assert_eq!(session.status, SessionStatus::Active);
    
    let result = session_manager.update_session_status(session_id, SessionStatus::Completed);
    assert!(result.is_ok());
    let session = session_manager.get_session(session_id).unwrap();
    assert_eq!(session.status, SessionStatus::Completed);
}

/// Test session operations
#[tokio::test]
async fn test_session_operations() {
    use aura_protocol::runtime::SessionConfig;
    let mut session_manager = SessionManager::new(SessionConfig::default());
    let participants = create_test_participants(2);
    
    // Create multiple sessions
    let session1 = session_manager.create_session(
        "DKD".to_string(),
        participants.clone(),
        None,
    ).unwrap();
    
    let session2 = session_manager.create_session(
        "Resharing".to_string(),
        participants,
        None,
    ).unwrap();
    
    // Test both sessions exist
    assert!(session_manager.get_session(session1).is_some());
    assert!(session_manager.get_session(session2).is_some());
    
    // Test completion
    let result = session_manager.complete_session(session1);
    assert!(result.is_ok());
    
    let session = session_manager.get_session(session1).unwrap();
    assert_eq!(session.status, SessionStatus::Completed);
}

/// Test execution context integration with effects
#[tokio::test]
async fn test_execution_context_effects_integration() {
    let context = create_test_execution_context();
    
    // Test that we can use effects through the context
    let random_bytes = context.effects.random_bytes(20).await;
    assert_eq!(random_bytes.len(), 20);
    
    // Test storage operations
    let test_key = "context_integration_test";
    let test_value = create_test_data(15);
    
    context.effects.store(test_key, test_value.clone()).await.unwrap();
    let retrieved = context.effects.retrieve(test_key).await.unwrap();
    assert_eq!(retrieved, Some(test_value));
    
    // Test crypto operations
    let test_data = b"context integration test data";
    let hash = context.effects.blake3_hash(test_data).await;
    assert_eq!(hash.len(), 32);
    
    // Test time operations using TimeEffects trait
    use aura_protocol::effects::TimeEffects;
    let timestamp = context.effects.current_timestamp().await;
    assert!(timestamp.is_ok());
    
    // Test network operations
    let peers = context.effects.connected_peers().await;
    assert!(peers.is_empty()); // Test environment starts with no peers
}

/// Test session error handling
#[tokio::test]
async fn test_session_error_handling() {
    use aura_protocol::runtime::SessionConfig;
    let mut session_manager = SessionManager::new(SessionConfig::default());
    let invalid_session_id = Uuid::new_v4();
    
    // Test operations on non-existent session
    let result = session_manager.get_session(invalid_session_id);
    assert!(result.is_none());
    
    let result = session_manager.update_session_status(invalid_session_id, SessionStatus::Active);
    assert!(result.is_err());
}

/// Test execution mode variations
#[tokio::test]
async fn test_execution_mode_variations() {
    let device_id = create_test_device_id();
    let participants = create_test_participants(2);
    
    // Test different execution modes through context creation
    let contexts = vec![
        ContextBuilder::new()
            .with_device_id(device_id)
            .with_participants(participants.clone())
            .build_for_testing(),
        ContextBuilder::new()
            .with_device_id(device_id)
            .with_participants(participants.clone())
            .build_for_production(),
        ContextBuilder::new()
            .with_device_id(device_id)
            .with_participants(participants)
            .build_for_simulation(),
    ];
    
    // All contexts should provide working effects
    for context in contexts {
        let random_bytes = context.effects.random_bytes(8).await;
        assert_eq!(random_bytes.len(), 8);
        
        use aura_protocol::effects::TimeEffects;
        let timestamp = context.effects.current_timestamp().await;
        assert!(timestamp.is_ok());
        
        // Test basic storage operation
        let test_key = format!("mode_test_{}", context.is_simulation);
        let test_value = b"mode_test_value".to_vec();
        
        context.effects.store(&test_key, test_value.clone()).await.unwrap();
        let retrieved = context.effects.retrieve(&test_key).await.unwrap();
        assert_eq!(retrieved, Some(test_value));
    }
}