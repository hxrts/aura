//! Maintenance service
//!
//! Provides coordinated maintenance operations including snapshots,
//! cache invalidation, and OTA upgrades.
//!
//! # Architecture
//!
//! The maintenance service:
//! - Uses `SnapshotProtocol` and `OTAProtocol` from protocols/
//! - Uses `CacheManager` from infrastructure/
//! - Publishes maintenance events to journal
//! - Coordinates threshold approval for major operations
//!
//! # Usage
//!
//! ```rust,no_run
//! use aura_sync::services::{MaintenanceService, MaintenanceServiceConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = MaintenanceServiceConfig::default();
//! let service = MaintenanceService::new(config)?;
//!
//! // Propose snapshot
//! service.propose_snapshot(target_epoch, state_digest).await?;
//!
//! // Handle OTA upgrade
//! service.activate_upgrade(upgrade_proposal).await?;
//! # Ok(())
//! # }
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};
use std::collections::BTreeSet;
use parking_lot::RwLock;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use aura_core::{DeviceId, Hash32, SemanticVersion, tree::Snapshot};
use crate::core::{SyncError, SyncResult};
use crate::infrastructure::CacheManager;
use crate::protocols::{SnapshotProtocol, SnapshotConfig, OTAProtocol, OTAConfig, UpgradeKind};
use super::{Service, HealthStatus, HealthCheck, ServiceState};

// Re-export maintenance event types from legacy module
// These will be gradually migrated to new patterns
pub use crate::maintenance::{
    MaintenanceEvent, SnapshotProposed, SnapshotCompleted,
    CacheInvalidated, UpgradeActivated, AdminReplaced,
    UpgradeProposal, IdentityEpochFence,
};

// =============================================================================
// Configuration
// =============================================================================

/// Maintenance service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaintenanceServiceConfig {
    /// Snapshot protocol configuration
    pub snapshot: SnapshotConfig,

    /// OTA protocol configuration
    pub ota: OTAConfig,

    /// Enable automatic snapshot proposals
    pub auto_snapshot_enabled: bool,

    /// Interval between automatic snapshot proposals
    pub auto_snapshot_interval: Duration,

    /// Minimum epoch between snapshots
    pub min_snapshot_interval_epochs: u64,
}

impl Default for MaintenanceServiceConfig {
    fn default() -> Self {
        Self {
            snapshot: SnapshotConfig::default(),
            ota: OTAConfig::default(),
            auto_snapshot_enabled: false, // Manual by default
            auto_snapshot_interval: Duration::from_secs(3600), // 1 hour
            min_snapshot_interval_epochs: 100,
        }
    }
}

// =============================================================================
// Maintenance Service
// =============================================================================

/// High-level maintenance service
///
/// Coordinates distributed maintenance operations including snapshots,
/// cache invalidation, and over-the-air upgrades.
pub struct MaintenanceService {
    /// Configuration
    config: MaintenanceServiceConfig,

    /// Service state
    state: Arc<RwLock<ServiceState>>,

    /// Snapshot protocol
    snapshot_protocol: Arc<RwLock<SnapshotProtocol>>,

    /// OTA protocol
    ota_protocol: Arc<RwLock<OTAProtocol>>,

    /// Cache manager
    cache_manager: Arc<RwLock<CacheManager>>,

    /// Service start time
    started_at: Arc<RwLock<Option<Instant>>>,

    /// Last snapshot epoch
    last_snapshot_epoch: Arc<RwLock<Option<u64>>>,
}

impl MaintenanceService {
    /// Create a new maintenance service
    pub fn new(config: MaintenanceServiceConfig) -> SyncResult<Self> {
        let snapshot_protocol = SnapshotProtocol::new(config.snapshot.clone());
        let ota_protocol = OTAProtocol::new(config.ota.clone());
        let cache_manager = CacheManager::new();

        Ok(Self {
            config,
            state: Arc::new(RwLock::new(ServiceState::Stopped)),
            snapshot_protocol: Arc::new(RwLock::new(snapshot_protocol)),
            ota_protocol: Arc::new(RwLock::new(ota_protocol)),
            cache_manager: Arc::new(RwLock::new(cache_manager)),
            started_at: Arc::new(RwLock::new(None)),
            last_snapshot_epoch: Arc::new(RwLock::new(None)),
        })
    }

    /// Propose a snapshot
    pub async fn propose_snapshot(
        &self,
        proposer: DeviceId,
        target_epoch: u64,
        state_digest: Hash32,
    ) -> SyncResult<SnapshotProposed> {
        let mut protocol = self.snapshot_protocol.write();

        let (_guard, proposal) = protocol.propose(proposer, target_epoch, state_digest)?;

        // Convert to maintenance event type
        Ok(SnapshotProposed {
            proposal_id: proposal.proposal_id,
            proposer: proposal.proposer,
            target_epoch: proposal.target_epoch,
            state_digest: proposal.state_digest,
        })
    }

    /// Complete a snapshot
    pub async fn complete_snapshot(
        &self,
        proposal_id: Uuid,
        snapshot: Snapshot,
        participants: BTreeSet<DeviceId>,
        threshold_signature: Vec<u8>,
    ) -> SyncResult<SnapshotCompleted> {
        *self.last_snapshot_epoch.write() = Some(snapshot.leaf_epoch);

        Ok(SnapshotCompleted {
            proposal_id,
            snapshot,
            participants,
            threshold_signature,
        })
    }

