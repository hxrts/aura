//! Maintenance workflows (snapshots, GC, OTA wiring).
//!
//! This module wires `aura-sync` helpers (snapshot manager, writer fence)
//! into the agent runtime so operator tooling can trigger maintenance flows.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use aura_core::{
    from_slice, hash_canonical,
    maintenance::{AdminReplaced, MaintenanceEvent},
    serialization::to_vec,
    tree::{Epoch as TreeEpoch, LeafId, NodeIndex, Policy, Snapshot},
    AccountId, AuraError, DeviceId, Hash32,
};
use aura_protocol::effects::{
    AuraEffectSystem, ConsoleEffects, LedgerEffects, StorageEffects, TimeEffects, TreeEffects,
};
use aura_sync::{SnapshotManager, WriterFence};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::errors::Result;

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
}

impl MaintenanceController {
    /// Create a controller bound to the agent's effect system.
    pub fn new(effects: Arc<RwLock<AuraEffectSystem>>, device_id: DeviceId) -> Self {
        Self {
            effects,
            device_id,
            snapshot_manager: SnapshotManager::new(),
        }
    }

    /// Expose the writer fence so other subsystems can respect the snapshot barrier.
    pub fn writer_fence(&self) -> WriterFence {
        self.snapshot_manager.fence()
    }

    /// Propose + commit a `Snapshot_v1`, persisting the blob and emitting maintenance events.
    pub async fn propose_snapshot(&self) -> Result<SnapshotOutcome> {
        let effects = self.clone_effects().await;

        let target_epoch = LedgerEffects::current_epoch(&effects)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get current epoch: {}", e)))?;
        let timestamp = LedgerEffects::current_timestamp(&effects)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to get current timestamp: {}", e)))?;
        let commitment = TreeEffects::get_current_commitment(&effects)
            .await
            .unwrap_or(Hash32([0u8; 32]));
        let snapshot = self
            .build_snapshot(commitment, target_epoch as TreeEpoch, timestamp)
            .map_err(|e| AuraError::internal(format!("snapshot synthesis failed: {}", e)))?;
        let state_digest = hash_canonical(&snapshot)
            .map_err(|e| AuraError::serialization(format!("snapshot hash failed: {}", e)))?;

        let (fence_guard, proposed) = self
            .snapshot_manager
            .propose(self.device_id, target_epoch as TreeEpoch, state_digest)
            .map_err(|e| AuraError::coordination_failed(e.to_string()))?;
        self.append_event(&effects, &proposed).await?;

        let mut participants = BTreeSet::new();
        participants.insert(self.device_id);
        let (completed, proposal_id) = self
            .snapshot_manager
            .complete(snapshot.clone(), participants, vec![])
            .map_err(|e| AuraError::coordination_failed(e.to_string()))?;

        self.store_snapshot_blob(&effects, proposal_id, &snapshot)
            .await?;
        self.append_event(&effects, &completed).await?;
        self.cleanup_snapshot_blobs(&effects, proposal_id).await?;
        self.prune_local_state(&effects, snapshot.epoch).await?;

        drop(fence_guard);
        effects.log_info(
            &format!(
                "Snapshot {} committed at epoch {} (digest {:02x?})",
                proposal_id,
                snapshot.epoch,
                &state_digest[..4]
            ),
            &[],
        );

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
        let effects = self.clone_effects().await;
        let record = AdminReplaced::new(account_id, self.device_id, new_admin, activation_epoch);
        let event = MaintenanceEvent::AdminReplaced(record.clone());
        self.append_event(&effects, &event).await?;
        self.store_admin_override(&effects, &record).await?;
        effects.log_info(
            &format!(
                "Admin replacement stub recorded for account {} (new admin {}, activation epoch {})",
                account_id, new_admin, activation_epoch
            ),
            &[],
        );
        Ok(())
    }

    /// Return the locally stored admin replacement (if any).
    pub async fn current_admin_override(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AdminReplaced>> {
        let effects = self.clone_effects().await;
        let key = admin_override_key(&account_id);
        match StorageEffects::retrieve(&effects, &key)
            .await
            .map_err(|e| AuraError::storage(format!("load admin override: {}", e)))?
        {
            Some(bytes) => {
                let record: AdminReplaced = aura_core::from_slice(&bytes).map_err(|e| {
                    AuraError::serialization(format!("decode admin override: {}", e))
                })?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
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

    async fn cleanup_snapshot_blobs(&self, effects: &AuraEffectSystem, keep: Uuid) -> Result<()> {
        let prefix = "maintenance:snapshot:";
        let keys = StorageEffects::list_keys(effects, Some(prefix))
            .await
            .unwrap_or_default();
        for key in keys {
            if !key.ends_with(&keep.to_string()) {
                let _ = StorageEffects::remove(effects, &key).await;
            }
        }
        Ok(())
    }

    async fn prune_local_state(
        &self,
        effects: &AuraEffectSystem,
        snapshot_epoch: TreeEpoch,
    ) -> Result<()> {
        // Drop cached maintenance markers whose epoch is older than the snapshot fence.
        let cache_prefix = "maintenance:cache_epoch:";
        let keys = StorageEffects::list_keys(effects, Some(cache_prefix))
            .await
            .unwrap_or_default();
        for key in keys {
            if let Some(epoch) = Self::parse_epoch_suffix(&key) {
                if epoch < snapshot_epoch {
                    let _ = StorageEffects::remove(effects, &key).await;
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
                    let _ = StorageEffects::remove(effects, &key).await;
                }
            }
        }
        Ok(())
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

    async fn clone_effects(&self) -> AuraEffectSystem {
        self.effects.read().await.clone()
    }

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
