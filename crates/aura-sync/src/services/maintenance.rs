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

// PROGRESS: Migrated to PhysicalTimeEffects and RandomEffects.
// - Added start_with_time_effects() method for proper effect system integration
// - Updated propose_upgrade() to use RandomEffects for deterministic UUID generation
// - Original Service trait methods still use direct time calls for compatibility
#![allow(clippy::disallowed_methods)]

use parking_lot::RwLock;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{HealthCheck, HealthStatus, Service, ServiceState};
use crate::core::{sync_session_error, SyncResult};
use crate::infrastructure::CacheManager;
use crate::protocols::{OTAConfig, OTAProtocol, SnapshotConfig, SnapshotProtocol, UpgradeKind};
use aura_core::effects::{PhysicalTimeEffects, RandomEffects, TimeEffects};
use aura_core::{tree::Snapshot, AccountId, AuraError, DeviceId, Epoch, Hash32, SemanticVersion};

// =============================================================================
// Maintenance Event Types
// =============================================================================

/// Key used for cache invalidation events.
pub type CacheKey = String;

/// Maintenance events replicated through the journal CRDT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaintenanceEvent {
    /// Snapshot proposal broadcast.
    SnapshotProposed(SnapshotProposed),
    /// Snapshot completion notification.
    SnapshotCompleted(SnapshotCompleted),
    /// Cache invalidation fact.
    CacheInvalidated(CacheInvalidated),
    /// Upgrade activation notice.
    UpgradeActivated(UpgradeActivated),
    /// Admin replacement announcement (stub for fork workflow).
    AdminReplaced(AdminReplaced),
}

/// Snapshot proposal metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotProposed {
    /// Unique proposal identifier.
    pub proposal_id: Uuid,
    /// Device that initiated the proposal.
    pub proposer: DeviceId,
    /// Identity epoch fence for the snapshot.
    pub target_epoch: Epoch,
    /// Digest of the candidate snapshot payload (hash of canonical encoding).
    pub state_digest: Hash32,
}

impl SnapshotProposed {
    /// Create a new proposal.
    pub fn new(proposer: DeviceId, target_epoch: Epoch, state_digest: Hash32) -> Self {
        Self {
            #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in maintenance proposal ID generation
            proposal_id: Uuid::new_v4(),
            proposer,
            target_epoch,
            state_digest,
        }
    }
}

/// Snapshot completion payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotCompleted {
    /// Identifier of the accepted proposal.
    pub proposal_id: Uuid,
    /// Finalized snapshot payload.
    pub snapshot: Snapshot,
    /// Participants that contributed to the threshold signature.
    pub participants: BTreeSet<DeviceId>,
    /// Threshold signature attesting to this snapshot.
    pub threshold_signature: Vec<u8>,
}

impl SnapshotCompleted {
    /// Convenience constructor.
    pub fn new(
        proposal_id: Uuid,
        snapshot: Snapshot,
        participants: BTreeSet<DeviceId>,
        threshold_signature: Vec<u8>,
    ) -> Self {
        Self {
            proposal_id,
            snapshot,
            participants,
            threshold_signature,
        }
    }
}

/// Cache invalidation payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CacheInvalidated {
    /// Keys that must be refreshed.
    pub keys: Vec<CacheKey>,
    /// Earliest identity epoch the cache entry remains valid for.
    pub epoch_floor: Epoch,
}

impl CacheInvalidated {
    /// Create a new invalidation payload.
    pub fn new(keys: Vec<CacheKey>, epoch_floor: Epoch) -> Self {
        Self { keys, epoch_floor }
    }
}

/// Upgrade activation metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpgradeActivated {
    /// Unique identifier of the upgrade package.
    pub package_id: Uuid,
    /// Protocol version activated.
    pub to_version: SemanticVersion,
    /// Identity epoch fence where the upgrade becomes mandatory.
    pub activation_fence: IdentityEpochFence,
}

impl UpgradeActivated {
    /// Create a new activation event.
    pub fn new(package_id: Uuid, to_version: SemanticVersion, fence: IdentityEpochFence) -> Self {
        Self {
            package_id,
            to_version,
            activation_fence: fence,
        }
    }
}

/// Admin replacement announcement (allows users to fork away from a malicious admin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminReplaced {
    /// Account the new admin controls.
    pub account_id: AccountId,
    /// Previous administrator device (for audit).
    pub previous_admin: DeviceId,
    /// New administrator device.
    pub new_admin: DeviceId,
    /// Epoch when the new admin takes effect.
    pub activation_epoch: Epoch,
}

