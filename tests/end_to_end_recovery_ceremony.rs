//! End-to-end recovery ceremony validation tests
//!
//! These tests validate the complete recovery ceremony flow across the entire system,
//! from initial request through guardian approval to final key recovery.

use aura_agent::handlers::recovery::RecoveryOperations;
use aura_authenticate::guardian_auth::{RecoveryContext, RecoveryOperationType};
use aura_core::{AccountId, DeviceId};
use aura_protocol::effects::AuraEffectSystem;
use aura_recovery::{
    guardian_recovery::{
        guardian_from_device, GuardianRecoveryRequest, RecoveryPriority,
        DEFAULT_DISPUTE_WINDOW_SECS,
    },
    GuardianSet,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;

/// Complete system test for guardian recovery ceremony
struct SystemRecoveryTest {
    /// The device being recovered
    recovering_device: DeviceId,
    /// Guardian devices that can approve recovery
    guardian_devices: Vec<DeviceId>,
    /// Recovery operations for each device
    recovery_handlers: HashMap<DeviceId, RecoveryOperations>,
    /// Account being recovered
    account_id: AccountId,
}

impl SystemRecoveryTest {
    async fn new(num_guardians: usize) -> Self {
        let recovering_device = DeviceId::new();
        let guardian_devices: Vec<DeviceId> = (0..num_guardians)
            .map(|_| DeviceId::new())
            .collect();
        let account_id = AccountId::new();
        
        let mut recovery_handlers = HashMap::new();
        
        // Create recovery handler for recovering device
        let recovering_effects = Arc::new(RwLock::new(
            AuraEffectSystem::for_testing(recovering_device)
        ));
        let recovering_ops = RecoveryOperations::new(recovering_effects, recovering_device);
        recovery_handlers.insert(recovering_device, recovering_ops);
        
        // Create recovery handlers for each guardian
        for guardian_device in &guardian_devices {
            let guardian_effects = Arc::new(RwLock::new(
                AuraEffectSystem::for_testing(*guardian_device)
            ));
            let guardian_ops = RecoveryOperations::new(guardian_effects, *guardian_device);
            recovery_handlers.insert(*guardian_device, guardian_ops);
        }
        
        Self {
            recovering_device,
            guardian_devices,
            recovery_handlers,
            account_id,
        }
    }
    
    fn create_guardian_set(&self) -> GuardianSet {
        GuardianSet::new(
            self.guardian_devices.iter()
                .map(|device| guardian_from_device(*device, "ceremony-test"))
                .collect()
        )
    }
    
    fn create_recovery_request(&self, threshold: usize, priority: RecoveryPriority) -> GuardianRecoveryRequest {
        GuardianRecoveryRequest {
            requesting_device: self.recovering_device,
            account_id: self.account_id,
            recovery_context: RecoveryContext {
                operation_type: RecoveryOperationType::DeviceKeyRecovery,
                justification: "Complete ceremony test".to_string(),
                is_emergency: matches!(priority, RecoveryPriority::Emergency),
                timestamp: SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs(),
            },
            required_threshold: threshold,
            available_guardians: self.create_guardian_set(),
            priority,
            dispute_window_secs: DEFAULT_DISPUTE_WINDOW_SECS,
        }
    }
    
    fn recovering_ops(&self) -> &RecoveryOperations {
        &self.recovery_handlers[&self.recovering_device]
    }
    
    fn guardian_ops(&self, guardian_device: &DeviceId) -> &RecoveryOperations {
        &self.recovery_handlers[guardian_device]
    }
}

#[tokio::test]
async fn test_complete_recovery_ceremony_2_of_3() {
    let test = SystemRecoveryTest::new(3).await;
    let request = test.create_recovery_request(2, RecoveryPriority::Normal);
    
    println!("Starting complete recovery ceremony test (2-of-3)...");
    
    // Step 1: Check initial status
    let initial_status = test.recovering_ops()
        .recovery_status()
        .await
        .expect("Should get initial status");
    
    assert_eq!(initial_status.pending_sessions, 0, "No sessions initially");
    assert!(initial_status.latest_evidence.is_none(), "No evidence initially");
    
    // Step 2: Initiate recovery from recovering device
    println!("Initiating recovery from recovering device...");
    let recovery_response = test.recovering_ops()
        .start_guardian_recovery(request.clone())
        .await
        .expect("Recovery initiation should succeed");
    
    assert!(recovery_response.success, "Recovery should be successful");
    assert_eq!(recovery_response.guardian_approvals.len(), 2, "Should have 2 approvals");
    assert!(!recovery_response.recovery_outcome.evidence_id.is_empty(), "Should have evidence");
    
    // Step 3: Verify recovery artifacts
    assert_eq!(recovery_response.recovery_artifacts.len(), 1, "Should have one artifact");
    let artifact = &recovery_response.recovery_artifacts[0];
    assert!(!artifact.content.is_empty(), "Artifact should have content");
    assert!(!artifact.signatures.is_empty(), "Artifact should have signatures");
    
    // Step 4: Verify key material recovery
    assert!(
        recovery_response.recovery_outcome.key_material.is_some(),
        "Should have recovered key material"
    );
    let recovered_key = recovery_response.recovery_outcome.key_material.unwrap();
    assert!(!recovered_key.is_empty(), "Recovered key should not be empty");
    assert_eq!(recovered_key.len(), 32, "Recovered key should be 32 bytes");
    
    // Step 5: Check final status
    let final_status = test.recovering_ops()
        .recovery_status()
        .await
        .expect("Should get final status");
    
    assert_eq!(final_status.pending_sessions, 0, "No pending sessions after completion");
    assert!(final_status.latest_evidence.is_some(), "Should have evidence record");
    assert!(final_status.cooldown_expires_at.is_some(), "Should have cooldown expiration");
    assert!(final_status.dispute_window_ends_at.is_some(), "Should have dispute window");
    assert!(!final_status.disputed, "Should not be disputed initially");
    
    // Step 6: Verify metrics
    let metrics = &recovery_response.metrics;
    assert_eq!(metrics.guardians_contacted, 3, "Should contact all guardians");
    assert_eq!(metrics.guardians_approved, 2, "Should get required approvals");
    assert_eq!(metrics.cooldown_blocked, 0, "No cooldown blocks initially");
    assert!(metrics.completed_at > metrics.started_at, "Valid timing");
    
    println!("Complete recovery ceremony test passed!");
}

#[tokio::test]
async fn test_complete_ceremony_with_guardian_approval_validation() {
    let test = SystemRecoveryTest::new(3).await;
    let request = test.create_recovery_request(1, RecoveryPriority::Normal);
    
    println!("Testing guardian approval validation...");
    
    // Test that individual guardians can approve the request
    for (i, guardian_device) in test.guardian_devices.iter().enumerate() {
        let guardian_share = test.guardian_ops(guardian_device)
            .approve_guardian_recovery(request.clone())
            .await
            .expect(&format!("Guardian {} should approve", i));
        
        assert_eq!(guardian_share.guardian.device_id, *guardian_device);
        assert!(!guardian_share.share.is_empty(), "Share should have content");
        assert!(!guardian_share.partial_signature.is_empty(), "Share should have signature");
        
        println!("Guardian {} approved successfully", i);
    }
    
    println!("All guardian approvals validated!");
}

#[tokio::test]
async fn test_ceremony_cooldown_enforcement() {
    let test = SystemRecoveryTest::new(2).await;
    
    println!("Testing cooldown enforcement...");
    
    // First recovery should succeed
    let request1 = test.create_recovery_request(1, RecoveryPriority::Normal);
    let response1 = test.recovering_ops()
        .start_guardian_recovery(request1)
        .await
        .expect("First recovery should succeed");
    
    assert!(response1.success);
    println!("First recovery completed successfully");
    
    // Immediate second recovery should fail due to cooldown
    let request2 = test.create_recovery_request(1, RecoveryPriority::Normal);
    let result2 = test.recovering_ops()
        .start_guardian_recovery(request2)
        .await;
    
    assert!(result2.is_err(), "Second recovery should fail due to cooldown");
    println!("Second recovery properly blocked by cooldown");
    
    // Verify the error mentions cooldown or permission
    let error_msg = result2.unwrap_err().to_string();
    assert!(
        error_msg.to_lowercase().contains("permission") || 
        error_msg.to_lowercase().contains("cooldown"),
        "Error should mention permission or cooldown: {}", error_msg
    );
    
    println!("Cooldown enforcement test passed!");
}

#[tokio::test]
async fn test_ceremony_with_different_priorities() {
    let test = SystemRecoveryTest::new(3).await;
    
    println!("Testing ceremony with different priority levels...");
    
    // Test normal priority
    let normal_request = test.create_recovery_request(2, RecoveryPriority::Normal);
    let normal_response = test.recovering_ops()
        .start_guardian_recovery(normal_request)
        .await
        .expect("Normal priority recovery should succeed");
    
    assert!(normal_response.success);
    println!("Normal priority recovery succeeded");
    
    // Wait and test urgent priority (would normally be blocked by cooldown, 
    // but we're testing with a fresh ceremony state)
    let test_urgent = SystemRecoveryTest::new(3).await;
    let urgent_request = test_urgent.create_recovery_request(2, RecoveryPriority::Urgent);
    let urgent_response = test_urgent.recovering_ops()
        .start_guardian_recovery(urgent_request)
        .await
        .expect("Urgent priority recovery should succeed");
    
    assert!(urgent_response.success);
    println!("Urgent priority recovery succeeded");
    
    // Test emergency priority
    let test_emergency = SystemRecoveryTest::new(3).await;
    let emergency_request = test_emergency.create_recovery_request(2, RecoveryPriority::Emergency);
    let emergency_response = test_emergency.recovering_ops()
        .start_guardian_recovery(emergency_request)
        .await
        .expect("Emergency priority recovery should succeed");
    
    assert!(emergency_response.success);
    println!("Emergency priority recovery succeeded");
    
    println!("All priority levels tested successfully!");
}

#[tokio::test]
async fn test_ceremony_insufficient_threshold() {
    let test = SystemRecoveryTest::new(2).await; // Only 2 guardians
    let request = test.create_recovery_request(3, RecoveryPriority::Normal); // Need 3 approvals
    
    println!("Testing insufficient threshold handling...");
    
    let result = test.recovering_ops()
        .start_guardian_recovery(request)
        .await;
    
    assert!(result.is_err(), "Should fail with insufficient guardians");
    
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.to_lowercase().contains("permission") ||
        error_msg.to_lowercase().contains("guardian") ||
        error_msg.to_lowercase().contains("threshold"),
        "Error should mention guardians/threshold: {}", error_msg
    );
    
    println!("Insufficient threshold properly handled!");
}

