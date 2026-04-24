//! Recovery Service Integration Tests
//!
//! Tests for the RecoveryServiceApi public API exposed through AuraAgent.
#![allow(clippy::uninlined_format_args)] // Test code uses explicit format args for clarity

use std::future::Future;

use aura_agent::core::config::StorageConfig;
use aura_agent::core::{AgentConfig, AuthorityContext};
use aura_agent::handlers::RecoveryHandler;
use aura_agent::{
    recovery_guardian_public_key_storage_key, AgentBuilder, AuraEffectSystem, AuthorityId,
    EffectContext, ExecutionMode, GuardianApproval, RecoveryState,
};
use aura_core::effects::{CryptoCoreEffects, StorageCoreEffects};
use aura_core::hash::hash;
use aura_core::types::identifiers::{ContextId, DeviceId, RecoveryId};
use aura_recovery::recovery_approval::{
    recovery_operation_hash, RecoveryApprovalTranscript, RecoveryApprovalTranscriptPayload,
};

/// Create a test effect context for async tests
fn test_context(authority_id: AuthorityId) -> EffectContext {
    let context_entropy = hash(&authority_id.to_bytes());
    EffectContext::new(
        authority_id,
        ContextId::new_from_entropy(context_entropy),
        ExecutionMode::Testing,
    )
}

fn isolated_test_config() -> (tempfile::TempDir, AgentConfig) {
    let temp = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("failed to create isolated tempdir: {error}"));
    let config = AgentConfig {
        storage: StorageConfig {
            base_path: temp.path().join("aura"),
            ..Default::default()
        },
        ..Default::default()
    };
    (temp, config)
}

async fn run_in_local_set<F>(future: F) -> F::Output
where
    F: Future,
{
    tokio::task::LocalSet::new().run_until(future).await
}

async fn signed_test_approval(
    effects: &AuraEffectSystem,
    request: &aura_agent::RecoveryRequest,
    guardian_id: AuthorityId,
    approved_at: u64,
) -> GuardianApproval {
    let (private_key, public_key) = effects
        .ed25519_generate_keypair()
        .await
        .unwrap_or_else(|error| panic!("failed to generate test guardian keypair: {error}"));
    effects
        .store(
            &recovery_guardian_public_key_storage_key(guardian_id),
            public_key,
        )
        .await
        .unwrap_or_else(|error| panic!("failed to store guardian public key: {error}"));
    let operation_hash = recovery_operation_hash(&request.operation)
        .unwrap_or_else(|error| panic!("failed to hash recovery operation: {error}"));
    let transcript = RecoveryApprovalTranscript::new(RecoveryApprovalTranscriptPayload {
        recovery_id: request.recovery_id.clone(),
        account_authority: request.account_authority,
        operation_hash,
        prestate_hash: request.prestate_hash,
        approved: true,
        approved_at_ms: approved_at,
        guardian_id,
    });
    let signature = aura_signature::sign_ed25519_transcript(effects, &transcript, &private_key)
        .await
        .unwrap_or_else(|error| panic!("failed to sign guardian approval transcript: {error}"));
    GuardianApproval {
        recovery_id: request.recovery_id.clone(),
        guardian_id,
        signature,
        share_data: None,
        approved_at,
        prestate_hash: request.prestate_hash,
    }
}

