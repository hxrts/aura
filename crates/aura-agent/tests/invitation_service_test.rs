//! Invitation Service Integration Tests
//!
//! Tests for the InvitationServiceApi public API exposed through AuraAgent.
//!
//! Each test uses a distinct entropy range to ensure test isolation:
//! - test_invitation_service_via_agent: 200-209
//! - test_invite_as_contact_via_agent: 210-219
//! - test_invite_as_guardian_via_agent: 220-229
//! - test_invite_to_channel_via_agent: 230-239
//! - test_accept_invitation_via_agent: 240-241
//! - test_decline_invitation_via_agent_succeeds_despite_followup_failure: 242-243
//! - test_cancel_invitation_via_agent: 244-245
//! - test_list_pending_via_agent: 246-249
//! - test_get_invitation_via_agent: 250-251

use aura_agent::{
    AgentBuilder, AuraAgent, AuthorityId, EffectContext, ExecutionMode, InvitationStatus,
    InvitationType,
};
use aura_core::effects::amp::ChannelCreateParams;
use aura_core::effects::AmpChannelEffects;
use aura_core::effects::ThresholdSigningEffects;
use aura_core::hash::hash;
use aura_core::threshold::ParticipantIdentity;
use aura_core::types::identifiers::{ChannelId, ContextId, InvitationId};
use std::sync::Arc;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

/// Create a test effect context for async tests
fn test_context(authority_id: AuthorityId) -> EffectContext {
    let context_entropy = hash(&authority_id.to_bytes());
    EffectContext::new(
        authority_id,
        ContextId::new_from_entropy(context_entropy),
        ExecutionMode::Testing,
    )
}

