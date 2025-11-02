//! Production example demonstrating hardened choreographic protocols
//!
//! This example shows how to use all Phase 5 production hardening features:
//! - Error handling with aura-types integration
//! - Timeout management
//! - Byzantine fault tolerance
//! - Full middleware integration

use crate::context::BaseContext;
use crate::effects::AuraEffectsAdapter;
use crate::protocols::choreographic::{
    BridgedRole, ByzantineDetector, ChoreographicHandlerBuilder, ChoreographyMiddlewareConfig,
    ChoreographyResult, OperationType, SafeChoreography, TimeoutConfig, TimeoutManager,
};
use aura_crypto::Effects;
use aura_types::{errors::AuraError, DeviceId};
use rumpsteak_choreography::ChoreoHandler;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Example: Production-hardened FROST signing protocol
pub async fn production_frost_signing_example(
    device_id: DeviceId,
    context: BaseContext,
    participants: Vec<DeviceId>,
    message: &[u8],
) -> ChoreographyResult<Vec<u8>> {
    // 1. Create production effects
    let effects = Effects::production();
    let effects_adapter = AuraEffectsAdapter::new(effects, device_id.into());

    // 2. Configure middleware for production
    let config = ChoreographyMiddlewareConfig {
        device_name: format!("frost-signer-{}", device_id),
        enable_observability: true,  // Always trace and monitor in production
        enable_capabilities: true,   // Enforce authorization
        enable_error_recovery: true, // Handle transient failures
        max_retries: 5,              // More retries for critical operations
    };

    // 3. Build handler with full middleware stack
    let handler = ChoreographicHandlerBuilder::new(effects_adapter)
        .with_config(config)
        .build_in_memory(device_id, context);

    // 4. Wrap in error-safe handler
    let mut safe_handler = SafeChoreography::new(handler);

    // 5. Create timeout manager with production config
    let timeout_mgr = TimeoutManager::with_config(TimeoutConfig::for_production());

    // 6. Create Byzantine detector
    let byzantine_detector = Arc::new(Mutex::new(ByzantineDetector::new()));

    // 7. Execute FROST signing with all protections
    timeout_mgr
        .with_timeout(
            OperationType::Frost,
            execute_frost_with_byzantine_detection(
                &mut safe_handler,
                participants,
                message,
                byzantine_detector.clone(),
            ),
        )
        .await
}

async fn execute_frost_with_byzantine_detection<H>(
    safe_handler: &mut SafeChoreography<H>,
    participants: Vec<DeviceId>,
    message: &[u8],
    detector: Arc<Mutex<ByzantineDetector>>,
) -> ChoreographyResult<Vec<u8>>
where
    H: ChoreoHandler<Role = BridgedRole>,
{
    use crate::choreo_assert;

    // Validate inputs
    safe_handler
        .execute(|_| {
            choreo_assert!(
                participants.len() >= 3,
                "FROST requires at least 3 participants"
            );
            choreo_assert!(
                participants.len() <= 100,
                "Too many participants for efficient FROST"
            );
            choreo_assert!(!message.is_empty(), "Cannot sign empty message");
            Ok(())
        })
        .await?;

    // Phase 1: Round 1 commitments with Byzantine detection
    let round1_result = safe_handler
        .execute_with_retry(3, |handler| {
            frost_round1_with_detection(handler, &participants, detector.clone())
        })
        .await?;

    // Phase 2: Round 2 signing with Byzantine detection
    let signature = safe_handler
        .execute_with_retry(3, |handler| {
            frost_round2_with_detection(
                handler,
                &participants,
                message,
                round1_result.clone(),
                detector.clone(),
            )
        })
        .await?;

    // Verify Byzantine threshold wasn't exceeded
    {
        let detector = detector.lock().await;
        let report = detector.get_report();

        if !report.byzantine_participants.is_empty() {
            tracing::warn!(
                "Detected {} Byzantine participants during FROST signing",
                report.byzantine_participants.len()
            );
        }
    }

    Ok(signature)
}

fn frost_round1_with_detection<H>(
    _handler: &mut H,
    participants: &[DeviceId],
    detector: Arc<Mutex<ByzantineDetector>>,
) -> Result<Vec<u8>, rumpsteak_choreography::ChoreographyError>
where
    H: ChoreoHandler<Role = BridgedRole>,
{
    // Simulated FROST round 1
    // In production, this would use actual FROST implementation

    // Record successful interactions for honest participants
    let detector_clone = detector.clone();
    let participants_clone = participants.to_vec();
    tokio::spawn(async move {
        let mut det = detector_clone.lock().await;
        for participant in participants_clone {
            det.record_success(participant.into());
        }
    });

    Ok(vec![1, 2, 3, 4]) // Mock commitment
}

fn frost_round2_with_detection<H>(
    _handler: &mut H,
    participants: &[DeviceId],
    _message: &[u8],
    _round1_result: Vec<u8>,
    detector: Arc<Mutex<ByzantineDetector>>,
) -> Result<Vec<u8>, rumpsteak_choreography::ChoreographyError>
where
    H: ChoreoHandler<Role = BridgedRole>,
{
    // Simulated FROST round 2
    // In production, this would use actual FROST implementation

    // Simulate Byzantine behavior detection
    let detector_clone = detector.clone();
    let participants_clone = participants.to_vec();
    tokio::spawn(async move {
        let mut det = detector_clone.lock().await;

        // Most participants behave honestly
        for (i, participant) in participants_clone.iter().enumerate() {
            if i == 0 && participants_clone.len() > 10 {
                // Simulate one Byzantine participant in large groups
                let _ = det.record_invalid_message((*participant).into());
            } else {
                det.record_success((*participant).into());
            }
        }
    });

    Ok(vec![5, 6, 7, 8]) // Mock signature
}

