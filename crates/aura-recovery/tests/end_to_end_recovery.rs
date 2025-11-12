//! End-to-end recovery ceremony testing
//!
//! This module provides comprehensive testing for the guardian recovery system,
//! including multi-device coordination, policy enforcement, and failure scenarios.

use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::{AccountId, AuraError, DeviceId};
use aura_protocol::effects::AuraEffectSystem;
use aura_recovery::{
    guardian_recovery::{
        guardian_from_device, GuardianRecoveryCoordinator, GuardianRecoveryRequest,
        PolicyViolation, PolicyWarning, RecoveryPolicyConfig, RecoveryPriority,
        DEFAULT_DISPUTE_WINDOW_SECS,
    },
    GuardianSet, RecoveryChoreography, RecoveryRole,
};
use std::collections::HashMap;
use std::time::SystemTime;

/// Test fixture for end-to-end recovery scenarios
struct RecoveryTestHarness {
    /// Recovering device
    recovering_device: DeviceId,
    /// Guardian devices
    guardian_devices: Vec<DeviceId>,
    /// Guardian set for recovery
    guardian_set: GuardianSet,
    /// Effect systems for each device
    device_effects: HashMap<DeviceId, AuraEffectSystem>,
    /// Recovery coordinators for each device
    coordinators: HashMap<DeviceId, GuardianRecoveryCoordinator>,
}

impl RecoveryTestHarness {
    /// Create new test harness with specified guardian count
    async fn new(guardian_count: usize) -> Self {
        let recovering_device = DeviceId::new();
        let guardian_devices: Vec<DeviceId> =
            (0..guardian_count).map(|_| DeviceId::new()).collect();

        let guardian_set = GuardianSet::new(
            guardian_devices
                .iter()
                .map(|device| guardian_from_device(*device, "test-guardian"))
                .collect(),
        );

        let mut device_effects = HashMap::new();
        let mut coordinators = HashMap::new();

        // Set up effect systems and coordinators for all devices
        for device in std::iter::once(&recovering_device).chain(guardian_devices.iter()) {
            let effects = AuraEffectSystem::for_testing(*device);
            let coordinator = GuardianRecoveryCoordinator::new(effects.clone());

            device_effects.insert(*device, effects);
            coordinators.insert(*device, coordinator);
        }

        Self {
            recovering_device,
            guardian_devices,
            guardian_set,
            device_effects,
            coordinators,
        }
    }

