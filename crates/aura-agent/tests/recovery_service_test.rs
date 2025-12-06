//! Recovery Service Integration Tests
//!
//! Tests for the RecoveryService public API exposed through AuraAgent.

use aura_agent::{
    AgentBuilder, AuthorityId, EffectContext, ExecutionMode, GuardianApproval, RecoveryState,
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
async fn test_recovery_service_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([90u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let recovery = agent.recovery().await?;

    // Initially no active recoveries
    let active = recovery.list_active().await;
    assert!(active.is_empty());
    Ok(())
}

#[tokio::test]
async fn test_add_device_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([91u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let recovery = agent.recovery().await?;

    let guardians = vec![
        AuthorityId::new_from_entropy([92u8; 32]),
        AuthorityId::new_from_entropy([93u8; 32]),
    ];

    let request = recovery
        .add_device(
            vec![0u8; 32],
            guardians,
            2,
            "Adding backup device".to_string(),
            None,
        )
        .await?;

    assert!(request.recovery_id.starts_with("recovery-"));
    assert_eq!(request.threshold, 2);
    Ok(())
}

#[tokio::test]
async fn test_remove_device_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([94u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let recovery = agent.recovery().await?;

    let guardians = vec![AuthorityId::new_from_entropy([95u8; 32])];

    let request = recovery
        .remove_device(0, guardians, 1, "Device compromised".to_string(), None)
        .await?;

    assert!(request.recovery_id.starts_with("recovery-"));
    Ok(())
}

#[tokio::test]
async fn test_replace_tree_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([96u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let recovery = agent.recovery().await?;

    let guardians = vec![
        AuthorityId::new_from_entropy([97u8; 32]),
        AuthorityId::new_from_entropy([98u8; 32]),
        AuthorityId::new_from_entropy([99u8; 32]),
    ];

    let request = recovery
        .replace_tree(
            vec![0u8; 32],
            guardians,
            2,
            "Full recovery after device loss".to_string(),
            Some(604800000), // 1 week
        )
        .await?;

    assert!(request.recovery_id.starts_with("recovery-"));
    assert!(request.expires_at.is_some());
    Ok(())
}

#[tokio::test]
async fn test_update_guardians_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([100u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let recovery = agent.recovery().await?;

    let current_guardians = vec![AuthorityId::new_from_entropy([101u8; 32])];
    let new_guardians = vec![
        AuthorityId::new_from_entropy([102u8; 32]),
        AuthorityId::new_from_entropy([103u8; 32]),
    ];

    let request = recovery
        .update_guardians(
            new_guardians,
            2, // new threshold
            current_guardians,
            1, // current threshold
            "Upgrading guardian set".to_string(),
            None,
        )
        .await?;

    assert!(request.recovery_id.starts_with("recovery-"));
    Ok(())
}

#[tokio::test]
async fn test_full_recovery_flow_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([104u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let recovery = agent.recovery().await?;

    let guardians = vec![AuthorityId::new_from_entropy([105u8; 32])];

    // Initiate
    let request = recovery
        .add_device(
            vec![0u8; 32],
            guardians.clone(),
            1,
            "Test".to_string(),
            None,
        )
        .await?;

    // Check pending
    assert!(recovery.is_pending(&request.recovery_id).await);

    // Submit approval
    let approval = GuardianApproval {
        recovery_id: request.recovery_id.clone(),
        guardian_id: guardians[0],
        signature: vec![0u8; 64],
        share_data: None,
        approved_at: 12345,
    };
    recovery.submit_approval(approval).await?;

    // Complete
    let result = recovery.complete(&request.recovery_id).await?;
    assert!(result.success);
    Ok(())
}

#[tokio::test]
async fn test_cancel_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([106u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let recovery = agent.recovery().await?;

    let guardians = vec![AuthorityId::new_from_entropy([107u8; 32])];

    // Initiate
    let request = recovery
        .add_device(vec![0u8; 32], guardians, 1, "Test".to_string(), None)
        .await?;

    // Verify pending
    assert!(recovery.is_pending(&request.recovery_id).await);

    // Cancel - note: success is false because cancellation means the recovery ceremony failed
    let result = recovery
        .cancel(&request.recovery_id, "Changed my mind".to_string())
        .await?;
    // Cancellation sets success to false (recovery did not complete successfully)
    assert!(!result.success);
    assert_eq!(result.error, Some("Changed my mind".to_string()));

    // Verify no longer pending
    assert!(!recovery.is_pending(&request.recovery_id).await);
    Ok(())
}

#[tokio::test]
async fn test_list_active_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([108u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let recovery = agent.recovery().await?;

    // Initially empty
    let active = recovery.list_active().await;
    assert!(active.is_empty());

    // Create recoveries
    let guardians = vec![
        AuthorityId::new_from_entropy([109u8; 32]),
        AuthorityId::new_from_entropy([110u8; 32]),
    ];

    recovery
        .add_device(
            vec![0u8; 32],
            guardians.clone(),
            2,
            "Test 1".to_string(),
            None,
        )
        .await?;

    recovery
        .remove_device(0, guardians, 2, "Test 2".to_string(), None)
        .await?;

    // Should have 2 active
    let active = recovery.list_active().await;
    assert_eq!(active.len(), 2);
    Ok(())
}

#[tokio::test]
async fn test_get_state_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([111u8; 32]);
    let ctx = test_context(authority_id);
    let agent = AgentBuilder::new()
        .with_authority(authority_id)
        .build_testing_async(&ctx)
        .await?;

    let recovery = agent.recovery().await?;

    let guardians = vec![AuthorityId::new_from_entropy([112u8; 32])];

    let request = recovery
        .add_device(vec![0u8; 32], guardians, 1, "Test".to_string(), None)
        .await?;

    // Should be able to retrieve state
    let state = match recovery.get_state(&request.recovery_id).await {
        Some(state) => state,
        None => return Err("Recovery state should exist".into()),
    };

    match state {
        RecoveryState::Initiated { .. } => {}
        _ => panic!("Expected Initiated state"),
    }

    // Non-existent recovery should return None
    let non_existent = recovery.get_state("non-existent-id").await;
    assert!(non_existent.is_none());
    Ok(())
}
