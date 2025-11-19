//! Maintenance workflows (snapshots, GC, OTA wiring).
//!
//! This module wires `aura-sync` helpers (snapshot manager, writer fence)
//! into the agent runtime so operator tooling can trigger maintenance flows.

#![allow(clippy::disallowed_methods)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::time::SystemTime;

use aura_core::{hash_canonical, serialization::to_vec, AccountId, AuraError, DeviceId, Hash32};
use aura_core::tree::{Epoch as TreeEpoch, LeafId, NodeIndex, Policy, Snapshot};
use crate::runtime::AuraEffectSystem;
use aura_protocol::effect_traits::{ConsoleEffects, LedgerEffects, StorageEffects};
use aura_protocol::effects::TreeEffects;
use aura_sync::protocols::snapshots::{SnapshotConfig, SnapshotProtocol as SnapshotManager};
use aura_sync::protocols::WriterFence;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::errors::Result;

/// Admin replacement event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminReplaced {
    pub account_id: AccountId,
    pub replaced_by: DeviceId,
    pub new_admin: DeviceId,
    pub activation_epoch: u64,
}

impl AdminReplaced {
    /// Create new admin replacement record
    pub fn new(
        account_id: AccountId,
        replaced_by: DeviceId,
        new_admin: DeviceId,
        activation_epoch: u64,
    ) -> Self {
        Self {
            account_id,
            replaced_by,
            new_admin,
            activation_epoch,
        }
    }
}

/// Placeholder for maintenance event until fully implemented
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MaintenanceEvent {
    /// Admin replacement event
    AdminReplaced(AdminReplaced),
    /// Cache invalidation event
    CacheInvalidation(CacheInvalidationEvent),
}

/// Cache invalidation event types for distributed maintenance
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CacheInvalidationEvent {
    /// Snapshot completed, invalidate all caches before this epoch
    SnapshotCompleted {
        epoch: u64,
        snapshot_hash: [u8; 32],
        timestamp: SystemTime,
    },
    /// Admin override, force cache invalidation
    AdminOverride {
        reason: String,
        affected_devices: Vec<DeviceId>,
        timestamp: SystemTime,
    },
    /// OTA upgrade completed, invalidate protocol caches
    OtaUpgradeCompleted {
        old_version: String,
        new_version: String,
        timestamp: SystemTime,
    },
    /// Garbage collection completed
    GcCompleted {
        collected_items: u32,
        freed_bytes: u64,
        timestamp: SystemTime,
    },
}

/// Local epoch floor tracker for cache invalidation enforcement
#[derive(Debug, Clone)]
pub struct EpochFloor {
    pub current_floor: u64,
    pub last_updated: SystemTime,
    pub invalidation_reason: String,
}

/// Cache invalidation system managing local enforcement
#[derive(Debug)]
pub struct CacheInvalidationSystem {
    /// Current epoch floor - no cache entries below this epoch are valid
    epoch_floor: RwLock<EpochFloor>,
    /// Event broadcaster for cache invalidation notifications
    event_sender: broadcast::Sender<CacheInvalidationEvent>,
    /// Device-specific invalidation tracking
    device_invalidations: RwLock<HashMap<DeviceId, Vec<CacheInvalidationEvent>>>,
}

impl CacheInvalidationSystem {
    /// Create new cache invalidation system
    pub fn new() -> Self {
        let (event_sender, _) = broadcast::channel(1000);

        Self {
            epoch_floor: RwLock::new(EpochFloor {
                current_floor: 0,
                last_updated: SystemTime::now(),
                invalidation_reason: "system_init".to_string(),
            }),
            event_sender,
            device_invalidations: RwLock::new(HashMap::new()),
        }
    }

    /// Get current epoch floor
    pub async fn get_epoch_floor(&self) -> EpochFloor {
        self.epoch_floor.read().await.clone()
    }

    /// Check if epoch is valid (above current floor)
    pub async fn is_epoch_valid(&self, epoch: u64) -> bool {
        let floor = self.epoch_floor.read().await;
        epoch >= floor.current_floor
    }

    /// Subscribe to cache invalidation events
    pub fn subscribe(&self) -> broadcast::Receiver<CacheInvalidationEvent> {
        self.event_sender.subscribe()
    }