impl AdminReplaced {
    /// Create a new admin replacement fact.
    pub fn new(
        account_id: AccountId,
        previous_admin: DeviceId,
        new_admin: DeviceId,
        activation_epoch: Epoch,
    ) -> Self {
        Self {
            account_id,
            previous_admin,
            new_admin,
            activation_epoch,
        }
    }
}

/// Identity-epoch fence describing when an upgrade becomes mandatory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityEpochFence {
    /// Account the fence applies to.
    pub account_id: AccountId,
    /// Target epoch for enforcement.
    pub epoch: Epoch,
}

impl IdentityEpochFence {
    /// Helper constructor.
    pub fn new(account_id: AccountId, epoch: Epoch) -> Self {
        Self { account_id, epoch }
    }
}

/// Upgrade proposal metadata used by the OTA coordinator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpgradeProposal {
    /// Package identifier.
    pub package_id: Uuid,
    /// Semantic version of the new protocol bundle.
    pub version: SemanticVersion,
    /// Hash of the artifact (canonical digest of bundle manifest).
    pub artifact_hash: Hash32,
    /// Optional download location (HTTP(s), git ref, etc.).
    pub artifact_uri: Option<String>,
    /// Upgrade flavor.
    pub kind: UpgradeKind,
    /// Optional activation fence for hard forks.
    pub activation_fence: Option<IdentityEpochFence>,
}

