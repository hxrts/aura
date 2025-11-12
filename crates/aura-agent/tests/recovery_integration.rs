//! Integration tests for agent recovery operations
//!
//! These tests verify the full recovery workflow from the agent perspective,
//! including policy enforcement and multi-device coordination.
//!
//! NOTE: These tests are currently disabled as the recovery functionality
//! is not yet implemented in aura-agent handlers.

#[cfg(disabled_recovery_tests)]
mod recovery_tests {

// Note: Recovery functionality not yet implemented in aura-agent handlers  
// use aura_agent::handlers::recovery::{RecoveryOperations, RecoveryStatus};
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::{AccountId, DeviceId};
use uuid;
use aura_protocol::effects::AuraEffectSystem;
use aura_recovery::{
    guardian_recovery::{
        guardian_from_device, GuardianRecoveryRequest, RecoveryPolicyConfig, RecoveryPriority,
        DEFAULT_DISPUTE_WINDOW_SECS,
    },
    GuardianSet, PolicyViolation,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

/// Integration test fixture for recovery operations
struct RecoveryIntegrationHarness {
    /// Recovery operations for recovering device
    recovery_ops: RecoveryOperations,
    /// Recovery operations for guardian devices
    guardian_ops: Vec<RecoveryOperations>,
    /// Guardian device IDs
    guardian_devices: Vec<DeviceId>,
    /// Account ID for testing
    account_id: AccountId,
}

impl RecoveryIntegrationHarness {
    async fn new(guardian_count: usize) -> Self {
        let recovering_device = DeviceId(uuid::Uuid::new_v4());
        let guardian_devices: Vec<DeviceId> =
            (0..guardian_count).map(|_| DeviceId(uuid::Uuid::new_v4())).collect();
        let account_id = AccountId(uuid::Uuid::new_v4());

        // Create effects for recovering device
        let recovering_effects = Arc::new(RwLock::new(AuraEffectSystem::for_testing(
            recovering_device,
        )));
        let recovery_ops = RecoveryOperations::new(recovering_effects, recovering_device);

        // Create guardian operations
        let mut guardian_ops = Vec::new();
        for guardian_device in &guardian_devices {
            let guardian_effects =
                Arc::new(RwLock::new(AuraEffectSystem::for_testing(*guardian_device)));
            let ops = RecoveryOperations::new(guardian_effects, *guardian_device);
            guardian_ops.push(ops);
        }

        Self {
            recovery_ops,
            guardian_ops,
            guardian_devices,
            account_id,
        }
    }

    fn create_guardian_set(&self) -> GuardianSet {
        GuardianSet::new(
            self.guardian_devices
                .iter()
                .map(|device| guardian_from_device(*device, "integration-test"))
                .collect(),
        )
    }

    fn create_recovery_request(
        &self,
        threshold: usize,
        priority: RecoveryPriority,
    ) -> GuardianRecoveryRequest {
        GuardianRecoveryRequest {
            requesting_device: self.recovery_ops.device_id(),
            account_id: self.account_id,
            recovery_context: RecoveryContext {
                operation_type: RecoveryOperationType::DeviceKeyRecovery,
                justification: "Integration test recovery".to_string(),
                is_emergency: matches!(priority, RecoveryPriority::Emergency),
                timestamp: SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
            required_threshold: threshold,
            available_guardians: self.create_guardian_set(),
            priority,
            dispute_window_secs: DEFAULT_DISPUTE_WINDOW_SECS,
        }
    }
}

#[tokio::test]
async fn test_agent_recovery_happy_path() {
    let harness = RecoveryIntegrationHarness::new(3).await;
    let request = harness.create_recovery_request(2, RecoveryPriority::Normal);

    // Start recovery from agent perspective
    let response = harness
        .recovery_ops
        .start_guardian_recovery(request)
        .await
        .expect("Agent recovery should succeed");

    assert!(response.success, "Recovery should be successful");
    assert_eq!(
        response.guardian_approvals.len(),
        2,
        "Should have required approvals"
    );
    assert!(
        !response.recovery_outcome.evidence_id.is_empty(),
        "Should have evidence ID"
    );
}

#[tokio::test]
async fn test_agent_recovery_status_tracking() {
    let harness = RecoveryIntegrationHarness::new(3).await;

    // Check initial status
    let initial_status = harness
        .recovery_ops
        .recovery_status()
        .await
        .expect("Should get recovery status");

    assert_eq!(initial_status.pending_sessions, 0, "No sessions initially");
    assert!(
        initial_status.latest_evidence.is_none(),
        "No evidence initially"
    );

    // Start recovery
    let request = harness.create_recovery_request(2, RecoveryPriority::Normal);
    let _response = harness
        .recovery_ops
        .start_guardian_recovery(request)
        .await
        .expect("Recovery should succeed");

    // Check status after recovery
    let post_recovery_status = harness
        .recovery_ops
        .recovery_status()
        .await
        .expect("Should get recovery status");

    assert_eq!(
        post_recovery_status.pending_sessions, 0,
        "Session should be complete"
    );
    assert!(
        post_recovery_status.latest_evidence.is_some(),
        "Should have evidence"
    );
    assert!(
        post_recovery_status.latest_evidence_id.is_some(),
        "Should have evidence ID"
    );
}

#[tokio::test]
async fn test_guardian_approval_workflow() {
    let harness = RecoveryIntegrationHarness::new(3).await;
    let request = harness.create_recovery_request(1, RecoveryPriority::Normal);

    // Test guardian approval from guardian perspective
    let guardian_share = harness.guardian_ops[0]
        .approve_guardian_recovery(request)
        .await
        .expect("Guardian should approve recovery");

    assert_eq!(
        guardian_share.guardian.device_id,
        harness.guardian_devices[0]
    );
    assert!(
        !guardian_share.share.is_empty(),
        "Share should have content"
    );
    assert!(
        !guardian_share.partial_signature.is_empty(),
        "Share should have signature"
    );
}

#[tokio::test]
async fn test_policy_enforcement_in_agent_operations() {
    let harness = RecoveryIntegrationHarness::new(2).await;

    // Create policy that requires 3 guardians for urgent recovery
    let mut policy_config = RecoveryPolicyConfig::default();
    policy_config
        .threshold_requirements
        .insert(RecoveryPriority::Urgent, 3);

    let custom_recovery_ops = RecoveryOperations::with_policy_config(
        Arc::new(RwLock::new(AuraEffectSystem::for_testing(
            harness.recovery_ops.device_id(),
        ))),
        harness.recovery_ops.device_id(),
        policy_config,
    );

    // Request urgent recovery with only 2 guardians available
    let request = harness.create_recovery_request(2, RecoveryPriority::Urgent);

    let result = custom_recovery_ops.start_guardian_recovery(request).await;

    assert!(result.is_err(), "Should reject due to policy violation");

    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("policy violations"),
        "Error should mention policy violations"
    );
}

#[tokio::test]
async fn test_cooldown_enforcement_in_agent() {
    let harness = RecoveryIntegrationHarness::new(3).await;

    // First recovery
    let request1 = harness.create_recovery_request(2, RecoveryPriority::Normal);
    let response1 = harness
        .recovery_ops
        .start_guardian_recovery(request1)
        .await
        .expect("First recovery should succeed");

    assert!(response1.success);

    // Immediate second recovery should fail due to cooldown
    let request2 = harness.create_recovery_request(2, RecoveryPriority::Normal);
    let result2 = harness.recovery_ops.start_guardian_recovery(request2).await;

    assert!(
        result2.is_err(),
        "Second recovery should be blocked by cooldown"
    );
}

#[tokio::test]
async fn test_emergency_recovery_priority_handling() {
    let harness = RecoveryIntegrationHarness::new(3).await;

    // Emergency recovery should succeed even with relaxed requirements
    let emergency_request = harness.create_recovery_request(2, RecoveryPriority::Emergency);

    let response = harness
        .recovery_ops
        .start_guardian_recovery(emergency_request)
        .await
        .expect("Emergency recovery should succeed");

    assert!(response.success);
    assert_eq!(
        response.recovery_outcome.operation_type,
        RecoveryOperationType::DeviceKeyRecovery
    );

    // Verify emergency flag is set in context
    assert!(
        response.guardian_approvals.len() >= 2,
        "Should have sufficient approvals"
    );
}

#[tokio::test]
async fn test_dispute_window_in_agent_operations() {
    let harness = RecoveryIntegrationHarness::new(3).await;
    let request = harness.create_recovery_request(2, RecoveryPriority::Normal);

    let response = harness
        .recovery_ops
        .start_guardian_recovery(request)
        .await
        .expect("Recovery should succeed");

    // Verify dispute-related fields are present
    assert!(!response.recovery_outcome.evidence_id.is_empty());
    assert!(!response.recovery_artifacts.is_empty());

    // In a full implementation, we would test dispute filing here
    let status = harness
        .recovery_ops
        .recovery_status()
        .await
        .expect("Should get status");

    assert!(
        status.dispute_window_ends_at.is_some(),
        "Should have dispute window deadline"
    );
    assert!(!status.disputed, "Should not be disputed initially");
}

#[tokio::test]
async fn test_recovery_metrics_in_agent() {
    let harness = RecoveryIntegrationHarness::new(4).await;
    let request = harness.create_recovery_request(3, RecoveryPriority::Normal);

    let response = harness
        .recovery_ops
        .start_guardian_recovery(request)
        .await
        .expect("Recovery should succeed");

    // Verify metrics are properly collected
    let metrics = &response.metrics;
    assert!(metrics.guardians_contacted > 0, "Should contact guardians");
    assert_eq!(
        metrics.guardians_approved, 3,
        "Should get required approvals"
    );
    assert!(metrics.completed_at >= metrics.started_at, "Valid timing");
}

#[tokio::test]
async fn test_multiple_account_recovery_isolation() {
    let harness = RecoveryIntegrationHarness::new(3).await;

    // Create requests for different accounts
    let mut request1 = harness.create_recovery_request(2, RecoveryPriority::Normal);
    let account2 = AccountId(uuid::Uuid::new_v4());
    let mut request2 = harness.create_recovery_request(2, RecoveryPriority::Normal);
    request2.account_id = account2;

    // Both recoveries should succeed independently
    let response1 = harness
        .recovery_ops
        .start_guardian_recovery(request1)
        .await
        .expect("First account recovery should succeed");

    let response2 = harness
        .recovery_ops
        .start_guardian_recovery(request2)
        .await
        .expect("Second account recovery should succeed");

    assert!(response1.success);
    assert!(response2.success);

    // Evidence IDs should be different
    assert_ne!(
        response1.recovery_outcome.evidence_id, response2.recovery_outcome.evidence_id,
        "Evidence IDs should be unique per account"
    );
}

#[tokio::test]
async fn test_recovery_artifact_validation() {
    let harness = RecoveryIntegrationHarness::new(3).await;
    let request = harness.create_recovery_request(2, RecoveryPriority::Normal);

    let response = harness
        .recovery_ops
        .start_guardian_recovery(request)
        .await
        .expect("Recovery should succeed");

    // Verify artifact structure
    assert_eq!(
        response.recovery_artifacts.len(),
        1,
        "Should have one artifact"
    );

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

    // Verify artifact type
    use aura_recovery::guardian_recovery::ArtifactType;
    assert!(matches!(
        artifact.artifact_type,
        ArtifactType::RecoveryAuthorization
    ));
}

#[tokio::test]
async fn test_concurrent_agent_operations() {
    let harness = RecoveryIntegrationHarness::new(5).await;

    // Create multiple concurrent operations
    let operations = vec![
        harness.recovery_ops.recovery_status(),
        harness.recovery_ops.recovery_status(),
        harness.recovery_ops.recovery_status(),
    ];

    // All status operations should succeed concurrently
    let results = futures::future::join_all(operations).await;

    for result in results {
        assert!(
            result.is_ok(),
            "Concurrent status operations should succeed"
        );
    }
}

} // End of recovery_tests module