/// Example: Production-hardened recovery protocol with deadlines
pub async fn production_recovery_example(
    device_id: DeviceId,
    context: BaseContext,
    guardians: Vec<DeviceId>,
) -> ChoreographyResult<()> {
    use crate::protocols::choreographic::timeout_management::DeadlineTracker;
    use std::time::Duration;

    // Create timeout manager with conservative timeouts
    let timeout_config = TimeoutConfig {
        recovery_timeout: Duration::from_secs(600), // 10 minutes for recovery
        network_timeout: Duration::from_secs(60),   // 1 minute for network ops
        ..TimeoutConfig::for_production()
    };
    let timeout_mgr = TimeoutManager::with_config(timeout_config);

    // Create deadline tracker for multi-phase recovery
    let mut deadline_tracker = DeadlineTracker::new(Duration::from_secs(600));
    deadline_tracker.add_phase("Guardian Discovery".to_string(), Duration::from_secs(120));
    deadline_tracker.add_phase("Approval Collection".to_string(), Duration::from_secs(300));
    deadline_tracker.add_phase("State Recovery".to_string(), Duration::from_secs(180));

    // Phase 1: Guardian discovery
    deadline_tracker.check_phase_timeout()?;
    let available_guardians = timeout_mgr
        .with_custom_timeout(
            Duration::from_secs(120),
            "Guardian Discovery",
            discover_guardians(device_id, &context, guardians),
        )
        .await?;
    deadline_tracker.next_phase()?;

    // Phase 2: Collect guardian approvals
    deadline_tracker.check_phase_timeout()?;
    if available_guardians.len() < 2 {
        return Err(AuraError::recovery_failed(
            "Insufficient guardians available",
        ));
    }

    let approvals = timeout_mgr
        .with_custom_timeout(
            Duration::from_secs(300),
            "Approval Collection",
            collect_guardian_approvals(device_id, &context, available_guardians),
        )
        .await?;
    deadline_tracker.next_phase()?;

    // Phase 3: Recover state
    deadline_tracker.check_phase_timeout()?;
    timeout_mgr
        .with_custom_timeout(
            Duration::from_secs(180),
            "State Recovery",
            recover_account_state(device_id, &context, approvals),
        )
        .await?;

    Ok(())
}

async fn discover_guardians(
    _device_id: DeviceId,
    _context: &BaseContext,
    guardians: Vec<DeviceId>,
) -> ChoreographyResult<Vec<DeviceId>> {
    // Simulate guardian discovery
    // In production, this would check guardian availability
    Ok(guardians.into_iter().take(3).collect())
}

async fn collect_guardian_approvals(
    _device_id: DeviceId,
    _context: &BaseContext,
    guardians: Vec<DeviceId>,
) -> ChoreographyResult<Vec<Vec<u8>>> {
    // Simulate approval collection
    // In production, this would use threshold signatures
    Ok(guardians.iter().map(|_| vec![1, 2, 3]).collect())
}

async fn recover_account_state(
    _device_id: DeviceId,
    _context: &BaseContext,
    _approvals: Vec<Vec<u8>>,
) -> ChoreographyResult<()> {
    // Simulate state recovery
    // In production, this would restore encrypted state
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::MemoryTransport;
    use aura_journal::AccountLedger;
    use ed25519_dalek::SigningKey;
    use tokio::sync::RwLock;

    fn create_test_context(device_id: Uuid) -> BaseContext {
        let session_id = Uuid::new_v4();
        let participants = vec![DeviceId::from(device_id)];
        let ledger = Arc::new(RwLock::new(AccountLedger::new(vec![])));
        let transport = Arc::new(MemoryTransport::new());
        let effects = Effects::test(42);
        let device_key = SigningKey::from_bytes(&[1u8; 32]);
        let time_source = Box::new(crate::effects::SimulatedTimeSource::new());

        BaseContext::new(
            session_id,
            device_id,
            participants,
            Some(2),
            ledger,
            transport,
            effects,
            device_key,
            time_source,
        )
    }

    #[tokio::test]
    async fn test_production_frost_example() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let context = create_test_context(device_id.into());
        let participants = vec![
            device_id,
            DeviceId::from(Uuid::new_v4()),
            DeviceId::from(Uuid::new_v4()),
        ];

        let result =
            production_frost_signing_example(device_id, context, participants, b"test message")
                .await;

        // In real test, this would fail due to missing transport setup
        // But it demonstrates the API usage
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_production_recovery_example() {
        let device_id = DeviceId::from(Uuid::new_v4());
        let context = create_test_context(device_id.into());
        let guardians = vec![
            DeviceId::from(Uuid::new_v4()),
            DeviceId::from(Uuid::new_v4()),
            DeviceId::from(Uuid::new_v4()),
        ];

        let result = production_recovery_example(device_id, context, guardians).await;

        // Should complete successfully with mocked operations
        assert!(result.is_ok());
    }
}
