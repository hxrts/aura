//! End-to-end recovery ceremony testing
//!
//! Tests for the simplified guardian recovery choreographies.

#![allow(clippy::disallowed_methods)]
#![allow(clippy::expect_used)]

use aura_agent::runtime::AuraEffectSystem;
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::{identifiers::GuardianId, AccountId, DeviceId, TrustLevel};
use aura_macros::aura_test;
use aura_protocol::{authorization::BiscuitAuthorizationBridge, guards::BiscuitGuardEvaluator};
use aura_recovery::{
    GuardianKeyRecoveryCoordinator, GuardianMembershipCoordinator, GuardianProfile, GuardianSet,
    GuardianSetupCoordinator, MembershipChange, MembershipChangeRequest, RecoveryRequest,
};
use aura_wot::{AccountAuthority, BiscuitTokenManager};
use std::time::SystemTime;

/// Helper to create guardian profile
fn create_guardian(device_id: DeviceId, label: &str) -> GuardianProfile {
    GuardianProfile {
        guardian_id: GuardianId::new(),
        device_id,
        label: label.to_string(),
        trust_level: TrustLevel::High,
        cooldown_secs: 900,
    }
}

/// Helper to create recovery context
fn create_recovery_context() -> RecoveryContext {
    RecoveryContext {
        operation_type: RecoveryOperationType::DeviceKeyRecovery,
        justification: "End-to-end test recovery".to_string(),
        is_emergency: true, // All recoveries are emergency in simplified model
        timestamp: SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .expect("System time should be after UNIX_EPOCH")
            .as_secs(),
    }
}

/// Helper to create Biscuit token manager and evaluator for testing
fn create_biscuit_components(
    device_id: DeviceId,
    account_id: AccountId,
) -> (BiscuitTokenManager, BiscuitGuardEvaluator) {
    let authority = AccountAuthority::new(account_id);
    let token = authority
        .create_device_token(device_id)
        .expect("Failed to create device token");
    let token_manager = BiscuitTokenManager::new(device_id, token);

    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), device_id);
    let guard_evaluator = BiscuitGuardEvaluator::new(bridge);

    (token_manager, guard_evaluator)
}

#[aura_test]
async fn test_guardian_setup() -> aura_core::AuraResult<()> {
    let fixture = aura_testkit::create_test_fixture().await?;
    let device_id = DeviceId::new();
    let account_id = AccountId::new();
    let (token_manager, guard_evaluator) = create_biscuit_components(device_id, account_id);
    let coordinator = GuardianSetupCoordinator::new_with_biscuit(
        fixture.effect_system().into(),
        token_manager,
        guard_evaluator,
    );

    // Create guardian set
    let guardians = vec![
        create_guardian(DeviceId::new(), "Guardian 1"),
        create_guardian(DeviceId::new(), "Guardian 2"),
        create_guardian(DeviceId::new(), "Guardian 3"),
    ];
    let guardian_set = GuardianSet::new(guardians);

    // Create setup request
    let request = RecoveryRequest {
        requesting_device: device_id,
        account_id,
        context: create_recovery_context(),
        threshold: 2,
        guardians: guardian_set,
        auth_token: None, // Let the coordinator use its own token
    };

    // Execute setup
    let response = coordinator.execute_setup(request).await?;

    assert!(response.success, "Setup should be successful");
    assert_eq!(
        response.guardian_shares.len(),
        2,
        "Should have 2 guardian shares"
    );
    assert!(response.error.is_none(), "Should have no error");
    Ok(())
}

#[aura_test]
async fn test_guardian_key_recovery() -> aura_core::AuraResult<()> {
    let fixture = aura_testkit::create_test_fixture().await?;
    let device_id = DeviceId::new();
    let account_id = AccountId::new();
    let (token_manager, guard_evaluator) = create_biscuit_components(device_id, account_id);
    let coordinator = GuardianKeyRecoveryCoordinator::new_with_biscuit(
        fixture.effect_system().into(),
        token_manager,
        guard_evaluator,
    );

    // Create guardian set
    let guardians = vec![
        create_guardian(DeviceId::new(), "Guardian 1"),
        create_guardian(DeviceId::new(), "Guardian 2"),
        create_guardian(DeviceId::new(), "Guardian 3"),
    ];
    let guardian_set = GuardianSet::new(guardians);

    // Create recovery request
    let request = RecoveryRequest {
        requesting_device: device_id,
        account_id,
        context: create_recovery_context(),
        threshold: 2,
        guardians: guardian_set,
        auth_token: None, // Let the coordinator use its own token
    };

    // Execute key recovery
    let response = coordinator.execute_key_recovery(request).await?;

    assert!(response.success, "Recovery should be successful");
    assert_eq!(
        response.guardian_shares.len(),
        2,
        "Should have 2 guardian shares"
    );
    assert!(
        response.key_material.is_some(),
        "Should have recovered key material"
    );
    assert!(response.error.is_none(), "Should have no error");
    Ok(())
}

