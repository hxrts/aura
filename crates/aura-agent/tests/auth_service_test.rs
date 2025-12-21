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
async fn test_auth_service_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([60u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let auth = agent.auth()?;

    // Check device ID is set
    assert!(!auth.device_id().0.is_nil());
    Ok(())
}

#[tokio::test]
async fn test_is_authenticated_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([61u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let auth = agent.auth()?;

    // In test mode, is_authenticated should return true
    assert!(auth.is_authenticated().await);
    Ok(())
}

#[tokio::test]
async fn test_create_challenge_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([62u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let auth = agent.auth()?;

    let challenge = auth.create_challenge().await?;

    assert!(!challenge.challenge_id.is_empty());
    assert_eq!(challenge.challenge_bytes.len(), 32);
    assert!(challenge.expires_at > challenge.created_at);
    assert_eq!(challenge.authority_id, authority_id);
    Ok(())
}

#[tokio::test]
async fn test_supported_methods_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([63u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let auth = agent.auth()?;

    let methods = auth.supported_methods();
    assert!(methods.contains(&AuthMethod::DeviceKey));
    assert!(methods.contains(&AuthMethod::ThresholdSignature));
    assert!(!methods.contains(&AuthMethod::Passkey)); // Not yet supported
    Ok(())
}

#[tokio::test]
async fn test_device_key_auth_flow_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([64u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let auth = agent.auth()?;

    // Test the full device key authentication flow
    let result = auth.authenticate_with_device_key().await?;

    assert!(result.authenticated);
    assert_eq!(result.authority_id, Some(authority_id));
    assert!(result.device_id.is_some());
    assert!(result.failure_reason.is_none());
    Ok(())
}

#[tokio::test]
async fn test_challenge_response_flow_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([65u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let auth = agent.auth()?;

    // Create a challenge
    let challenge = auth.create_challenge().await?;

    // Verify the challenge was created correctly
    assert!(challenge.challenge_id.starts_with("challenge-"));
    assert_eq!(challenge.challenge_bytes.len(), 32);
    Ok(())
}

#[tokio::test]
async fn test_invalid_challenge_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([66u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let auth = agent.auth()?;

    // Try to verify a response with an invalid challenge ID
    let invalid_response = aura_agent::AuthResponse {
        challenge_id: "invalid-challenge-id".to_string(),
        signature: vec![0u8; 64],
        public_key: vec![0u8; 32],
        auth_method: AuthMethod::DeviceKey,
    };

    let result = auth.verify(&invalid_response).await?;

    assert!(!result.authenticated);
    if let Some(reason) = result.failure_reason {
        assert!(reason.contains("not found"));
    } else {
        return Err("Expected failure reason to be present".into());
    }
    Ok(())
}