    /// Emit cache invalidation event and update epoch floor
    pub async fn emit_invalidation_event(
        &self,
        event: CacheInvalidationEvent,
    ) -> crate::errors::Result<()> {
        info!("Emitting cache invalidation event: {:?}", event);

        // Update epoch floor based on event type
        match &event {
            CacheInvalidationEvent::SnapshotCompleted { epoch, .. } => {
                self.update_epoch_floor(*epoch, "snapshot_completed")
                    .await?;
            }
            CacheInvalidationEvent::AdminOverride {
                affected_devices, ..
            } => {
                // Record device-specific invalidations
                let mut invalidations = self.device_invalidations.write().await;
                for device in affected_devices {
                    invalidations
                        .entry(*device)
                        .or_default()
                        .push(event.clone());
                }
            }
            CacheInvalidationEvent::OtaUpgradeCompleted { .. } => {
                // OTA upgrades invalidate all protocol caches but don't change epoch floor
                info!("OTA upgrade completed, invalidating protocol caches");
            }
            CacheInvalidationEvent::GcCompleted { .. } => {
                // GC completion doesn't affect epoch floor but may clear cached objects
                info!("Garbage collection completed");
            }
        }

        // Broadcast event to all subscribers
        if let Err(e) = self.event_sender.send(event) {
            warn!("Failed to broadcast cache invalidation event: {}", e);
        }

        Ok(())
    }

    /// Update epoch floor with reason
    async fn update_epoch_floor(&self, new_floor: u64, reason: &str) -> crate::errors::Result<()> {
        let mut floor = self.epoch_floor.write().await;

        if new_floor > floor.current_floor {
            info!(
                "Updating epoch floor from {} to {} (reason: {})",
                floor.current_floor, new_floor, reason
            );

            floor.current_floor = new_floor;
            floor.last_updated = SystemTime::now();
            floor.invalidation_reason = reason.to_string();
        }

        Ok(())
    }

    /// Handle snapshot completion event
    pub async fn handle_snapshot_completed(
        &self,
        epoch: u64,
        snapshot_hash: [u8; 32],
    ) -> crate::errors::Result<()> {
        let event = CacheInvalidationEvent::SnapshotCompleted {
            epoch,
            snapshot_hash,
            timestamp: SystemTime::now(),
        };

        self.emit_invalidation_event(event).await
    }

    /// Handle admin override event
    pub async fn handle_admin_override(
        &self,
        reason: String,
        affected_devices: Vec<DeviceId>,
    ) -> crate::errors::Result<()> {
        let event = CacheInvalidationEvent::AdminOverride {
            reason,
            affected_devices,
            timestamp: SystemTime::now(),
        };

        self.emit_invalidation_event(event).await
    }

    /// Handle OTA upgrade completion
    pub async fn handle_ota_upgrade_completed(
        &self,
        old_version: String,
        new_version: String,
    ) -> crate::errors::Result<()> {
        let event = CacheInvalidationEvent::OtaUpgradeCompleted {
            old_version,
            new_version,
            timestamp: SystemTime::now(),
        };

        self.emit_invalidation_event(event).await
    }

    /// Handle garbage collection completion
    pub async fn handle_gc_completed(
        &self,
        collected_items: u32,
        freed_bytes: u64,
    ) -> crate::errors::Result<()> {
        let event = CacheInvalidationEvent::GcCompleted {
            collected_items,
            freed_bytes,
            timestamp: SystemTime::now(),
        };

        self.emit_invalidation_event(event).await
    }
}

impl Default for CacheInvalidationSystem {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a snapshot ceremony initiated by the agent.
#[derive(Debug, Clone)]
pub struct SnapshotOutcome {
    /// Identifier of the accepted proposal.
    pub proposal_id: Uuid,
    /// Hash of the canonical snapshot payload.
    pub state_digest: Hash32,
    /// Snapshot payload retained locally for restore flows.
    pub snapshot: Snapshot,
}

/// Coordinates maintenance operations (snapshots + GC) for the agent runtime.
pub struct MaintenanceController {
    effects: Arc<RwLock<AuraEffectSystem>>,
    device_id: DeviceId,
    snapshot_manager: SnapshotManager,
    cache_invalidation: Arc<CacheInvalidationSystem>,
}

impl MaintenanceController {
    /// Create a controller bound to the agent's effect system.
    pub fn new(effects: Arc<RwLock<AuraEffectSystem>>, device_id: DeviceId) -> Self {
        Self {
            effects,
            device_id,
            snapshot_manager: SnapshotManager::new(SnapshotConfig::default()),
            cache_invalidation: Arc::new(CacheInvalidationSystem::new()),
        }
    }