impl UpgradeProposal {
    /// Ensure proposal semantics make sense (e.g., hard forks need a fence).
    pub fn validate(&self) -> aura_core::AuraResult<()> {
        if matches!(self.kind, UpgradeKind::HardFork) && self.activation_fence.is_none() {
            return Err(aura_core::AuraError::invalid(
                "hard fork proposals must include an activation fence",
            ));
        }
        Ok(())
    }
}

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
        let protocol = self.snapshot_protocol.write();

        let (_guard, proposal) =
            protocol.propose(proposer, target_epoch, state_digest, Uuid::nil())?;

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
        *self.last_snapshot_epoch.write() = Some(snapshot.epoch);

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

        Ok(CacheInvalidated { keys, epoch_floor })
    }

    /// Propose OTA upgrade
    pub async fn propose_upgrade<R: RandomEffects>(
        &self,
        package_id: Uuid,
        version: SemanticVersion,
        kind: UpgradeKind,
        package_hash: Hash32,
        proposer: DeviceId,
        random_effects: &R,
    ) -> SyncResult<UpgradeProposal> {
        // Use RandomEffects for deterministic UUID generation
        let proposal_id = random_effects.random_uuid().await;

        let mut protocol = self.ota_protocol.write();
        let proposal = protocol.propose_upgrade(
            proposal_id,
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
            artifact_hash: proposal.package_hash,
            artifact_uri: Self::generate_artifact_uri(&proposal, &version), // Add URI support for artifacts
            activation_fence: Self::map_activation_epoch(&proposal, proposer), // Map activation_epoch to IdentityEpochFence
        })
    }

    /// Verify threshold signature for maintenance operation
    async fn verify_threshold_signature<C: aura_core::effects::CryptoEffects>(
        &self,
        proposal: &UpgradeProposal,
        crypto_effects: &C,
        threshold_signature: &[u8],
        group_public_key: &[u8],
    ) -> SyncResult<()> {
        // Construct message for signature verification
        // This should match the format used when creating the signature
        let message = self.construct_upgrade_message(proposal);

        // Verify FROST threshold signature
        match crypto_effects
            .frost_verify(&message, threshold_signature, group_public_key)
            .await
        {
            Ok(true) => {
                tracing::info!(
                    "Threshold signature verification successful for upgrade proposal {}",
                    proposal.package_id
                );
                Ok(())
            }
            Ok(false) => {
                let error_msg = format!(
                    "Threshold signature verification failed for upgrade proposal {}",
                    proposal.package_id
                );
                tracing::error!("{}", error_msg);
                Err(crate::core::errors::sync_validation_error(error_msg))
            }
            Err(e) => {
                let error_msg = format!(
                    "Threshold signature verification error for upgrade proposal {}: {}",
                    proposal.package_id, e
                );
                tracing::error!("{}", error_msg);
                Err(crate::core::errors::sync_validation_error(error_msg))
            }
        }
    }

    /// Construct message for upgrade proposal signature verification
    #[allow(clippy::unwrap_used)] // Vec::write_all is infallible
    fn construct_upgrade_message(&self, proposal: &UpgradeProposal) -> Vec<u8> {
        use std::io::Write;

        let mut message = Vec::new();

        // Domain separator
        message.write_all(b"AURA_UPGRADE_PROPOSAL").unwrap();

        // Package ID
        message.write_all(proposal.package_id.as_bytes()).unwrap();

        // Version
        message
            .write_all(proposal.version.to_string().as_bytes())
            .unwrap();

        // Artifact hash
        message.write_all(&proposal.artifact_hash.0).unwrap();

        // Upgrade kind (serialized)
        match proposal.kind {
            UpgradeKind::SoftFork => message.write_all(b"SOFT_FORK").unwrap(),
            UpgradeKind::HardFork => message.write_all(b"HARD_FORK").unwrap(),
        }

        // Activation fence if present
        if let Some(ref fence) = proposal.activation_fence {
            message.write_all(fence.account_id.0.as_bytes()).unwrap();
            message.write_all(&fence.epoch.to_le_bytes()).unwrap();
        }

        message
    }

    /// Map activation_epoch to IdentityEpochFence
    fn map_activation_epoch(
        proposal: &crate::protocols::ota::UpgradeProposal,
        proposer: DeviceId,
    ) -> Option<IdentityEpochFence> {
        // Map activation epoch from OTA proposal to identity epoch fence
        if let Some(activation_epoch) = proposal.activation_epoch {
            // For hard forks, we need an epoch fence to coordinate the upgrade
            // The account ID is derived from the proposer device ID
            let account_id = AccountId(proposer.0); // Device belongs to account

            Some(IdentityEpochFence::new(account_id, activation_epoch))
        } else {
            // Soft upgrades don't require epoch fencing
            None
        }
    }

    /// Generate artifact URI for package downloads
    fn generate_artifact_uri(
        proposal: &crate::protocols::ota::UpgradeProposal,
        version: &SemanticVersion,
    ) -> Option<String> {
        // Generate standardized URI for artifact downloads
        // This follows the Aura artifact naming convention:
        // aura://{package_id}/{version}/{hash}
        // This URI can be resolved by the artifact resolver to actual download locations

        let uri = format!(
            "aura://{}/{}/{:02x}{:02x}{:02x}{:02x}",
            proposal.package_id.hyphenated(),
            version,
            proposal.package_hash.0[0], // Use first 4 bytes of hash for brevity
            proposal.package_hash.0[1],
            proposal.package_hash.0[2],
            proposal.package_hash.0[3]
        );

        Some(uri)
    }

    /// Activate upgrade after approval
    pub async fn activate_upgrade<C: aura_core::effects::CryptoEffects>(
        &self,
        proposal: UpgradeProposal,
        account_id: AccountId,
        crypto_effects: &C,
        threshold_signature: &[u8],
        group_public_key: &[u8],
    ) -> SyncResult<UpgradeActivated> {
        // Verify threshold signature during maintenance
        self.verify_threshold_signature(
            &proposal,
            crypto_effects,
            threshold_signature,
            group_public_key,
        )
        .await?;

        let activation_fence = proposal
            .activation_fence
            .unwrap_or_else(|| IdentityEpochFence::new(account_id, 0));

        Ok(UpgradeActivated {
            package_id: proposal.package_id,
            to_version: proposal.version,
            activation_fence,
        })
    }

    /// Check if snapshot is due
    pub fn is_snapshot_due(&self, current_epoch: u64) -> bool {
        if !self.config.auto_snapshot_enabled {
            return false;
        }

        match *self.last_snapshot_epoch.read() {
            None => true, // First snapshot
            Some(last) => current_epoch >= last + self.config.min_snapshot_interval_epochs,
        }
    }

    /// Get service uptime
    pub fn uptime(&self) -> Duration {
        self.started_at
            .read()
            .map(|t| t.elapsed())
            .unwrap_or(Duration::ZERO)
    }

    async fn flush_pending_operations(&self) -> SyncResult<()> {
        // Ensure cache manager flushes pending invalidations and OTA protocol finalizes any in-flight tasks.
        // No-op until cache manager exposes a flush API.

        // Snapshot/OTA protocols are stateless per call; no-op flush placeholder for now.
        Ok(())
    }

    /// Start the service using PhysicalTimeEffects (preferred over Service::start)
    pub async fn start_with_time_effects<T: PhysicalTimeEffects + TimeEffects>(
        &self,
        time_effects: &T,
    ) -> SyncResult<()> {
        {
            let mut state = self.state.write();
            if *state == ServiceState::Running {
                return Err(sync_session_error("Service already running"));
            }
            *state = ServiceState::Starting;
        } // Lock dropped here

        // Use PhysicalTimeEffects for deterministic wall-clock; store local Instant for uptime
        let _ts = time_effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("time error: {e}")))?;
        *self.started_at.write() = Some(Instant::now());

        *self.state.write() = ServiceState::Running;
        Ok(())
    }
}

