//! Authentication Service Integration Tests
//!
//! Tests for the AuthService public API exposed through AuraAgent.

use aura_agent::{AgentBuilder, AuthMethod, AuthorityId, EffectContext, ExecutionMode};
use aura_core::hash::hash;
use aura_core::identifiers::ContextId;

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
async fn test_auth_service_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([60u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let auth = agent.auth().await.expect("Failed to get auth service");

    // Check device ID is set
    assert!(!auth.device_id().0.is_nil());
}

#[tokio::test]
async fn test_is_authenticated_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([61u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let auth = agent.auth().await.expect("Failed to get auth service");

    // In test mode, is_authenticated should return true
    assert!(auth.is_authenticated().await);
}

#[tokio::test]
async fn test_create_challenge_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([62u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let auth = agent.auth().await.expect("Failed to get auth service");

    let challenge = auth
        .create_challenge()
        .await
        .expect("Failed to create challenge");

    assert!(!challenge.challenge_id.is_empty());
    assert_eq!(challenge.challenge_bytes.len(), 32);
    assert!(challenge.expires_at > challenge.created_at);
    assert_eq!(challenge.authority_id, authority_id);
}

#[tokio::test]
async fn test_supported_methods_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([63u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let auth = agent.auth().await.expect("Failed to get auth service");

    let methods = auth.supported_methods();
    assert!(methods.contains(&AuthMethod::DeviceKey));
    assert!(methods.contains(&AuthMethod::ThresholdSignature));
    assert!(!methods.contains(&AuthMethod::Passkey)); // Not yet supported
}

#[tokio::test]
async fn test_device_key_auth_flow_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([64u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let auth = agent.auth().await.expect("Failed to get auth service");

    // Test the full device key authentication flow
    let result = auth
        .authenticate_with_device_key()
        .await
        .expect("Failed to authenticate");

    assert!(result.authenticated);
    assert_eq!(result.authority_id, Some(authority_id));
    assert!(result.device_id.is_some());
    assert!(result.failure_reason.is_none());
}

#[tokio::test]
async fn test_challenge_response_flow_via_agent() {
    let authority_id = AuthorityId::new_from_entropy([65u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let auth = agent.auth().await.expect("Failed to get auth service");

    // Create a challenge
    let challenge = auth
        .create_challenge()
        .await
        .expect("Failed to create challenge");

    // Verify the challenge was created correctly
    assert!(challenge.challenge_id.starts_with("challenge-"));
    assert_eq!(challenge.challenge_bytes.len(), 32);
}

#[tokio::test]
async fn test_invalid_challenge_rejected() {
    let authority_id = AuthorityId::new_from_entropy([66u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await
        .expect("Failed to build testing agent");

    let auth = agent.auth().await.expect("Failed to get auth service");

    // Try to verify a response with an invalid challenge ID
    let invalid_response = aura_agent::AuthResponse {
        challenge_id: "invalid-challenge-id".to_string(),
        signature: vec![0u8; 64],
        public_key: vec![0u8; 32],
        auth_method: AuthMethod::DeviceKey,
    };

    let result = auth
        .verify(&invalid_response)
        .await
        .expect("Verify should return result");

    assert!(!result.authenticated);
    assert!(result.failure_reason.is_some());
    assert!(result.failure_reason.unwrap().contains("not found"));
}