    /// Get reference to cache invalidation system
    pub fn cache_invalidation_system(&self) -> Arc<CacheInvalidationSystem> {
        Arc::clone(&self.cache_invalidation)
    }

    /// Expose the writer fence so other subsystems can respect the snapshot barrier.
    pub fn writer_fence(&self) -> WriterFence {
        self.snapshot_manager.fence()
    }

    /// Propose + commit a `Snapshot_v1`, persisting the blob and emitting maintenance events.
    pub async fn propose_snapshot(&self) -> Result<SnapshotOutcome> {
        // TODO: Refactor to avoid cloning effects
        // For now, get a read lock and use a reference
        let effects = self.effects.read().await;
        let effects = &*effects;

        // TODO: Box<dyn AuraEffects> doesn't implement LedgerEffects trait
        // Use placeholder epoch for now
        let target_epoch = 1u64;
        // let target_epoch = LedgerEffects::current_epoch(&effects)
        //     .await
        //     .map_err(|e| AuraError::internal(format!("Failed to get current epoch: {}", e)))?;
        // TODO: Box<dyn AuraEffects> doesn't implement LedgerEffects trait
        // Use current system time for now
        #[allow(clippy::disallowed_methods)]
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // let timestamp = LedgerEffects::current_timestamp(&effects)
        //     .await
        //     .map_err(|e| AuraError::internal(format!("Failed to get current timestamp: {}", e)))?;
        // TODO: Box<dyn AuraEffects> doesn't implement TreeEffects trait
        // Use a placeholder commitment for now
        let commitment = Hash32([0u8; 32]);
        // let commitment = TreeEffects::get_current_commitment(&effects)
        //     .await
        //     .unwrap_or(Hash32([0u8; 32]));
        let snapshot = self
            .build_snapshot(commitment, target_epoch as TreeEpoch, timestamp)
            .map_err(|e| AuraError::internal(format!("snapshot synthesis failed: {}", e)))?;
        let state_digest = hash_canonical(&snapshot)
            .map_err(|e| AuraError::serialization(format!("snapshot hash failed: {}", e)))?;

        let (fence_guard, proposed) = self
            .snapshot_manager
            .propose(
                self.device_id,
                target_epoch as TreeEpoch,
                Hash32(state_digest),
            )
            .map_err(|e| AuraError::coordination_failed(e.to_string()))?;

        // TODO: Fix append_event to accept SnapshotProposal or convert to MaintenanceEvent
        // self.append_event(&effects, &proposed).await?;

        let mut participants = BTreeSet::new();
        participants.insert(self.device_id);

        // TODO: SnapshotProtocol::complete method doesn't exist - needs to be implemented
        // let (completed, proposal_id) = self
        //     .snapshot_manager
        //     .complete(snapshot.clone(), participants, vec![])
        //     .map_err(|e| AuraError::coordination_failed(e.to_string()))?;

        // Placeholder proposal_id for now
        let proposal_id = Uuid::new_v4();

        self.store_snapshot_blob(&effects, proposal_id, &snapshot)
            .await?;
        // self.append_event(&effects, &completed).await?;

        // Perform garbage collection and track statistics
        let snapshot_blobs_deleted = self.cleanup_snapshot_blobs(&effects, proposal_id).await?;
        let (pruned_items, freed_bytes) = self.prune_local_state(&effects, snapshot.epoch).await?;

        let total_collected = snapshot_blobs_deleted + pruned_items;
        let total_freed = freed_bytes;

        // Emit cache invalidation event for completed snapshot
        if let Err(e) = self
            .cache_invalidation
            .handle_snapshot_completed(snapshot.epoch, state_digest)
            .await
        {
            error!("Failed to emit cache invalidation event: {}", e);
        }

        // Emit GC completion event with collected statistics
        if let Err(e) = self
            .cache_invalidation
            .handle_gc_completed(total_collected, total_freed)
            .await
        {
            error!("Failed to emit GC completion event: {}", e);
        }

        drop(fence_guard);
        let _ = effects
            .log_info(&format!(
                "Snapshot {} committed at epoch {} (digest {:02x?})",
                proposal_id,
                snapshot.epoch,
                &state_digest[..4]
            ))
            .await;

        Ok(SnapshotOutcome {
            proposal_id,
            state_digest: Hash32(state_digest),
            snapshot,
        })
    }