#[tokio::test]
async fn test_recovery_service_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_in_local_set(async move {
        let authority_id = AuthorityId::new_from_entropy([90u8; 32]);
        let ctx = test_context(authority_id);
        let (_temp, config) = isolated_test_config();
        let agent = AgentBuilder::new()
            .with_config(config)
            .with_authority(authority_id)
            .build_testing_async(&ctx)
            .await?;

        let recovery = agent.recovery()?;

        // Initially no active recoveries
        let active = recovery.list_active().await;
        assert!(active.is_empty());
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_add_device_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_in_local_set(async move {
        let authority_id = AuthorityId::new_from_entropy([91u8; 32]);
        let ctx = test_context(authority_id);
        let (_temp, config) = isolated_test_config();
        let agent = AgentBuilder::new()
            .with_config(config)
            .with_authority(authority_id)
            .build_testing_async(&ctx)
            .await?;

        let recovery = agent.recovery()?;

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

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
        assert_eq!(request.threshold, 2);
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_remove_device_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_in_local_set(async move {
        let authority_id = AuthorityId::new_from_entropy([94u8; 32]);
        let ctx = test_context(authority_id);
        let (_temp, config) = isolated_test_config();
        let agent = AgentBuilder::new()
            .with_config(config)
            .with_authority(authority_id)
            .build_testing_async(&ctx)
            .await?;

        let recovery = agent.recovery()?;

        let guardians = vec![AuthorityId::new_from_entropy([95u8; 32])];

        let request = recovery
            .remove_device(0, guardians, 1, "Device compromised".to_string(), None)
            .await?;

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_replace_tree_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_in_local_set(async move {
        let authority_id = AuthorityId::new_from_entropy([96u8; 32]);
        let ctx = test_context(authority_id);
        let (_temp, config) = isolated_test_config();
        let agent = AgentBuilder::new()
            .with_config(config)
            .with_authority(authority_id)
            .build_testing_async(&ctx)
            .await?;

        let recovery = agent.recovery()?;

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

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
        assert!(request.expires_at.is_some());
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_update_guardians_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_in_local_set(async move {
        let authority_id = AuthorityId::new_from_entropy([100u8; 32]);
        let ctx = test_context(authority_id);
        let (_temp, config) = isolated_test_config();
        let agent = AgentBuilder::new()
            .with_config(config)
            .with_authority(authority_id)
            .build_testing_async(&ctx)
            .await?;

        let recovery = agent.recovery()?;

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

        assert!(request.recovery_id.as_str().starts_with("recovery-"));
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_full_recovery_flow_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_in_local_set(async move {
        let authority_id = AuthorityId::new_from_entropy([104u8; 32]);
        let ctx = test_context(authority_id);
        let (_temp, config) = isolated_test_config();
        let agent = AgentBuilder::new()
            .with_config(config)
            .with_authority(authority_id)
            .build_testing_async(&ctx)
            .await?;

        let recovery = agent.recovery()?;

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
        let effects = agent.runtime().effects();
        let approval = signed_test_approval(&effects, &request, guardians[0], 12345).await;
        recovery.submit_approval(approval).await?;

        // Complete
        let result = recovery.complete(&request.recovery_id).await?;
        assert!(result.success);
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_cancel_recovery_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_in_local_set(async move {
        let authority_id = AuthorityId::new_from_entropy([106u8; 32]);
        let ctx = test_context(authority_id);
        let (_temp, config) = isolated_test_config();
        let agent = AgentBuilder::new()
            .with_config(config)
            .with_authority(authority_id)
            .build_testing_async(&ctx)
            .await?;

        let recovery = agent.recovery()?;

        let guardians = vec![AuthorityId::new_from_entropy([107u8; 32])];

        // Initiate
        let request = recovery
            .add_device(vec![0u8; 32], guardians, 1, "Test".to_string(), None)
            .await?;

        // Verify pending
        assert!(recovery.is_pending(&request.recovery_id).await);

        // Cancel - note: success is false because cancellation means the
        // recovery ceremony failed.
        let result = recovery
            .cancel(&request.recovery_id, "Changed my mind".to_string())
            .await?;
        // Cancellation sets success to false (recovery did not complete successfully)
        assert!(!result.success);
        assert_eq!(result.error, Some("Changed my mind".to_string()));

        // Verify no longer pending
        assert!(!recovery.is_pending(&request.recovery_id).await);
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_list_active_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_in_local_set(async move {
        let authority_id = AuthorityId::new_from_entropy([108u8; 32]);
        let ctx = test_context(authority_id);
        let (_temp, config) = isolated_test_config();
        let agent = AgentBuilder::new()
            .with_config(config)
            .with_authority(authority_id)
            .build_testing_async(&ctx)
            .await?;

        let recovery = agent.recovery()?;

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
    })
    .await
}

#[tokio::test]
async fn test_get_state_via_agent() -> Result<(), Box<dyn std::error::Error>> {
    run_in_local_set(async move {
        let authority_id = AuthorityId::new_from_entropy([111u8; 32]);
        let ctx = test_context(authority_id);
        let (_temp, config) = isolated_test_config();
        let agent = AgentBuilder::new()
            .with_config(config)
            .with_authority(authority_id)
            .build_testing_async(&ctx)
            .await?;

        let recovery = agent.recovery()?;

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
        let non_existent = recovery
            .get_state(&RecoveryId::new("non-existent-id"))
            .await;
        assert!(non_existent.is_none());
        Ok(())
    })
    .await
}

#[tokio::test]
async fn test_recovery_execution_requires_consensus_in_production(
) -> Result<(), Box<dyn std::error::Error>> {
    let authority_id = AuthorityId::new_from_entropy([120u8; 32]);
    let mut config = AgentConfig::default();
    config.storage.base_path =
        std::env::temp_dir().join(format!("aura-test-recovery-{}", authority_id));

    let effects = AuraEffectSystem::production_for_authority(config, authority_id)?;
    let device_id = DeviceId::new_from_entropy([121u8; 32]);
    let handler = RecoveryHandler::new(AuthorityContext::new_with_device(authority_id, device_id))?;

    let err = handler
        .complete(&effects, &RecoveryId::new("recovery-missing"))
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("consensus finalization"),
        "Expected consensus finalization error, got: {err}"
    );

    Ok(())
}
