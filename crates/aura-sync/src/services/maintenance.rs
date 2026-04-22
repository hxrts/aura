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
//! ```rust,ignore
//! use aura_sync::services::{MaintenanceService, MaintenanceServiceConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = MaintenanceServiceConfig::default();
//! let service = MaintenanceService::new(config)?;
//!
//! // Propose snapshot
//! service
//!     .propose_snapshot(authority_id, target_epoch, state_digest)
//!     .await?;
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

mod health;
mod state;
#[cfg(test)]
mod tests;
mod upgrade;

use parking_lot::RwLock;
use std::collections::BTreeSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    begin_service_start, begin_service_stop, finish_service_start, finish_service_stop,
    HealthCheck, MonotonicInstant, Service, ServiceState,
};
use crate::core::SyncResult;
use crate::infrastructure::CacheManager;
use crate::protocols::{OTAConfig, OTAProtocol, SnapshotConfig, SnapshotProtocol, UpgradeKind};
use aura_core::effects::{PhysicalTimeEffects, RandomEffects};
use aura_core::types::Epoch;
use aura_core::{
    tree::Snapshot, AccountId, AuraError, AuthorityId, Hash32, SemanticVersion, TrustedKeyResolver,
};
use aura_maintenance::{
    CacheInvalidated, CacheKey, IdentityEpochFence, SnapshotCompleted, SnapshotProposed,
    UpgradeActivated, UpgradeProposalMetadata,
};

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
    state: RwLock<ServiceState>,

    /// Snapshot protocol
    snapshot_protocol: RwLock<SnapshotProtocol>,

    /// OTA protocol
    ota_protocol: RwLock<OTAProtocol>,

    /// Cache manager
    cache_manager: RwLock<CacheManager>,

    /// Service start time
    started_at: RwLock<Option<MonotonicInstant>>,

    /// Last snapshot epoch
    last_snapshot_epoch: RwLock<Option<Epoch>>,
}

impl MaintenanceService {
    /// Create a new maintenance service
    pub fn new(config: MaintenanceServiceConfig) -> SyncResult<Self> {
        let snapshot_protocol = SnapshotProtocol::new(config.snapshot.clone());
        let ota_protocol = OTAProtocol::new(config.ota.clone());
        let cache_manager = CacheManager::new();

        Ok(Self {
            config,
            state: RwLock::new(ServiceState::Stopped),
            snapshot_protocol: RwLock::new(snapshot_protocol),
            ota_protocol: RwLock::new(ota_protocol),
            cache_manager: RwLock::new(cache_manager),
            started_at: RwLock::new(None),
            last_snapshot_epoch: RwLock::new(None),
        })
    }

    /// Propose a snapshot
    pub async fn propose_snapshot(
        &self,
        proposer: AuthorityId,
        target_epoch: Epoch,
        state_digest: Hash32,
    ) -> SyncResult<SnapshotProposed> {
        let protocol = self.snapshot_protocol.write();

        // Derive a deterministic proposal id from the state digest to avoid entropy usage.
        let mut id_bytes = [0u8; 16];
        id_bytes.copy_from_slice(&state_digest.0[..16]);
        let proposal_id = Uuid::from_bytes(id_bytes);

        let (_guard, proposal) =
            protocol.propose(proposer, target_epoch, state_digest, proposal_id)?;

        Ok(SnapshotProposed::new(
            proposal.proposer,
            proposal.proposal_id,
            proposal.target_epoch,
            proposal.state_digest,
        ))
    }

    /// Complete a snapshot
    pub async fn complete_snapshot(
        &self,
        authority_id: AuthorityId,
        proposal_id: Uuid,
        snapshot: Snapshot,
        participants: BTreeSet<AuthorityId>,
        threshold_signature: Vec<u8>,
    ) -> SyncResult<SnapshotCompleted> {
        *self.last_snapshot_epoch.write() = Some(snapshot.epoch);

        Ok(SnapshotCompleted::new(
            authority_id,
            proposal_id,
            snapshot,
            participants,
            threshold_signature,
        ))
    }

    /// Invalidate cache keys
    pub fn invalidate_cache(
        &self,
        authority_id: AuthorityId,
        keys: Vec<String>,
        epoch_floor: Epoch,
    ) -> SyncResult<CacheInvalidated> {
        let mut cache = self.cache_manager.write();
        cache.invalidate_keys(&keys, epoch_floor);

        let wrapped_keys = keys.into_iter().map(CacheKey).collect();
        Ok(CacheInvalidated::new(
            authority_id,
            wrapped_keys,
            epoch_floor,
        ))
    }

    /// Propose OTA upgrade
    pub async fn propose_upgrade<R: RandomEffects>(
        &self,
        package_id: Uuid,
        version: SemanticVersion,
        kind: UpgradeKind,
        package_hash: Hash32,
        proposer: AuthorityId,
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

    /// Activate upgrade after approval
    pub async fn activate_upgrade<C: aura_core::effects::CryptoEffects>(
        &self,
        authority_id: AuthorityId,
        proposal: UpgradeProposal,
        account_id: AccountId,
        crypto_effects: &C,
        key_resolver: &impl TrustedKeyResolver,
        threshold_signature: &[u8],
    ) -> SyncResult<UpgradeActivated> {
        // Verify threshold signature during maintenance
        self.verify_threshold_signature(
            authority_id,
            &proposal,
            crypto_effects,
            key_resolver,
            threshold_signature,
        )
        .await?;

        let activation_fence = proposal
            .activation_fence
            .unwrap_or_else(|| IdentityEpochFence::new(account_id, Epoch::new(0)));

        let version = proposal.version;
        Ok(UpgradeActivated::new(
            authority_id,
            proposal.package_id,
            proposal.version,
            activation_fence,
            UpgradeProposalMetadata {
                package_id: proposal.package_id,
                version,
                artifact_hash: proposal.artifact_hash,
            },
        ))
    }

    /// Start the service using PhysicalTimeEffects (preferred over Service::start)
    ///
    /// # Arguments
    /// - `time_effects`: Time effects provider
    /// - `now_instant`: Current monotonic time instant (obtain from runtime layer)
    pub async fn start_with_time_effects<T: PhysicalTimeEffects>(
        &self,
        time_effects: &T,
        now_instant: MonotonicInstant,
    ) -> SyncResult<()> {
        begin_service_start(&self.state, &self.started_at, now_instant)?;

        // Use PhysicalTimeEffects for deterministic wall-clock; store MonotonicInstant for uptime tracking
        let _ts = time_effects
            .physical_time()
            .await
            .map_err(|e| AuraError::internal(format!("time error: {e}")))?;
        finish_service_start(&self.state);
        Ok(())
    }
}

#[async_trait::async_trait]
impl Service for MaintenanceService {
    async fn start(&self, now: MonotonicInstant) -> SyncResult<()> {
        // NOTE: Prefer start_with_time_effects() for proper effect system integration
        begin_service_start(&self.state, &self.started_at, now)?;
        finish_service_start(&self.state);
        Ok(())
    }

    async fn stop(&self, _now: MonotonicInstant) -> SyncResult<()> {
        if !begin_service_stop(&self.state) {
            return Ok(());
        }

        self.flush_pending_operations().await?;

        finish_service_stop(&self.state);
        Ok(())
    }

    async fn health_check(&self) -> SyncResult<HealthCheck> {
        self.build_health_check().await
    }

    fn name(&self) -> &str {
        "MaintenanceService"
    }

    fn is_running(&self) -> bool {
        self.state.read().is_running()
    }
}
