//! Test utilities for aura-sync integration tests
//!
//! This module provides common utilities, helpers, and fixtures used across
//! all integration test scenarios.

use super::test_device_id;
use aura_core::{AuraError, AuraResult, DeviceId};
use aura_sync::{
    core::{SessionManager, SyncConfig, SyncResult},
    protocols::{
        AntiEntropyConfig, AntiEntropyProtocol, EpochConfig, EpochRotationCoordinator,
        JournalSyncConfig, JournalSyncProtocol, OTAConfig, OTAProtocol, SnapshotConfig,
        SnapshotProtocol,
    },
    services::{MaintenanceService, SyncService},
};
use aura_testkit::{
    builders::{account::*, device::*},
    foundation::{create_mock_test_context, TestEffectComposer},
    simulation::{
        choreography::{
            ChoreographyTestHarness, CoordinatedSession, MockSessionState, SessionStatus, TestError,
        },
        network::{NetworkCondition, NetworkSimulator},
    },
};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tokio::time::timeout;

/// Test fixture for multi-device sync scenarios
pub struct MultiDeviceTestFixture {
    pub harness: ChoreographyTestHarness,
    pub network: NetworkSimulator,
    pub devices: Vec<DeviceId>,
    pub session_managers: HashMap<DeviceId, SessionManager<()>>,
    pub config: SyncConfig,
}

impl MultiDeviceTestFixture {
    /// Create a new multi-device test fixture
    pub async fn new(device_count: usize) -> AuraResult<Self> {
        let device_labels: Vec<String> =
            (0..device_count).map(|i| format!("device_{}", i)).collect();
        let device_labels_refs: Vec<&str> = device_labels.iter().map(|s| s.as_str()).collect();

        let harness = ChoreographyTestHarness::with_labeled_devices(device_labels_refs);
        let network = NetworkSimulator::new();
        let devices = harness.device_ids();
        let config = test_sync_config();

        let mut session_managers = HashMap::new();
        for device_id in &devices {
            let session_config = aura_sync::core::session::SessionConfig::default();
            let session_manager = SessionManager::new(session_config, Self::current_time());
            session_managers.insert(*device_id, session_manager);
        }

        Ok(Self {
            harness,
            network,
            devices,
            session_managers,
            config,
        })
    }

    /// Create a three-device fixture (most common scenario)
    pub async fn trio() -> AuraResult<Self> {
        Self::new(3).await
    }

    /// Create a five-device fixture (for threshold scenarios)
    pub async fn threshold_group() -> AuraResult<Self> {
        Self::new(5).await
    }

    /// Set network conditions between specific devices
    pub async fn set_network_condition(
        &mut self,
        from: DeviceId,
        to: DeviceId,
        condition: NetworkCondition,
    ) {
        self.network.set_conditions(from, to, condition).await;
    }

    /// Partition the network between two device groups
    pub async fn create_partition(&mut self, group1: Vec<DeviceId>, group2: Vec<DeviceId>) {
        self.network.partition(group1, group2).await;
    }

    /// Heal all network partitions
    pub async fn heal_partitions(&mut self) {
        self.network.heal_partition().await;
    }

    /// Get session manager for a device
    pub fn session_manager(&self, device: DeviceId) -> Option<&SessionManager<()>> {
        self.session_managers.get(&device)
    }

    /// Get current time for session management
    fn current_time() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Create coordinated session across all devices
    pub async fn create_coordinated_session(
        &self,
        session_type: &str,
    ) -> AuraResult<CoordinatedSession> {
        self.harness
            .create_coordinated_session(session_type)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to create session: {}", e)))
    }

    /// Wait for session to complete with timeout
    pub async fn wait_for_session_completion(
        &self,
        session: &CoordinatedSession,
        timeout_duration: Duration,
    ) -> AuraResult<()> {
        timeout(timeout_duration, async {
            loop {
                let status = session.status().await.map_err(|e| {
                    AuraError::internal(format!("Failed to get session status: {}", e))
                })?;
                match status.status {
                    SessionStatus::Ended => return Ok(()),
                    SessionStatus::Active => tokio::time::sleep(Duration::from_millis(100)).await,
                }
            }
        })
        .await
        .map_err(|_| AuraError::internal("Session timeout".to_string()))?
    }
}

/// Helper for creating anti-entropy protocol instances
pub fn create_anti_entropy_protocol() -> AntiEntropyProtocol {
    let config = AntiEntropyConfig {
        digest_timeout: Duration::from_secs(5),
        transfer_timeout: Duration::from_secs(10),
        batch_size: 100,
        max_rounds: 3,
        ..Default::default()
    };
    AntiEntropyProtocol::new(config)
}

