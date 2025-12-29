//! Invitation Service Integration Tests
//!
//! Tests for the InvitationService public API exposed through AuraAgent.

use aura_agent::{
    AgentBuilder, AuthorityId, EffectContext, ExecutionMode, InvitationStatus, InvitationType,
};
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
async fn test_invitation_service_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([70u8; 32]);
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
    let authority_id = AuthorityId::new_from_entropy([71u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([72u8; 32]);
    let invitation = invitations
        .invite_as_contact(
            receiver_id,
            Some("alice".to_string()),
            Some("Hi Alice!".to_string()),
            None,
        )
        .await?;

    assert!(invitation.invitation_id.starts_with("inv-"));
    assert_eq!(invitation.sender_id, authority_id);
    assert_eq!(invitation.receiver_id, receiver_id);
    assert_eq!(invitation.status, InvitationStatus::Pending);
    assert_eq!(invitation.message, Some("Hi Alice!".to_string()));
    Ok(())
}

#[tokio::test]
async fn test_invite_as_guardian_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([73u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([74u8; 32]);
    let invitation = invitations
        .invite_as_guardian(
            receiver_id,
            authority_id, // guarding self
            Some("Please be my guardian".to_string()),
            Some(604800000), // 1 week
        )
        .await?;

    assert!(invitation.invitation_id.starts_with("inv-"));
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
    let authority_id = AuthorityId::new_from_entropy([75u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([76u8; 32]);
    let invitation = invitations
        .invite_to_channel(receiver_id, "channel-123".to_string(), None, None)
        .await?;

    assert!(invitation.invitation_id.starts_with("inv-"));
    match &invitation.invitation_type {
        InvitationType::Channel { home_id } => {
            assert_eq!(home_id, "channel-123");
        }
        _ => panic!("Expected Channel invitation type"),
    }
    Ok(())
}

#[tokio::test]
async fn test_accept_invitation_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([77u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([78u8; 32]);
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
    let authority_id = AuthorityId::new_from_entropy([79u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([80u8; 32]);
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
    let authority_id = AuthorityId::new_from_entropy([81u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([82u8; 32]);
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
    let authority_id = AuthorityId::new_from_entropy([83u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    // Create 3 invitations
    let inv1 = invitations
        .invite_as_contact(AuthorityId::new_from_entropy([84u8; 32]), None, None, None)
        .await?;

    let inv2 = invitations
        .invite_as_contact(AuthorityId::new_from_entropy([85u8; 32]), None, None, None)
        .await?;

    let _inv3 = invitations
        .invite_as_contact(AuthorityId::new_from_entropy([86u8; 32]), None, None, None)
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
    let authority_id = AuthorityId::new_from_entropy([87u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let invitations = agent.invitations()?;

    let receiver_id = AuthorityId::new_from_entropy([88u8; 32]);
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
    let non_existent = invitations.get("non-existent-id").await;
    assert!(non_existent.is_none());
    Ok(())
}