#[tokio::test]
async fn test_ceremony_evidence_tracking() {
    let test = SystemRecoveryTest::new(3).await;
    let request = test.create_recovery_request(2, RecoveryPriority::Normal);
    
    println!("Testing evidence tracking...");
    
    let response = test.recovering_ops()
        .start_guardian_recovery(request)
        .await
        .expect("Recovery should succeed");
    
    // Verify evidence structure
    let evidence_id = &response.recovery_outcome.evidence_id;
    assert!(!evidence_id.is_empty(), "Should have evidence ID");
    assert!(evidence_id.contains(':'), "Evidence ID should have account:timestamp format");
    
    // Check that status reflects the evidence
    let status = test.recovering_ops()
        .recovery_status()
        .await
        .expect("Should get status");
    
    assert_eq!(
        status.latest_evidence_id.as_ref().unwrap(),
        evidence_id,
        "Status should reflect latest evidence"
    );
    
    assert!(status.latest_evidence.is_some(), "Should have evidence in status");
    
    println!("Evidence tracking validated!");
}

#[tokio::test]
async fn test_ceremony_metrics_accuracy() {
    let test = SystemRecoveryTest::new(4).await;
    let request = test.create_recovery_request(3, RecoveryPriority::Normal);
    
    println!("Testing ceremony metrics accuracy...");
    
    let response = test.recovering_ops()
        .start_guardian_recovery(request)
        .await
        .expect("Recovery should succeed");
    
    let metrics = &response.metrics;
    
    // Verify all metrics are reasonable
    assert_eq!(metrics.guardians_contacted, 4, "Should contact all available guardians");
    assert_eq!(metrics.guardians_approved, 3, "Should get exactly the threshold number");
    assert_eq!(metrics.cooldown_blocked, 0, "No guardians should be in cooldown initially");
    
    // Timing should be valid
    assert!(metrics.started_at > 0, "Start time should be set");
    assert!(metrics.completed_at >= metrics.started_at, "Completion after start");
    assert!(metrics.completed_at - metrics.started_at < 60, "Should complete within 60 seconds");
    
    // Disputes should be zero initially
    assert_eq!(metrics.dispute_count, 0, "No disputes initially");
    
    println!("Metrics accuracy validated!");
}

#[tokio::test]
async fn test_full_ceremony_deterministic_behavior() {
    println!("Testing deterministic ceremony behavior...");
    
    // Run the same ceremony multiple times with the same inputs
    let mut results = Vec::new();
    
    for i in 0..3 {
        let test = SystemRecoveryTest::new(3).await;
        let request = test.create_recovery_request(2, RecoveryPriority::Normal);
        
        let response = test.recovering_ops()
            .start_guardian_recovery(request)
            .await
            .expect(&format!("Recovery {} should succeed", i));
        
        results.push((
            response.guardian_approvals.len(),
            response.recovery_artifacts.len(),
            response.success,
        ));
        
        println!("Ceremony {} completed", i);
    }
    
    // All results should be consistent
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result.0, 2, "Run {} should have 2 approvals", i);
        assert_eq!(result.1, 1, "Run {} should have 1 artifact", i);
        assert!(result.2, "Run {} should be successful", i);
    }
    
    println!("Deterministic behavior validated!");
}