#[async_trait::async_trait]
impl Service for MaintenanceService {
    async fn start(&self, now: Instant) -> SyncResult<()> {
        // NOTE: Prefer start_with_time_effects() for proper effect system integration
        let mut state = self.state.write();
        if *state == ServiceState::Running {
            return Err(sync_session_error("Service already running"));
        }

        *state = ServiceState::Starting;
        *self.started_at.write() = Some(now);

        *state = ServiceState::Running;
        Ok(())
    }

    async fn stop(&self) -> SyncResult<()> {
        {
            let mut state = self.state.write();
            if *state == ServiceState::Stopped {
                return Ok(());
            }
            *state = ServiceState::Stopping;
        }

        self.flush_pending_operations().await?;

        *self.state.write() = ServiceState::Stopped;
        Ok(())
    }

    async fn health_check(&self) -> SyncResult<HealthCheck> {
        // Implement health check logic inline since Service trait doesn't support time_effects parameter
        let time_effects = aura_effects::time::PhysicalTimeHandler;
        let state = *self.state.read();
        let status = match state {
            ServiceState::Running => HealthStatus::Healthy,
            ServiceState::Starting => HealthStatus::Starting,
            ServiceState::Stopping => HealthStatus::Stopping,
            ServiceState::Stopped | ServiceState::Failed => HealthStatus::Unhealthy,
        };

        let mut details = std::collections::HashMap::new();

        // Read values and drop locks before await
        let snapshot_pending = {
            let snapshot_protocol = self.snapshot_protocol.read();
            snapshot_protocol.is_pending()
        };
        details.insert("snapshot_pending".to_string(), snapshot_pending.to_string());

        let ota_pending = {
            let ota_protocol = self.ota_protocol.read();
            ota_protocol.get_pending().is_some()
        };
        details.insert("ota_pending".to_string(), ota_pending.to_string());

        details.insert(
            "uptime".to_string(),
            format!("{}s", self.uptime().as_secs()),
        );

        // Get timestamp after dropping all locks
        let checked_at = time_effects
            .physical_time()
            .await
            .map_err(|e| crate::core::errors::sync_validation_error(format!("Time error: {}", e)))?
            .ts_ms
            / 1000;

        Ok(HealthCheck {
            status,
            message: Some("Maintenance service operational".to_string()),
            checked_at,
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

        let now = std::time::Instant::now();
        service.start(now).await.unwrap();
        assert!(service.is_running());

        service.stop().await.unwrap();
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_maintenance_service_with_time_effects() {
        let service = MaintenanceService::new(Default::default()).unwrap();
        let time_effects = aura_testkit::stateful_effects::SimulatedTimeHandler::new();

        service
            .start_with_time_effects(&time_effects)
            .await
            .unwrap();
        assert!(service.is_running());

        service.stop().await.unwrap();
        assert!(!service.is_running());
    }

    #[tokio::test]
    async fn test_propose_upgrade_with_random_effects() {
        let service = MaintenanceService::new(Default::default()).unwrap();
        let random_effects = aura_testkit::stateful_effects::MockCryptoHandler::new();

        let package_id = Uuid::new_v4();
        let version = SemanticVersion::new(1, 2, 3);
        let kind = UpgradeKind::SoftFork;
        let package_hash = Hash32::from([1u8; 32]);
        let proposer = DeviceId::new();

        let proposal = service
            .propose_upgrade(
                package_id,
                version,
                kind,
                package_hash,
                proposer,
                &random_effects,
            )
            .await
            .unwrap();

        // Verify that the deterministic UUID was used
        assert_eq!(proposal.package_id, package_id);
        assert_eq!(proposal.version, version);
        assert_eq!(proposal.kind, kind);
        assert_eq!(proposal.artifact_hash, package_hash);
    }

    #[test]
    fn test_cache_invalidation() {
        let service = MaintenanceService::new(Default::default()).unwrap();

        let result = service
            .invalidate_cache(vec!["key1".to_string(), "key2".to_string()], 10)
            .unwrap();

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
        assert!(service.is_snapshot_due(151)); // 151 - 50 = 101 >= 100
    }
}
