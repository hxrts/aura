//! Invitation Service Integration Tests
//!
//! Tests for the InvitationServiceApi public API exposed through AuraAgent.
//!
//! Each test uses a distinct entropy range to ensure test isolation:
//! - test_invitation_service_via_agent: 10-19
//! - test_invite_as_contact_via_agent: 20-29
//! - test_invite_as_guardian_via_agent: 30-39
//! - test_invite_to_channel_via_agent: 40-49
//! - test_accept_invitation_via_agent: 50-59
//! - test_decline_invitation_via_agent: 60-69
//! - test_cancel_invitation_via_agent: 70-79
//! - test_list_pending_via_agent: 80-89
//! - test_get_invitation_via_agent: 90-99

use aura_agent::{
    AgentBuilder, AuthorityId, EffectContext, ExecutionMode, InvitationStatus, InvitationType,
};
use aura_core::hash::hash;
use aura_core::identifiers::{ContextId, InvitationId};

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
async fn test_invitation_service_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    // Entropy range: 10-19
    let authority_id = AuthorityId::new_from_entropy([10u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    // Initially no pending invitations
    let pending = invitations.list_pending().await;
    assert!(pending.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_invite_as_contact_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    // Entropy range: 20-29
    let authority_id = AuthorityId::new_from_entropy([20u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([21u8; 32]);
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
async fn test_invite_as_guardian_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    // Entropy range: 30-39
    let authority_id = AuthorityId::new_from_entropy([30u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([31u8; 32]);
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
async fn test_invite_to_channel_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    // Entropy range: 40-49
    let authority_id = AuthorityId::new_from_entropy([40u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([41u8; 32]);
    let invitation = invitations
        .invite_to_channel(receiver_id, "channel-123".to_string(), None, None, None)
        .await?;

    assert!(invitation.invitation_id.as_str().starts_with("inv-"));
    match &invitation.invitation_type {
        InvitationType::Channel { home_id, .. } => {
            assert_eq!(home_id, "channel-123");
        }
        _ => panic!("Expected Channel invitation type"),
    }
    Ok(())
}

#[tokio::test]
async fn test_accept_invitation_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    // Entropy range: 50-59
    let authority_id = AuthorityId::new_from_entropy([50u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([51u8; 32]);
    let invitation = invitations
        .invite_as_contact(receiver_id, None, None, None)
        .await?;

    let result = invitations.accept(&invitation.invitation_id).await?;

    assert!(result.success);
    assert_eq!(result.new_status, Some(InvitationStatus::Accepted));
    Ok(())
}

#[tokio::test]
async fn test_decline_invitation_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    // Entropy range: 60-69
    let authority_id = AuthorityId::new_from_entropy([60u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([61u8; 32]);
    let invitation = invitations
        .invite_as_contact(receiver_id, None, None, None)
        .await?;

    let result = invitations.decline(&invitation.invitation_id).await?;

    assert!(result.success);
    assert_eq!(result.new_status, Some(InvitationStatus::Declined));
    Ok(())
}

#[tokio::test]
async fn test_cancel_invitation_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    // Entropy range: 70-79
    let authority_id = AuthorityId::new_from_entropy([70u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([71u8; 32]);
    let invitation = invitations
        .invite_as_contact(receiver_id, None, None, None)
        .await?;

    // Verify it's pending
    assert!(invitations.is_pending(&invitation.invitation_id).await);

    let result = invitations.cancel(&invitation.invitation_id).await?;

    assert!(result.success);
    assert_eq!(result.new_status, Some(InvitationStatus::Cancelled));

    // Verify it's no longer pending
    assert!(!invitations.is_pending(&invitation.invitation_id).await);
    Ok(())
}

#[tokio::test]
async fn test_list_pending_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    // Entropy range: 80-89
    let authority_id = AuthorityId::new_from_entropy([80u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    // Create 3 invitations with distinct deterministic IDs
    let inv1 = invitations
        .invite_as_contact(AuthorityId::new_from_entropy([81u8; 32]), None, None, None)
        .await?;

    let inv2 = invitations
        .invite_as_contact(AuthorityId::new_from_entropy([82u8; 32]), None, None, None)
        .await?;

    let _inv3 = invitations
        .invite_as_contact(AuthorityId::new_from_entropy([83u8; 32]), None, None, None)
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
async fn test_get_invitation_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    // Entropy range: 90-99
    let authority_id = AuthorityId::new_from_entropy([90u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([91u8; 32]);
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