#[aura_test]
async fn test_guardian_membership_add() -> aura_core::AuraResult<()> {
    let fixture = aura_testkit::create_test_fixture().await?;
    let device_id = DeviceId::new();
    let account_id = AccountId::new();
    let (token_manager, guard_evaluator) = create_biscuit_components(device_id, account_id);
    let coordinator = GuardianMembershipCoordinator::new_with_biscuit(
        fixture.effect_system().into(),
        token_manager,
        guard_evaluator,
    );

    // Create initial guardian set
    let initial_guardians = vec![
        create_guardian(DeviceId::new(), "Guardian 1"),
        create_guardian(DeviceId::new(), "Guardian 2"),
    ];
    let guardian_set = GuardianSet::new(initial_guardians);

    // Create new guardian to add
    let new_guardian = create_guardian(DeviceId::new(), "Guardian 3");

    // Create membership change request
    let request = MembershipChangeRequest {
        base: RecoveryRequest {
            requesting_device: device_id,
            account_id,
            context: create_recovery_context(),
            threshold: 2,
            guardians: guardian_set,
            auth_token: None, // Let the coordinator use its own token
        },
        change: MembershipChange::AddGuardian {
            guardian: new_guardian,
        },
        new_threshold: Some(2), // Keep threshold the same
    };

    // Execute membership change
    let response = coordinator.execute_membership_change(request).await?;

    assert!(response.success, "Membership change should be successful");
    assert_eq!(
        response.guardian_shares.len(),
        2,
        "Should have 2 approval shares"
    );
    assert!(response.error.is_none(), "Should have no error");
    Ok(())
}

#[aura_test]
async fn test_guardian_membership_remove() -> aura_core::AuraResult<()> {
    let fixture = aura_testkit::create_test_fixture().await?;
    let device_id = DeviceId::new();
    let account_id = AccountId::new();
    let (token_manager, guard_evaluator) = create_biscuit_components(device_id, account_id);
    let coordinator = GuardianMembershipCoordinator::new_with_biscuit(
        fixture.effect_system().into(),
        token_manager,
        guard_evaluator,
    );

    // Create initial guardian set
    let guardians = vec![
        create_guardian(DeviceId::new(), "Guardian 1"),
        create_guardian(DeviceId::new(), "Guardian 2"),
        create_guardian(DeviceId::new(), "Guardian 3"),
    ];
    let guardian_to_remove = guardians[2].guardian_id; // Remove third guardian
    let guardian_set = GuardianSet::new(guardians);

    // Create membership change request
    let request = MembershipChangeRequest {
        base: RecoveryRequest {
            requesting_device: device_id,
            account_id,
            context: create_recovery_context(),
            threshold: 2,
            guardians: guardian_set,
            auth_token: None, // Let the coordinator use its own token
        },
        change: MembershipChange::RemoveGuardian {
            guardian_id: guardian_to_remove,
        },
        new_threshold: Some(2), // Keep threshold the same
    };

    // Execute membership change
    let response = coordinator.execute_membership_change(request).await?;

    assert!(response.success, "Membership change should be successful");
    assert_eq!(
        response.guardian_shares.len(),
        2,
        "Should have 2 approval shares"
    );
    assert!(response.error.is_none(), "Should have no error");
    Ok(())
}

#[aura_test]
async fn test_insufficient_threshold_failure() -> aura_core::AuraResult<()> {
    let fixture = aura_testkit::create_test_fixture().await?;
    let device_id = DeviceId::new();
    let account_id = AccountId::new();
    let (token_manager, guard_evaluator) = create_biscuit_components(device_id, account_id);
    let coordinator = GuardianKeyRecoveryCoordinator::new_with_biscuit(
        fixture.effect_system().into(),
        token_manager,
        guard_evaluator,
    );

    // Create guardian set with only 2 guardians
    let guardians = vec![
        create_guardian(DeviceId::new(), "Guardian 1"),
        create_guardian(DeviceId::new(), "Guardian 2"),
    ];
    let guardian_set = GuardianSet::new(guardians);

    // Request threshold of 3 (more than available guardians)
    let request = RecoveryRequest {
        requesting_device: device_id,
        account_id,
        context: create_recovery_context(),
        threshold: 3, // More than the 2 available guardians
        guardians: guardian_set,
        auth_token: None, // Let the coordinator use its own token
    };

    // Execute key recovery
    let response = coordinator.execute_key_recovery(request).await?;

    assert!(!response.success, "Recovery should fail");
    assert!(response.error.is_some(), "Should have error message");
    assert!(
        response.key_material.is_none(),
        "Should have no key material"
    );
    assert_eq!(
        response.guardian_shares.len(),
        2,
        "Should still have guardian shares"
    );
    Ok(())
}