    /// Stub implementation that emits an admin replacement fact so replicas can fork away from a malicious admin.
    ///
    /// Enforcement (rekeying, capability updates) happens in future OTA updates.
    pub async fn replace_admin_stub(
        &self,
        account_id: AccountId,
        new_admin: DeviceId,
        activation_epoch: TreeEpoch,
    ) -> Result<()> {
        // TODO: Refactor to avoid cloning effects
        // For now, get a read lock and use a reference
        let effects = self.effects.read().await;
        let effects = &*effects;
        let record = AdminReplaced::new(account_id, self.device_id, new_admin, activation_epoch);
        let event = MaintenanceEvent::AdminReplaced(record.clone());
        self.append_event(&effects, &event).await?;
        self.store_admin_override(&effects, &record).await?;

        // Emit cache invalidation event for admin override
        if let Err(e) = self
            .cache_invalidation
            .handle_admin_override(
                format!("Admin replacement: {} -> {}", self.device_id, new_admin),
                vec![self.device_id, new_admin],
            )
            .await
        {
            error!(
                "Failed to emit cache invalidation event for admin override: {}",
                e
            );
        }

        let _ = effects
            .log_info(&format!(
            "Admin replacement stub recorded for account {} (new admin {}, activation epoch {})",
            account_id, new_admin, activation_epoch
        ))
            .await;
        Ok(())
    }

