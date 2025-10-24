// BeeKEM CGKA events for journal integration

use crate::types::*;
use aura_journal::DeviceId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a CGKA operation
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OperationId(pub Uuid);

impl OperationId {
    pub fn new_with_effects(effects: &aura_crypto::Effects) -> Self {
        Self(effects.gen_uuid())
    }
}

impl Default for OperationId {
    fn default() -> Self {
        // Note: Default implementation uses non-deterministic UUID
        // Prefer using new_with_effects() for deterministic behavior
        #[allow(clippy::disallowed_methods)]
        Self(Uuid::new_v4())
    }
}

/// BeeKEM group key agreement operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyhiveCgkaOperation {
    pub operation_id: OperationId,
    pub group_id: String,
    pub current_epoch: Epoch,
    pub target_epoch: Epoch,
    pub operation_type: CgkaOperationType,
    pub roster_delta: RosterDelta,
    pub tree_updates: Vec<TreeUpdate>,
    pub issued_by: DeviceId,
    pub issued_at: u64,
    pub signature: Vec<u8>,
}

impl KeyhiveCgkaOperation {
    pub fn new(
        group_id: String,
        current_epoch: Epoch,
        operation_type: CgkaOperationType,
        roster_delta: RosterDelta,
        tree_updates: Vec<TreeUpdate>,
        issued_by: DeviceId,
        effects: &aura_crypto::Effects,
    ) -> Self {
        Self {
            operation_id: OperationId::new_with_effects(effects),
            group_id,
            current_epoch,
            target_epoch: current_epoch.next(),
            operation_type,
            roster_delta,
            tree_updates,
            issued_by,
            issued_at: effects.now().unwrap_or(0),
            signature: Vec::new(), // To be filled by signing process
        }
    }
    
    /// Compute hash for signing/verification
    pub fn hash(&self) -> crate::Result<[u8; 32]> {
        let bytes = serde_json::to_vec(self)
            .map_err(|e| crate::CgkaError::SerializationError(e.to_string()))?;
        Ok(*blake3::hash(&bytes).as_bytes())
    }
}

/// Type of CGKA operation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CgkaOperationType {
    /// Add new members to the group
    Add { members: Vec<MemberId> },
    /// Remove members from the group
    Remove { members: Vec<MemberId> },
    /// Update tree without changing membership
    Update,
    /// Initialize new group
    Init { initial_members: Vec<MemberId> },
}

/// Changes to group roster
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterDelta {
    pub added_members: Vec<MemberId>,
    pub removed_members: Vec<MemberId>,
    pub previous_size: u32,
    pub new_size: u32,
}

impl RosterDelta {
    pub fn empty() -> Self {
        Self {
            added_members: Vec::new(),
            removed_members: Vec::new(),
            previous_size: 0,
            new_size: 0,
        }
    }
    
    pub fn add_members(members: Vec<MemberId>, previous_size: u32) -> Self {
        let new_size = previous_size + members.len() as u32;
        Self {
            added_members: members,
            removed_members: Vec::new(),
            previous_size,
            new_size,
        }
    }
    
    pub fn remove_members(members: Vec<MemberId>, previous_size: u32) -> Self {
        let new_size = previous_size - members.len() as u32;
        Self {
            added_members: Vec::new(),
            removed_members: members,
            previous_size,
            new_size,
        }
    }
}

/// Tree update operation for BeeKEM
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeUpdate {
    pub position: TreePosition,
    pub update_type: TreeUpdateType,
    pub path_updates: Vec<PathUpdate>,
}

/// Type of tree update
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TreeUpdateType {
    /// Add new leaf node
    AddLeaf {
        member_id: MemberId,
        key_package: KeyPackage,
    },
    /// Remove leaf node
    RemoveLeaf {
        member_id: MemberId,
    },
    /// Update existing node
    UpdateNode {
        new_public_key: PublicKey,
    },
}

/// Update to a node in the tree path
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathUpdate {
    pub position: TreePosition,
    pub public_key: PublicKey,
    pub encrypted_secret: Vec<u8>,
}

/// CGKA state synchronization event
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CgkaStateSyncEvent {
    pub group_id: String,
    pub epoch: Epoch,
    pub roster_snapshot: Roster,
    pub tree_snapshot: Vec<TreeNode>,
    pub application_secrets: Vec<ApplicationSecret>,
    pub sync_timestamp: u64,
}

/// CGKA epoch transition event
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CgkaEpochTransitionEvent {
    pub group_id: String,
    pub previous_epoch: Epoch,
    pub new_epoch: Epoch,
    pub roster_delta: RosterDelta,
    pub committed_operations: Vec<OperationId>,
    pub transition_timestamp: u64,
}

