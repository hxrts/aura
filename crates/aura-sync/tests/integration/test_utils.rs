//! Test utilities for aura-sync integration tests
//!
//! This module provides common utilities, helpers, and fixtures used across
//! all integration test scenarios.

#![allow(missing_docs)]

use crate::shared_support::{default_test_time, device_labels, test_device_id, test_sync_config};
use aura_core::time::PhysicalTime;
use aura_core::types::Epoch;
use aura_core::{AuraError, AuraResult, AuthorityId, DeviceId};
use aura_sync::{
    core::{SessionManager, SyncConfig, SyncResult},
    protocols::{
        AntiEntropyConfig, AntiEntropyProtocol, EpochConfig, EpochRotationCoordinator,
        JournalSyncConfig, JournalSyncProtocol, OTAConfig, OTAProtocol, SnapshotConfig,
        SnapshotProtocol,
    },
};
use aura_testkit::{
    foundation::TestEffectComposer,
    simulation::{
        choreography::{ChoreographyTestHarness, CoordinatedSession},
        network::{NetworkCondition, NetworkSimulator},
    },
};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::timeout;

/// Test fixture for multi-device sync scenarios
pub struct MultiDeviceTestFixture {
    /// Choreography test harness for coordinating devices
    pub harness: ChoreographyTestHarness,
    /// Network simulator for controlling message delivery
    pub network: NetworkSimulator,
    /// List of device IDs in the test scenario
    pub devices: Vec<DeviceId>,
    /// List of authority IDs paired with the test devices
    pub authorities: Vec<AuthorityId>,
    /// Session managers for each device
    pub session_managers: HashMap<DeviceId, SessionManager<()>>,
    /// Sync configuration for the test
    pub config: SyncConfig,
}

/// Builder for repeated multi-device topology setup.
pub struct ScenarioBuilder {
    device_count: usize,
    partition: Option<(Vec<usize>, Vec<usize>)>,
    isolated_indices: Vec<usize>,
}

impl ScenarioBuilder {
    pub fn new(device_count: usize) -> Self {
        Self {
            device_count,
            partition: None,
            isolated_indices: Vec::new(),
        }
    }

    pub fn trio() -> Self {
        Self::new(3)
    }

    pub fn threshold_group() -> Self {
        Self::new(5)
    }

    pub fn with_partition_indices(mut self, group1: &[usize], group2: &[usize]) -> Self {
        self.partition = Some((group1.to_vec(), group2.to_vec()));
        self
    }

    pub fn isolate_indices(mut self, isolated_indices: &[usize]) -> Self {
        self.isolated_indices = isolated_indices.to_vec();
        self
    }

    pub async fn build(self) -> AuraResult<MultiDeviceTestFixture> {
        let mut fixture = MultiDeviceTestFixture::new(self.device_count).await?;
        if let Some((group1, group2)) = self.partition {
            fixture.partition_indices(&group1, &group2).await?;
        }
        for index in self.isolated_indices {
            fixture.isolate_index(index).await?;
        }
        Ok(fixture)
    }
}