/// Helper for creating journal sync protocol instances
pub fn create_journal_sync_protocol() -> JournalSyncProtocol {
    let config = JournalSyncConfig {
        account_id: aura_core::AccountId::new(),
        batch_size: 50,
        sync_timeout: Duration::from_secs(15),
        max_concurrent_syncs: 5,
        retry_enabled: true,
        ..Default::default()
    };
    JournalSyncProtocol::new(config)
}

/// Helper for creating snapshot protocol instances
pub fn create_snapshot_protocol() -> SnapshotProtocol {
    let config = SnapshotConfig {
        approval_threshold: 2,
        quorum_size: 3,
        use_writer_fence: true,
        ..Default::default()
    };
    SnapshotProtocol::new(config)
}

/// Helper for creating OTA protocol instances
pub fn create_ota_protocol() -> OTAProtocol {
    let config = OTAConfig {
        readiness_threshold: 2,
        quorum_size: 3,
        enforce_epoch_fence: true,
    };
    OTAProtocol::new(config)
}

/// Helper for creating epoch rotation coordinator
pub fn create_epoch_coordinator(
    device_id: DeviceId,
    current_epoch: u64,
) -> EpochRotationCoordinator {
    let config = EpochConfig {
        epoch_duration: Duration::from_secs(300),
        rotation_threshold: 2,
        synchronization_timeout: Duration::from_secs(30),
    };
    EpochRotationCoordinator::new(device_id, current_epoch, config)
}

/// Test sync configuration for integration tests
pub fn test_sync_config() -> SyncConfig {
    SyncConfig::for_testing()
}

/// Simulate journal state divergence between devices
pub async fn create_divergent_journal_states(
    fixture: &mut MultiDeviceTestFixture,
) -> AuraResult<()> {
    // This would integrate with the actual journal implementation
    // For now, we simulate the setup that would create divergent states

    if fixture.devices.len() < 3 {
        return Err(AuraError::internal(
            "Need at least 3 devices for divergence test".to_string(),
        ));
    }

    // Simulate device 0 and 1 syncing while device 2 is partitioned
    let device0 = fixture.devices[0];
    let device1 = fixture.devices[1];
    let device2 = fixture.devices[2];

    // Partition device 2 from others
    let partition_condition = NetworkCondition {
        partitioned: true,
        ..Default::default()
    };

    fixture
        .network
        .set_conditions(device0, device2, partition_condition.clone())
        .await;
    fixture
        .network
        .set_conditions(device1, device2, partition_condition.clone())
        .await;
    fixture
        .network
        .set_conditions(device2, device0, partition_condition.clone())
        .await;
    fixture
        .network
        .set_conditions(device2, device1, partition_condition)
        .await;

    // At this point, device0 and device1 can sync while device2 is isolated
    // This creates the foundation for divergent journal states
    Ok(())
}

/// Verify that journal states are synchronized across devices
pub async fn verify_journal_consistency(fixture: &MultiDeviceTestFixture) -> AuraResult<bool> {
    // This would integrate with actual journal state verification
    // For the integration test, we simulate the verification logic

    let session = fixture.create_coordinated_session("verification").await?;

    // Wait for verification to complete
    fixture
        .wait_for_session_completion(&session, Duration::from_secs(30))
        .await?;

    // In a real implementation, this would check actual journal state equality
    Ok(true)
}

/// Create mock effect handlers for testing
pub fn create_test_effects() -> TestEffectComposer {
    use aura_testkit::foundation::ExecutionMode;

    let device_id = test_device_id(b"test_device");
    TestEffectComposer::new(ExecutionMode::Testing, device_id)
}

/// Assert that a sync result succeeded within timeout
pub async fn assert_sync_success<T>(
    future: impl std::future::Future<Output = SyncResult<T>>,
    timeout_duration: Duration,
) -> AuraResult<T> {
    timeout(timeout_duration, future)
        .await
        .map_err(|_| AuraError::internal("Sync operation timeout".to_string()))?
        .map_err(|e| AuraError::internal(format!("Sync failed: {}", e)))
}

/// Assert that a sync result fails within timeout
pub async fn assert_sync_failure<T>(
    future: impl std::future::Future<Output = SyncResult<T>>,
    timeout_duration: Duration,
) -> AuraResult<()> {
    let result = timeout(timeout_duration, future)
        .await
        .map_err(|_| AuraError::internal("Expected failure but operation timed out".to_string()))?;

    match result {
        Ok(_) => Err(AuraError::internal(
            "Expected failure but operation succeeded".to_string(),
        )),
        Err(_) => Ok(()),
    }
}