/// Helper to create a properly initialized test agent.
///
/// This sets up Biscuit tokens and key rotation which are required for
/// authorization guards to function correctly.
async fn create_test_agent(seed: u8) -> TestResult<Arc<AuraAgent>> {
    let authority_id = AuthorityId::new_from_entropy([seed; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;
    let effects = agent.runtime().effects();
    effects.bootstrap_authority(&authority_id).await?;
    let participants = vec![ParticipantIdentity::guardian(authority_id)];
    let (epoch, _, _) = effects
        .rotate_keys(&authority_id, 1, 1, &participants)
        .await?;
    effects.commit_key_rotation(&authority_id, epoch).await?;
    Ok(Arc::new(agent))
}

#[tokio::test]
async fn test_invitation_service_via_agent() -> TestResult {
    // Entropy range: 200-209
    let agent = create_test_agent(200).await?;

    let invitations = agent.invitations()?;

    // Initially no pending invitations
    let pending = invitations.list_pending().await;
    assert!(pending.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_invite_as_contact_via_agent() -> TestResult {
    // Entropy range: 210-219
    let agent = create_test_agent(210).await?;
    let authority_id = agent.authority_id();

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([211u8; 32]);
    let invitation = invitations
        .invite_as_contact(
            receiver_id,
            Some("alice".to_string()),
            Some("Hi Alice!".to_string()),
            None,
        )
        .await?;

    assert!(invitation.invitation_id.as_str().starts_with("inv-"));
    assert_eq!(invitation.sender_id, authority_id);
    assert_eq!(invitation.receiver_id, receiver_id);
    assert_eq!(invitation.status, InvitationStatus::Pending);
    assert_eq!(invitation.message, Some("Hi Alice!".to_string()));
    Ok(())
}

#[tokio::test]
async fn test_invite_as_guardian_via_agent() -> TestResult {
    // Entropy range: 220-229
    let agent = create_test_agent(220).await?;
    let authority_id = agent.authority_id();

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([221u8; 32]);
    let invitation = invitations
        .invite_as_guardian(
            receiver_id,
            authority_id, // guarding self
            Some("Please be my guardian".to_string()),
            Some(604800000), // 1 week
        )
        .await?;

    assert!(invitation.invitation_id.as_str().starts_with("inv-"));
    assert!(invitation.expires_at.is_some());
    match &invitation.invitation_type {
        InvitationType::Guardian { subject_authority } => {
            assert_eq!(*subject_authority, authority_id);
        }
        _ => panic!("Expected Guardian invitation type"),
    }
    Ok(())
}

#[tokio::test]
async fn test_invite_to_channel_via_agent() -> TestResult {
    // Entropy range: 230-239
    let agent = create_test_agent(230).await?;
    let context_id = ContextId::new_from_entropy([233u8; 32]);

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([231u8; 32]);
    let home_id = ChannelId::from_bytes([232u8; 32]);
    agent
        .runtime()
        .effects()
        .create_channel(ChannelCreateParams {
            context: context_id,
            channel: Some(home_id),
            skip_window: None,
            topic: None,
        })
        .await?;
    let invitation = invitations
        .invite_to_channel(
            receiver_id,
            home_id.to_string(),
            Some(context_id),
            Some("shared-parity-lab".to_string()),
            None,
            None,
            None,
        )
        .await?;

    assert!(invitation.invitation_id.as_str().starts_with("inv-"));
    match &invitation.invitation_type {
        InvitationType::Channel {
            home_id,
            nickname_suggestion,
            ..
        } => {
            assert_eq!(home_id, &ChannelId::from_bytes([232u8; 32]));
            assert_eq!(nickname_suggestion.as_deref(), Some("shared-parity-lab"));
        }
        _ => panic!("Expected Channel invitation type"),
    }
    Ok(())
}

#[tokio::test]
async fn test_invite_to_channel_rejects_invalid_home_id() -> TestResult {
    let agent = create_test_agent(239).await?;
    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([238u8; 32]);
    let err = match invitations
        .invite_to_channel(
            receiver_id,
            "channel-123".to_string(),
            None,
            None,
            None,
            None,
            None,
        )
        .await
    {
        Ok(value) => panic!("invalid home id should be rejected: {value:?}"),
        Err(error) => error,
    };
    assert!(err.to_string().contains("invalid channel/home id"));
    Ok(())
}

#[tokio::test]
async fn test_accept_invitation_via_agent() -> TestResult {
    // Entropy range: 240-241
    let sender = create_test_agent(240).await?;
    let receiver = create_test_agent(241).await?;

    let sender_invitations = sender.invitations()?;
    let receiver_invitations = receiver.invitations()?;
    let receiver_id = receiver.authority_id();
    let invitation = sender_invitations
        .invite_as_contact(receiver_id, None, None, None)
        .await?;
    let code = sender_invitations
        .export_code(&invitation.invitation_id)
        .await?;
    let imported = receiver_invitations.import_and_cache(&code).await?;

    let result = receiver_invitations.accept(&imported.invitation_id).await?;

    assert_eq!(result.new_status, InvitationStatus::Accepted);
    Ok(())
}

#[tokio::test]
async fn test_decline_invitation_via_agent_succeeds_despite_followup_failure() -> TestResult {
    // Entropy range: 242-243
    let sender = create_test_agent(242).await?;
    let receiver = create_test_agent(243).await?;

    let sender_invitations = sender.invitations()?;
    let receiver_invitations = receiver.invitations()?;
    let receiver_id = receiver.authority_id();
    let invitation = sender_invitations
        .invite_as_contact(receiver_id, None, None, None)
        .await?;
    let code = sender_invitations
        .export_code(&invitation.invitation_id)
        .await?;
    let imported = receiver_invitations.import_and_cache(&code).await?;

    let result = receiver_invitations
        .decline(&imported.invitation_id)
        .await?;
    assert_eq!(result.new_status, InvitationStatus::Declined);
    Ok(())
}

#[tokio::test]
async fn test_cancel_invitation_via_agent() -> TestResult {
    // Entropy range: 244-245
    let agent = create_test_agent(244).await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([245u8; 32]);
    let invitation = invitations
        .invite_as_contact(receiver_id, None, None, None)
        .await?;

    // Verify it's pending
    assert!(invitations.is_pending(&invitation.invitation_id).await);

    let result = invitations.cancel(&invitation.invitation_id).await?;

    assert_eq!(result.new_status, InvitationStatus::Cancelled);

    // Verify it's no longer pending
    assert!(!invitations.is_pending(&invitation.invitation_id).await);
    Ok(())
}

#[tokio::test]
async fn test_list_pending_via_agent() -> TestResult {
    // Entropy range: 246-249
    let agent = create_test_agent(246).await?;

    let invitations = agent.invitations()?;

    // Create 3 invitations with distinct deterministic IDs
    let inv1 = invitations
        .invite_as_contact(AuthorityId::new_from_entropy([247u8; 32]), None, None, None)
        .await?;

    let inv2 = invitations
        .invite_as_contact(AuthorityId::new_from_entropy([248u8; 32]), None, None, None)
        .await?;

    let _inv3 = invitations
        .invite_as_contact(AuthorityId::new_from_entropy([249u8; 32]), None, None, None)
        .await?;

    // All 3 should be pending
    let pending = invitations.list_pending().await;
    assert_eq!(pending.len(), 3);

    // Accept one
    invitations.accept(&inv1.invitation_id).await?;

    // Decline another
    invitations.decline(&inv2.invitation_id).await?;

    // Only 1 should remain pending
    let pending = invitations.list_pending().await;
    assert_eq!(pending.len(), 1);
    Ok(())
}

#[tokio::test]
async fn test_get_invitation_via_agent() -> TestResult {
    // Entropy range: 250-251
    let agent = create_test_agent(250).await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([251u8; 32]);
    let invitation = invitations
        .invite_as_contact(
            receiver_id,
            Some("bob".to_string()),
            Some("Hello Bob!".to_string()),
            None,
        )
        .await?;

    // Should be able to retrieve it
    let retrieved = match invitations.get(&invitation.invitation_id).await {
        Some(inv) => inv,
        None => return Err("Invitation should exist".into()),
    };

    assert_eq!(retrieved.invitation_id, invitation.invitation_id);
    assert_eq!(retrieved.message, Some("Hello Bob!".to_string()));

    // Non-existent invitation should return None
    let non_existent = invitations.get(&InvitationId::new("non-existent-id")).await;
    assert!(non_existent.is_none());
    Ok(())
}