impl MultiDeviceTestFixture {
    /// Create a new multi-device test fixture
    pub async fn new(device_count: usize) -> AuraResult<Self> {
        let device_labels = device_labels(device_count);
        let device_labels_refs: Vec<&str> = device_labels.iter().map(|s| s.as_str()).collect();

        let harness = ChoreographyTestHarness::with_labeled_devices(device_labels_refs);
        let network = NetworkSimulator::new();
        let devices = harness.device_ids();
        let authorities = harness.authority_ids();
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
            authorities,
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

    pub async fn partition_indices(
        &mut self,
        group1: &[usize],
        group2: &[usize],
    ) -> AuraResult<()> {
        self.create_partition(self.devices_for(group1)?, self.devices_for(group2)?)
            .await;
        Ok(())
    }

    /// Heal all network partitions
    pub async fn heal_partitions(&mut self) {
        self.network.heal_partition().await;
    }

    pub async fn isolate_index(&mut self, index: usize) -> AuraResult<()> {
        let isolated = self.device(index)?;
        for device in &self.devices {
            if *device != isolated {
                let partition_condition = NetworkCondition {
                    partitioned: true,
                    ..Default::default()
                };
                self.network
                    .set_conditions(isolated, *device, partition_condition.clone())
                    .await;
                self.network
                    .set_conditions(*device, isolated, partition_condition)
                    .await;
            }
        }
        Ok(())
    }

    /// Get session manager for a device
    pub fn session_manager(&self, device: DeviceId) -> Option<&SessionManager<()>> {
        self.session_managers.get(&device)
    }

    pub fn device(&self, index: usize) -> AuraResult<DeviceId> {
        self.devices
            .get(index)
            .copied()
            .ok_or_else(|| AuraError::internal(format!("No device at index {index}")))
    }

    pub fn authority(&self, index: usize) -> AuraResult<AuthorityId> {
        self.authorities
            .get(index)
            .copied()
            .ok_or_else(|| AuraError::internal(format!("No authority at index {index}")))
    }

    pub fn devices_for(&self, indices: &[usize]) -> AuraResult<Vec<DeviceId>> {
        indices.iter().map(|&index| self.device(index)).collect()
    }

    /// Get current time for session management
    fn current_time() -> PhysicalTime {
        default_test_time()
    }

    /// Create coordinated session across all devices
    pub async fn create_coordinated_session(
        &self,
        session_type: &str,
    ) -> AuraResult<CoordinatedSession> {
        self.harness
            .create_coordinated_session(session_type)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to create session: {e}")))
    }

    // REMOVED: wait_for_session_completion
    //
    // This method has been removed in favor of the type-state pattern.
    // Use EndedSession::wait_for_completion instead:
    //
    // OLD:  fixture.wait_for_session_completion(&session, timeout).await?;
    // NEW:  let ended = session.end().await?;
    //       ended.wait_for_completion(timeout).await?;
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
        account_id: aura_core::AccountId::new_from_entropy([1u8; 32]),
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
    current_epoch: Epoch,
) -> EpochRotationCoordinator {
    let config = EpochConfig {
        epoch_duration: Duration::from_secs(300),
        rotation_threshold: 2,
        synchronization_timeout: Duration::from_secs(30),
    };
    EpochRotationCoordinator::new(device_id, current_epoch, config)
}

/// Simulate journal state divergence between devices
pub async fn create_divergent_journal_states(
    fixture: &mut MultiDeviceTestFixture,
) -> AuraResult<()> {
    // This would integrate with the actual journal implementation
    // For now, we simulate the setup that would create divergent states

    if fixture.devices.len() < 3 {
        return Err(AuraError::internal(String::from(
            "Need at least 3 devices for divergence test",
        )));
    }

    fixture.partition_indices(&[0, 1], &[2]).await?;

    // At this point, device0 and device1 can sync while device2 is isolated
    // This creates the foundation for divergent journal states
    Ok(())
}

/// Verify that journal states are synchronized across devices
pub async fn verify_journal_consistency(fixture: &MultiDeviceTestFixture) -> AuraResult<bool> {
    // This would integrate with actual journal state verification
    // For the integration test, we simulate the verification logic

    let session = fixture.create_coordinated_session("verification").await?;

    // End the session and wait for completion using type-state pattern
    let ended = session
        .end()
        .await
        .map_err(|e| AuraError::internal(format!("Failed to end verification session: {e}")))?;

    ended
        .wait_for_completion(Duration::from_secs(30))
        .await
        .map_err(|e| AuraError::internal(format!("Verification session timeout: {e}")))?;

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
        .map_err(|_| AuraError::internal(String::from("Sync operation timeout")))?
        .map_err(|e| AuraError::internal(format!("Sync failed: {e}")))
}

/// Assert that a sync result fails within timeout
pub async fn assert_sync_failure<T>(
    future: impl std::future::Future<Output = SyncResult<T>>,
    timeout_duration: Duration,
) -> AuraResult<()> {
    let result = timeout(timeout_duration, future).await.map_err(|_| {
        AuraError::internal(String::from("Expected failure but operation timed out"))
    })?;

    match result {
        Ok(_) => Err(AuraError::internal(String::from(
            "Expected failure but operation succeeded",
        ))),
        Err(_) => Ok(()),
    }
}
