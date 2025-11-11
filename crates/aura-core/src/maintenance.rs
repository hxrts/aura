//! Shared maintenance events and helpers.
//!
//! These types back the day-one maintenance plan (`docs/501_dist_maintenance.md`).

use crate::{tree::Snapshot, AccountId, DeviceId, Epoch, Hash32, SemanticVersion};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

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

/// Upgrade flavor (soft vs hard fork).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpgradeKind {
    /// Soft fork: optional adoption, no automatic fence.
    SoftFork,
    /// Hard fork: mandatory activation at the fence.
    HardFork,
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
    pub fn validate(&self) -> crate::AuraResult<()> {
        if matches!(self.kind, UpgradeKind::HardFork) && self.activation_fence.is_none() {
            return Err(crate::AuraError::invalid(
                "hard fork proposals must include an activation fence",
            ));
        }
        Ok(())
    }
}