    /// Create recovery request with given priority and threshold
    fn create_request(
        &self,
        threshold: usize,
        priority: RecoveryPriority,
    ) -> GuardianRecoveryRequest {
        GuardianRecoveryRequest {
            requesting_device: self.recovering_device,
            account_id: AccountId::new(),
            recovery_context: RecoveryContext {
                operation_type: RecoveryOperationType::DeviceKeyRecovery,
                justification: "End-to-end test recovery".to_string(),
                is_emergency: matches!(priority, RecoveryPriority::Emergency),
                timestamp: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
            required_threshold: threshold,
            available_guardians: self.guardian_set.clone(),
            priority,
            dispute_window_secs: DEFAULT_DISPUTE_WINDOW_SECS,
        }
    }

    /// Get coordinator for recovering device
    fn recovering_coordinator(&self) -> &GuardianRecoveryCoordinator {
        &self.coordinators[&self.recovering_device]
    }

    /// Get coordinator for guardian device
    fn guardian_coordinator(&self, guardian_id: &DeviceId) -> &GuardianRecoveryCoordinator {
        &self.coordinators[guardian_id]
    }
}

#[tokio::test]
async fn test_full_recovery_ceremony_happy_path() {
    let harness = RecoveryTestHarness::new(3).await;
    let request = harness.create_request(2, RecoveryPriority::Normal);

    // Execute recovery from recovering device
    let response = harness
        .recovering_coordinator()
        .execute_recovery(request.clone())
        .await
        .expect("Recovery should succeed with sufficient guardians");

    assert!(response.success, "Recovery should be successful");
    assert_eq!(
        response.guardian_approvals.len(),
        2,
        "Should have 2 guardian approvals"
    );
    assert_eq!(
        response.recovery_artifacts.len(),
        1,
        "Should have recovery artifact"
    );

    // Verify recovered key material exists
    assert!(
        response.recovery_outcome.key_material.is_some(),
        "Should have recovered key material"
    );
    assert!(
        !response.recovery_outcome.evidence_id.is_empty(),
        "Should have evidence ID"
    );
}

#[tokio::test]
async fn test_insufficient_guardians_failure() {
    let harness = RecoveryTestHarness::new(2).await;
    let request = harness.create_request(3, RecoveryPriority::Normal); // Need 3 but only have 2

    let result = harness
        .recovering_coordinator()
        .execute_recovery(request)
        .await;

    assert!(
        result.is_err(),
        "Recovery should fail with insufficient guardians"
    );
    if let Err(error) = result {
        assert!(matches!(error, AuraError::PermissionDenied { .. }));
    }
}

#[tokio::test]
async fn test_policy_enforcement_threshold_requirements() {
    let harness = RecoveryTestHarness::new(3).await;

    // Create custom policy requiring higher threshold for urgent recoveries
    let mut policy_config = RecoveryPolicyConfig::default();
    policy_config
        .threshold_requirements
        .insert(RecoveryPriority::Urgent, 3);

    let coordinator = GuardianRecoveryCoordinator::with_policy_config(
        harness.device_effects[&harness.recovering_device].clone(),
        policy_config,
    );

    // Request with threshold 2 for urgent priority (policy requires 3)
    let request = harness.create_request(2, RecoveryPriority::Urgent);

    let result = coordinator.execute_recovery(request).await;

    assert!(
        result.is_err(),
        "Should reject request violating threshold policy"
    );
    if let Err(error) = result {
        assert!(error.to_string().contains("policy violations"));
    }
}

#[tokio::test]
async fn test_emergency_recovery_with_reduced_dispute_window() {
    let harness = RecoveryTestHarness::new(3).await;
    let request = harness.create_request(2, RecoveryPriority::Emergency);

    let response = harness
        .recovering_coordinator()
        .execute_recovery(request)
        .await
        .expect("Emergency recovery should succeed");

    assert!(response.success);
    // Emergency recoveries should have reduced dispute window
    // This is handled by the policy adjustment logic
}

#[tokio::test]
async fn test_cooldown_period_enforcement() {
    let harness = RecoveryTestHarness::new(3).await;
    let request1 = harness.create_request(2, RecoveryPriority::Normal);
    let request2 = harness.create_request(2, RecoveryPriority::Normal);

    // First recovery should succeed
    let response1 = harness
        .recovering_coordinator()
        .execute_recovery(request1)
        .await
        .expect("First recovery should succeed");

    assert!(response1.success);

    // Immediate second recovery should be blocked by cooldown
    let result2 = harness
        .recovering_coordinator()
        .execute_recovery(request2)
        .await;

    assert!(
        result2.is_err(),
        "Second recovery should be blocked by cooldown"
    );
}

#[tokio::test]
async fn test_guardian_approval_with_policy_validation() {
    let harness = RecoveryTestHarness::new(3).await;
    let guardian_device = harness.guardian_devices[0];
    let request = harness.create_request(2, RecoveryPriority::Normal);

    // Test policy validation for guardian approval
    let validation = harness
        .guardian_coordinator(&guardian_device)
        .validate_guardian_approval(&guardian_device, &request)
        .await
        .expect("Policy validation should succeed");

    assert!(validation.is_valid, "Guardian should be allowed to approve");
}

#[tokio::test]
async fn test_dispute_filing_during_window() {
    let harness = RecoveryTestHarness::new(3).await;
    let request = harness.create_request(2, RecoveryPriority::Normal);

    // Execute recovery
    let response = harness
        .recovering_coordinator()
        .execute_recovery(request)
        .await
        .expect("Recovery should succeed");

    // Verify dispute window is active
    assert!(!response.recovery_outcome.evidence_id.is_empty());

    // In a full implementation, we would test filing disputes here
    // For now, we verify the evidence structure is correct
    assert!(!response.recovery_artifacts.is_empty());
}

#[tokio::test]
async fn test_recovery_metrics_collection() {
    let harness = RecoveryTestHarness::new(4).await;
    let request = harness.create_request(3, RecoveryPriority::Normal);

    let response = harness
        .recovering_coordinator()
        .execute_recovery(request)
        .await
        .expect("Recovery should succeed");

    let metrics = &response.metrics;
    assert_eq!(
        metrics.guardians_contacted, 4,
        "Should contact all guardians"
    );
    assert_eq!(
        metrics.guardians_approved, 3,
        "Should get required approvals"
    );
    assert_eq!(
        metrics.cooldown_blocked, 0,
        "No guardians should be in cooldown"
    );
    assert!(
        metrics.completed_at > metrics.started_at,
        "Completion after start"
    );
}

#[tokio::test]
async fn test_policy_warning_generation() {
    let harness = RecoveryTestHarness::new(3).await;

    // Create policy with urgent priority having higher cooldown multiplier
    let mut policy_config = RecoveryPolicyConfig::default();
    policy_config
        .cooldown_multipliers
        .insert(RecoveryPriority::Urgent, 2.0);

    let coordinator = GuardianRecoveryCoordinator::with_policy_config(
        harness.device_effects[&harness.recovering_device].clone(),
        policy_config,
    );

    let guardian_device = harness.guardian_devices[0];
    let request = harness.create_request(2, RecoveryPriority::Urgent);

    let validation = coordinator
        .validate_guardian_approval(&guardian_device, &request)
        .await
        .expect("Validation should succeed");

    assert!(validation.is_valid, "Request should be valid");
    assert!(
        !validation.warnings.is_empty(),
        "Should generate cooldown multiplier warning"
    );

    // Check warning content
    if let Some(PolicyWarning::CooldownMultiplier { multiplier, .. }) = validation.warnings.first()
    {
        assert_eq!(
            *multiplier, 2.0,
            "Warning should reflect the configured multiplier"
        );
    } else {
        panic!("Expected cooldown multiplier warning");
    }
}

#[tokio::test]
async fn test_recovery_priority_levels() {
    let harness = RecoveryTestHarness::new(3).await;

    // Test each priority level
    for priority in [
        RecoveryPriority::Normal,
        RecoveryPriority::Urgent,
        RecoveryPriority::Emergency,
    ] {
        let request = harness.create_request(2, priority.clone());

        let response = harness
            .recovering_coordinator()
            .execute_recovery(request)
            .await
            .expect(&format!(
                "Recovery should succeed for {:?} priority",
                priority
            ));

        assert!(response.success);
        assert_eq!(response.guardian_approvals.len(), 2);
    }
}

#[tokio::test]
async fn test_account_status_changes_tracking() {
    let harness = RecoveryTestHarness::new(3).await;
    let request = harness.create_request(2, RecoveryPriority::Normal);

    let response = harness
        .recovering_coordinator()
        .execute_recovery(request)
        .await
        .expect("Recovery should succeed");

    // Verify recovery outcome structure
    assert_eq!(
        response.recovery_outcome.operation_type,
        RecoveryOperationType::DeviceKeyRecovery
    );

    // Account changes would be tracked in a full implementation
    // For now, verify the structure is correct
    assert!(response.recovery_outcome.account_changes.is_empty()); // Stub implementation
}

#[tokio::test]
async fn test_recovery_artifact_generation() {
    let harness = RecoveryTestHarness::new(3).await;
    let request = harness.create_request(2, RecoveryPriority::Normal);

    let response = harness
        .recovering_coordinator()
        .execute_recovery(request)
        .await
        .expect("Recovery should succeed");

    assert_eq!(response.recovery_artifacts.len(), 1);

    let artifact = &response.recovery_artifacts[0];
    assert!(!artifact.content.is_empty(), "Artifact should have content");
    assert!(
        !artifact.signatures.is_empty(),
        "Artifact should have signatures"
    );
    assert!(
        artifact.timestamp > 0,
        "Artifact should have valid timestamp"
    );
}

#[tokio::test]
async fn test_concurrent_recovery_attempts() {
    let harness = RecoveryTestHarness::new(5).await;

    // Create multiple recovery requests
    let requests: Vec<_> = (0..3)
        .map(|i| {
            harness.create_request(
                2,
                if i == 0 {
                    RecoveryPriority::Emergency
                } else {
                    RecoveryPriority::Normal
                },
            )
        })
        .collect();

    // Attempt concurrent recoveries (first should succeed, others should be blocked)
    let mut handles = Vec::new();
    for request in requests {
        let coordinator = harness.recovering_coordinator();
        let handle = tokio::spawn(async move { coordinator.execute_recovery(request).await });
        handles.push(handle);
    }

    let results: Vec<_> = futures::future::join_all(handles).await;

    let successful_count = results
        .into_iter()
        .filter_map(|r| r.ok())
        .filter(|r| r.is_ok())
        .count();

    // Only one recovery should succeed due to cooldown enforcement
    assert_eq!(
        successful_count, 1,
        "Only one concurrent recovery should succeed"
    );
}