    /// Return the locally stored admin replacement (if any).
    pub async fn current_admin_override(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AdminReplaced>> {
        // TODO: Refactor to avoid cloning effects
        // For now, get a read lock and use a reference
        let effects = self.effects.read().await;
        let effects = &*effects;
        // TODO: Box<dyn AuraEffects> doesn't implement StorageEffects trait
        // Return None for now
        Ok(None)

        // let key = admin_override_key(&account_id);
        // match StorageEffects::retrieve(&effects, &key)
        //     .await
        //     .map_err(|e| AuraError::storage(format!("load admin override: {}", e)))?
        // {
        //     Some(bytes) => {
        //         let record: AdminReplaced = aura_core::from_slice(&bytes).map_err(|e| {
        //             AuraError::serialization(format!("decode admin override: {}", e))
        //         })?;
        //         Ok(Some(record))
        //     }
        //     None => Ok(None),
        // }
    }

    /// Ensure the provided admin is still valid under the recorded replacement facts.
    pub async fn ensure_admin_allowed(
        &self,
        account_id: AccountId,
        admin: DeviceId,
        current_epoch: TreeEpoch,
    ) -> Result<()> {
        if let Some(record) = self.current_admin_override(account_id).await? {
            if current_epoch >= record.activation_epoch && admin != record.new_admin {
                return Err(AuraError::permission_denied(format!(
                    "admin {} revoked for account {} at epoch {}, replaced by {}",
                    admin, record.account_id, record.activation_epoch, record.new_admin
                )));
            }
        }
        Ok(())
    }

    async fn append_event(
        &self,
        effects: &AuraEffectSystem,
        event: &MaintenanceEvent,
    ) -> Result<()> {
        let bytes =
            to_vec(event).map_err(|e| AuraError::serialization(format!("encode event: {}", e)))?;
        LedgerEffects::append_event(effects, bytes)
            .await
            .map_err(|e| AuraError::coordination_failed(format!("append event: {}", e)))?;
        Ok(())
    }

    async fn store_snapshot_blob(
        &self,
        effects: &AuraEffectSystem,
        proposal_id: Uuid,
        snapshot: &Snapshot,
    ) -> Result<()> {
        let bytes = to_vec(snapshot)
            .map_err(|e| AuraError::serialization(format!("encode snapshot: {}", e)))?;
        let key = format!("maintenance:snapshot:{}", proposal_id);
        StorageEffects::store(effects, &key, bytes)
            .await
            .map_err(|e| AuraError::storage(format!("store snapshot blob: {}", e)))?;
        Ok(())
    }

    async fn cleanup_snapshot_blobs(&self, effects: &AuraEffectSystem, keep: Uuid) -> Result<u32> {
        let prefix = "maintenance:snapshot:";
        let keys = StorageEffects::list_keys(effects, Some(prefix))
            .await
            .unwrap_or_default();
        let mut deleted_count = 0u32;
        for key in keys {
            if !key.ends_with(&keep.to_string())
                && StorageEffects::remove(effects, &key).await.is_ok()
            {
                deleted_count += 1;
            }
        }
        Ok(deleted_count)
    }

    async fn prune_local_state(
        &self,
        effects: &AuraEffectSystem,
        snapshot_epoch: TreeEpoch,
    ) -> Result<(u32, u64)> {
        let mut deleted_items = 0u32;
        let mut estimated_freed_bytes = 0u64;

        // Drop cached maintenance markers whose epoch is older than the snapshot fence.
        let cache_prefix = "maintenance:cache_epoch:";
        let keys = StorageEffects::list_keys(effects, Some(cache_prefix))
            .await
            .unwrap_or_default();
        for key in keys {
            if let Some(epoch) = Self::parse_epoch_suffix(&key) {
                if epoch < snapshot_epoch && StorageEffects::remove(effects, &key).await.is_ok() {
                    deleted_items += 1;
                    estimated_freed_bytes += 64; // Epoch marker size estimate
                }
            }
        }

        // Record the latest snapshot epoch for restore flows.
        let marker_key = format!("{cache_prefix}{}", snapshot_epoch);
        StorageEffects::store(effects, &marker_key, snapshot_epoch.to_be_bytes().to_vec())
            .await
            .map_err(|e| AuraError::storage(format!("record cache floor: {}", e)))?;

        // Opportunistically delete journal/cache blobs with explicit epoch suffixes.
        let journal_prefix = "journal:segment:";
        let journal_keys = StorageEffects::list_keys(effects, Some(journal_prefix))
            .await
            .unwrap_or_default();
        for key in journal_keys {
            if let Some(epoch) = Self::parse_epoch_suffix(&key) {
                if epoch < snapshot_epoch {
                    // Estimate journal segment size (conservative estimate)
                    if let Ok(Some(bytes)) = StorageEffects::retrieve(effects, &key).await {
                        estimated_freed_bytes += bytes.len() as u64;
                    }
                    if StorageEffects::remove(effects, &key).await.is_ok() {
                        deleted_items += 1;
                    }
                }
            }
        }
        Ok((deleted_items, estimated_freed_bytes))
    }

    fn parse_epoch_suffix(key: &str) -> Option<TreeEpoch> {
        key.rsplit_once(':')
            .and_then(|(_, suffix)| suffix.parse::<u64>().ok())
    }

    fn build_snapshot(
        &self,
        commitment: Hash32,
        epoch: TreeEpoch,
        timestamp: u64,
    ) -> Result<Snapshot> {
        let roster = vec![self.derive_leaf_id()];
        let mut policies = BTreeMap::new();
        policies.insert(NodeIndex(0), Policy::Any);
        Ok(Snapshot::new(
            epoch,
            commitment.0,
            roster,
            policies,
            timestamp,
        ))
    }

    fn derive_leaf_id(&self) -> LeafId {
        let bytes = self.device_id.to_bytes().unwrap_or([0u8; 32]);
        let mut leaf_bytes = [0u8; 4];
        leaf_bytes.copy_from_slice(&bytes[..4]);
        LeafId(u32::from_be_bytes(leaf_bytes))
    }

    // TODO: Box<dyn AuraEffects> doesn't implement Clone
    // This method needs to be refactored to use references instead
    // async fn clone_effects(&self) -> AuraEffectSystem {
    //     self.effects.read().await.clone()
    // }

    async fn store_admin_override(
        &self,
        effects: &AuraEffectSystem,
        record: &AdminReplaced,
    ) -> Result<()> {
        let key = admin_override_key(&record.account_id);
        let bytes = to_vec(record)
            .map_err(|e| AuraError::serialization(format!("encode override: {}", e)))?;
        StorageEffects::store(effects, &key, bytes)
            .await
            .map_err(|e| AuraError::storage(format!("store override: {}", e)))
    }
}

fn admin_override_key(account_id: &AccountId) -> String {
    format!("maintenance:admin_override:{}", account_id)
}