    /// Invalidate cache keys
    pub fn invalidate_cache(
        &self,
        keys: Vec<String>,
        epoch_floor: u64,
    ) -> SyncResult<CacheInvalidated> {
        let mut cache = self.cache_manager.write();
        cache.invalidate_keys(&keys, epoch_floor);

        Ok(CacheInvalidated {
            keys,
            epoch_floor,
        })
    }

    /// Propose OTA upgrade
    pub async fn propose_upgrade(
        &self,
        package_id: Uuid,
        version: SemanticVersion,
        kind: UpgradeKind,
        package_hash: Hash32,
        proposer: DeviceId,
    ) -> SyncResult<UpgradeProposal> {
        let mut protocol = self.ota_protocol.write();

        let proposal = protocol.propose_upgrade(
            package_id,
            version.to_string(),
            kind,
            package_hash,
            proposer,
        )?;

        // Convert to maintenance event type
        Ok(UpgradeProposal {
            package_id: proposal.package_id,
            version,
            kind,
            package_hash: proposal.package_hash,
            activation_epoch: proposal.activation_epoch,
        })
    }

    /// Activate upgrade after approval
    pub async fn activate_upgrade(
        &self,
        proposal: UpgradeProposal,
        participants: BTreeSet<DeviceId>,
        threshold_signature: Vec<u8>,
    ) -> SyncResult<UpgradeActivated> {
        // TODO: Verify threshold signature

        Ok(UpgradeActivated {
            package_id: proposal.package_id,
            version: proposal.version,
            kind: proposal.kind,
            activation_epoch: proposal.activation_epoch.unwrap_or(0),
            epoch_fence: proposal.activation_epoch.map(|e| IdentityEpochFence { min_epoch: e }),
            participants,
            threshold_signature,
        })
    }

    /// Check if snapshot is due
    pub fn is_snapshot_due(&self, current_epoch: u64) -> bool {
        if !self.config.auto_snapshot_enabled {
            return false;
        }

        match *self.last_snapshot_epoch.read() {
            None => true, // First snapshot
            Some(last) => {
                current_epoch >= last + self.config.min_snapshot_interval_epochs
            }
        }
    }

    /// Get service uptime
    pub fn uptime(&self) -> Duration {
        self.started_at.read()
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO)
    }
}

#[async_trait::async_trait]
impl Service for MaintenanceService {
    async fn start(&self) -> SyncResult<()> {
        let mut state = self.state.write();
        if *state == ServiceState::Running {
            return Err(SyncError::Service("Service already running".to_string()));
        }

        *state = ServiceState::Starting;
        *self.started_at.write() = Some(Instant::now());

        // TODO: Start background tasks for auto-snapshot

        *state = ServiceState::Running;
        Ok(())
    }

    async fn stop(&self) -> SyncResult<()> {
        let mut state = self.state.write();
        if *state == ServiceState::Stopped {
            return Ok(());
        }

        *state = ServiceState::Stopping;

        // TODO: Stop background tasks
        // TODO: Complete pending operations

        *state = ServiceState::Stopped;
        Ok(())
    }

    async fn health_check(&self) -> SyncResult<HealthCheck> {
        let state = *self.state.read();
        let status = match state {
            ServiceState::Running => HealthStatus::Healthy,
            ServiceState::Starting => HealthStatus::Starting,
            ServiceState::Stopping => HealthStatus::Stopping,
            ServiceState::Stopped | ServiceState::Failed => HealthStatus::Unhealthy,
        };

        let mut details = std::collections::HashMap::new();

        let snapshot_protocol = self.snapshot_protocol.read();
        details.insert("snapshot_pending".to_string(),
            snapshot_protocol.is_pending().to_string());

        let ota_protocol = self.ota_protocol.read();
        details.insert("ota_pending".to_string(),
            ota_protocol.get_pending().is_some().to_string());

        details.insert("uptime".to_string(),
            format!("{}s", self.uptime().as_secs()));

        Ok(HealthCheck {
            status,
            message: Some("Maintenance service operational".to_string()),
            checked_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            details,
        })
    }

    fn name(&self) -> &str {
        "MaintenanceService"
    }

    fn is_running(&self) -> bool {
        *self.state.read() == ServiceState::Running
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_maintenance_service_creation() {
        let config = MaintenanceServiceConfig::default();
        let service = MaintenanceService::new(config).unwrap();

        assert_eq!(service.name(), "MaintenanceService");
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_maintenance_service_lifecycle() {
        let service = MaintenanceService::new(Default::default()).unwrap();

        service.start().await.unwrap();
        assert!(service.is_running());

        service.stop().await.unwrap();
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_cache_invalidation() {
        let service = MaintenanceService::new(Default::default()).unwrap();

        let result = service.invalidate_cache(
            vec!["key1".to_string(), "key2".to_string()],
            10,
        ).unwrap();

        assert_eq!(result.keys.len(), 2);
        assert_eq!(result.epoch_floor, 10);
    }

    #[test]
    fn test_snapshot_due_check() {
        let mut config = MaintenanceServiceConfig::default();
        config.auto_snapshot_enabled = true;
        config.min_snapshot_interval_epochs = 100;

        let service = MaintenanceService::new(config).unwrap();

        // First snapshot should be due
        assert!(service.is_snapshot_due(0));

        // After setting last snapshot
        *service.last_snapshot_epoch.write() = Some(50);
        assert!(!service.is_snapshot_due(100)); // 100 - 50 = 50 < 100
        assert!(service.is_snapshot_due(151));  // 151 - 50 = 101 >= 100
    }
}
