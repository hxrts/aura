use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::{AccountId, AuraError, DeviceId};
use aura_protocol::effects::AuraEffectSystem;
use aura_recovery::{
    guardian_recovery::{
        guardian_from_device, GuardianRecoveryRequest, RecoveryPriority,
        DEFAULT_DISPUTE_WINDOW_SECS,
    },
    GuardianSet, RecoveryChoreography, RecoveryRole,
};

fn sample_context() -> RecoveryContext {
    RecoveryContext {
        operation_type: RecoveryOperationType::DeviceKeyRecovery,
        justification: "lost primary device".to_string(),
        is_emergency: false,
        timestamp: 0,
    }
}

fn guardian_set(count: usize) -> GuardianSet {
    let guardians = (0..count)
        .map(|_| guardian_from_device(DeviceId::new(), "test-guardian"))
        .collect();
    GuardianSet::new(guardians)
}

fn sample_request(guardians: GuardianSet, threshold: usize) -> GuardianRecoveryRequest {
    GuardianRecoveryRequest {
        requesting_device: DeviceId::new(),
        account_id: AccountId::new(),
        recovery_context: sample_context(),
        required_threshold: threshold,
        available_guardians: guardians,
        priority: RecoveryPriority::Normal,
        dispute_window_secs: DEFAULT_DISPUTE_WINDOW_SECS,
    }
}

#[tokio::test]
async fn guardian_recovery_happy_path() {
    let effects = AuraEffectSystem::for_testing(DeviceId::new());
    let guardians = guardian_set(3);
    let request = sample_request(guardians.clone(), 2);
    let mut choreography = RecoveryChoreography::new(
        RecoveryRole::RecoveringDevice(request.requesting_device),
        guardians,
        2,
        effects,
    );

    let result = choreography
        .execute_recovery(request)
        .await
        .expect("recovery should succeed");

    assert_eq!(result.guardian_shares.len(), 2);
    assert_eq!(
        result.threshold_signature.signers.len(),
        result.guardian_shares.len()
    );
}

#[tokio::test]
async fn guardian_cooldown_blocks_immediate_retry() {
    let effects = AuraEffectSystem::for_testing(DeviceId::new());
    let guardians = guardian_set(2);
    let request = sample_request(guardians.clone(), 1);

    let mut choreography = RecoveryChoreography::new(
        RecoveryRole::RecoveringDevice(request.requesting_device),
        guardians.clone(),
        1,
        effects.clone(),
    );
    choreography
        .execute_recovery(request.clone())
        .await
        .expect("initial recovery succeeds");

    let mut follow_up = RecoveryChoreography::new(
        RecoveryRole::RecoveringDevice(request.requesting_device),
        guardians,
        1,
        effects,
    );

    let err = follow_up
        .execute_recovery(request)
        .await
        .expect_err("guardian cooldown should prevent immediate follow-up recovery");

    assert!(matches!(err, AuraError::PermissionDenied { .. }));
}

#[tokio::test]
async fn guardian_role_can_approve() {
    let effects = AuraEffectSystem::for_testing(DeviceId::new());
    let guardians = guardian_set(1);
    let guardian_device = guardians
        .iter()
        .next()
        .expect("guardian profile should exist")
        .device_id;
    let request = sample_request(guardians.clone(), 1);

    let mut choreography = RecoveryChoreography::new(
        RecoveryRole::Guardian(guardian_device),
        guardians,
        1,
        effects,
    );

    let share = choreography
        .approve_as_guardian(request)
        .await
        .expect("guardian should approve");

    assert_eq!(share.guardian.device_id, guardian_device);
}
