//! Session Service Integration Tests
//!
//! Tests for the SessionService public API exposed through AuraAgent.

use aura_agent::{AgentBuilder, AuthorityId, EffectContext, ExecutionMode};
use aura_core::hash::hash;
use aura_core::identifiers::ContextId;
use aura_protocol::effects::SessionType;

/// Create a test effect context for async tests
fn test_context(authority_id: AuthorityId) -> EffectContext {
    let context_entropy = hash(&authority_id.to_bytes());
    EffectContext::new(
        authority_id,
        ContextId::new_from_entropy(context_entropy),
        ExecutionMode::Testing,
    )
}

#[tokio::test]
async fn test_session_service_via_agent() {
    // Create a testing agent
    let authority_id = AuthorityId::new_from_entropy([50u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    // Get the session service
    let sessions = agent.sessions().await;

    // Create a coordination session
    let participants = vec![sessions.device_id()];
    let handle = sessions
        .create_coordination_session(participants.clone())
        .await
        .expect("Failed to create session");

    assert!(!handle.session_id.is_empty());
    assert_eq!(handle.participants, participants);
    assert_eq!(handle.session_type, SessionType::Coordination);
}

#[tokio::test]
async fn test_session_stats_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([51u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let sessions = agent.sessions().await;
    let stats = sessions.get_stats().await.expect("Failed to get stats");

    // Initially no active sessions
    assert_eq!(stats.active_sessions, 0);
}

#[tokio::test]
async fn test_threshold_session_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([52u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let sessions = agent.sessions().await;
    let device_id = sessions.device_id();

    // Create 3 device IDs for a 2-of-3 threshold
    let participants = vec![
        device_id,
        aura_core::identifiers::DeviceId::new_from_entropy([1u8; 32]),
        aura_core::identifiers::DeviceId::new_from_entropy([2u8; 32]),
    ];

    let handle = sessions
        .create_threshold_session(participants.clone(), 2)
        .await
        .expect("Failed to create threshold session");

    assert!(!handle.session_id.is_empty());
    assert_eq!(handle.session_type, SessionType::ThresholdOperation);
    assert!(handle.metadata.contains_key("threshold"));
}

#[tokio::test]
async fn test_key_rotation_session_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([53u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let sessions = agent.sessions().await;

    let handle = sessions
        .create_key_rotation_session()
        .await
        .expect("Failed to create key rotation session");

    assert!(!handle.session_id.is_empty());
    assert_eq!(handle.session_type, SessionType::KeyRotation);
    assert!(handle.metadata.contains_key("rotation_type"));
}

#[tokio::test]
async fn test_end_session_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([54u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let sessions = agent.sessions().await;
    let participants = vec![sessions.device_id()];

    // Create and then end a session
    let handle = sessions
        .create_coordination_session(participants)
        .await
        .expect("Failed to create session");

    let ended = sessions
        .end_session(&handle.session_id)
        .await
        .expect("Failed to end session");

    assert_eq!(ended.session_id, handle.session_id);
    assert!(ended.metadata.contains_key("status"));
}
