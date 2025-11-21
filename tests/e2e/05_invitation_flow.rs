//! Invitation Flow End-to-End Test
//!
//! This test validates invitation and relationship formation flows.

use aura_agent::runtime::AuthorityManager;
use aura_core::{AuthorityId, Result};

/// Test invitation context creation
#[tokio::test]
async fn test_invitation_context_setup() -> Result<()> {
    let mut manager = AuthorityManager::new("/tmp/aura-invitation-test".into());

    // Create inviter and invitee authorities
    let inviter_id = manager.create_authority(vec![], 1).await?;
    let invitee_id = manager.create_authority(vec![], 1).await?;

    // Create invitation context
    let context_id = manager
        .create_context(vec![inviter_id, invitee_id], "invitation".to_string())
        .await?;

    // Verify context was created with both participants
    let context = manager
        .get_context(&context_id)
        .expect("context should exist");
    assert_eq!(context.participants.len(), 2);
    assert!(context.participants.contains(&inviter_id));
    assert!(context.participants.contains(&invitee_id));

    Ok(())
}

/// Test multiple pending invitations
#[tokio::test]
async fn test_multiple_invitations() -> Result<()> {
    let mut manager = AuthorityManager::new("/tmp/aura-multi-invitation-test".into());

    // Create inviter authority
    let inviter_id = manager.create_authority(vec![], 1).await?;

    // Create multiple invitation contexts
    let mut context_ids = Vec::new();

    for _ in 0..3 {
        let invitee_id = manager.create_authority(vec![], 1).await?;

        let context_id = manager
            .create_context(vec![inviter_id, invitee_id], "invitation".to_string())
            .await?;
        context_ids.push(context_id);
    }

    // Verify all invitation contexts were created
    assert_eq!(context_ids.len(), 3);
    for context_id in context_ids {
        let context = manager
            .get_context(&context_id)
            .expect("context should exist");
        assert!(context.participants.contains(&inviter_id));
    }

    Ok(())
}

/// Test invitation acceptance flow
#[tokio::test]
async fn test_invitation_acceptance() -> Result<()> {
    let mut manager = AuthorityManager::new("/tmp/aura-invitation-accept-test".into());

    // Create inviter with device
    let inviter_id = manager.create_authority(vec![], 1).await?;
    manager
        .add_device_to_authority(inviter_id, vec![1, 2, 3, 4])
        .await?;

    // Create invitee with device
    let invitee_id = manager.create_authority(vec![], 1).await?;
    manager
        .add_device_to_authority(invitee_id, vec![5, 6, 7, 8])
        .await?;

    // Create invitation context
    let invitation_context = manager
        .create_context(vec![inviter_id, invitee_id], "invitation".to_string())
        .await?;

    // Verify invitation context
    let context = manager
        .get_context(&invitation_context)
        .expect("context should exist");
    assert_eq!(context.participants.len(), 2);

    // Create accepted relationship context (e.g., friend, colleague, etc.)
    let relationship_context = manager
        .create_context(vec![inviter_id, invitee_id], "friend".to_string())
        .await?;

    // Verify relationship context
    let rel_context = manager
        .get_context(&relationship_context)
        .expect("context should exist");
    assert!(rel_context.participants.contains(&inviter_id));
    assert!(rel_context.participants.contains(&invitee_id));

    Ok(())
}
