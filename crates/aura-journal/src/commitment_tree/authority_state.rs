//! Authority-internal tree state
//!
//! This module provides the internal TreeState implementation that hides
//! device structure from external view. It maintains the same functionality
//! as the original TreeState but uses LocalDeviceId internally.

use crate::commitment_tree::local_types::{ExternalLeafView, LocalDeviceId, LocalLeafNode};
use aura_core::tree::{BranchNode, Epoch, LeafId, NodeIndex, TreeHash32};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Authority-internal tree state with hidden device structure
///
/// This replaces the public TreeState with one that doesn't expose
/// device identifiers externally.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityTreeState {
    /// Current epoch
    pub epoch: Epoch,

    /// Root commitment hash
    pub root_commitment: TreeHash32,

    /// Branch nodes (unchanged from original)
    pub branches: BTreeMap<NodeIndex, BranchNode>,

    /// Leaf nodes with internal device references
    leaves: BTreeMap<LeafId, LocalLeafNode>,

    /// Leaf commitments (unchanged)
    leaf_commitments: BTreeMap<LeafId, TreeHash32>,

    /// Device ID mapping (internal only)
    device_mapping: BTreeMap<LocalDeviceId, LeafId>,

    /// Next local device ID counter
    next_device_id: u32,

    /// Next leaf ID counter
    next_leaf_id: u32,

    /// Current threshold policy
    threshold: u16,

    /// Active leaf indices
    active_leaves: BTreeSet<LeafId>,
}

impl AuthorityTreeState {
    /// Create a new empty tree state
    pub fn new() -> Self {
        Self {
            epoch: 0,
            root_commitment: [0; 32],
            branches: BTreeMap::new(),
            leaves: BTreeMap::new(),
            leaf_commitments: BTreeMap::new(),
            device_mapping: BTreeMap::new(),
            next_device_id: 1,
            next_leaf_id: 0,
            threshold: 1,
            active_leaves: BTreeSet::new(),
        }
    }

    /// Add a device with public key, returns the assigned leaf index
    pub fn add_device(&mut self, public_key: Vec<u8>) -> LeafId {
        let local_id = LocalDeviceId::new(self.next_device_id);
        self.next_device_id += 1;

        let leaf_id = LeafId(self.next_leaf_id);
        self.next_leaf_id += 1;

        let leaf = LocalLeafNode::new(leaf_id, local_id, public_key);

        self.leaves.insert(leaf_id, leaf);
        self.device_mapping.insert(local_id, leaf_id);
        self.active_leaves.insert(leaf_id);

        // TODO: Update tree structure and recompute commitments
        self.recompute_commitments();

        leaf_id
    }

    /// Get external view of leaves (no device info)
    pub fn get_external_leaves(&self) -> BTreeMap<LeafId, ExternalLeafView> {
        self.leaves
            .iter()
            .map(|(id, leaf)| (*id, leaf.to_external()))
            .collect()
    }

    /// Get public key for a leaf
    pub fn get_leaf_public_key(&self, leaf_id: LeafId) -> Option<&[u8]> {
        self.leaves
            .get(&leaf_id)
            .map(|leaf| leaf.public_key.as_slice())
    }

    /// Internal: Look up leaf by local device ID (reserved for future use)
    #[allow(dead_code)]
    fn get_leaf_by_device(&self, device_id: LocalDeviceId) -> Option<&LocalLeafNode> {
        self.device_mapping
            .get(&device_id)
            .and_then(|leaf_id| self.leaves.get(leaf_id))
    }

    /// Remove a device by leaf index
    pub fn remove_device(&mut self, leaf_index: u32) -> Result<(), aura_core::AuraError> {
        let leaf_id = LeafId(leaf_index);

        if !self.active_leaves.contains(&leaf_id) {
            return Err(aura_core::AuraError::not_found(format!(
                "Leaf {} not found or already removed",
                leaf_index
            )));
        }

        // Remove from active set
        self.active_leaves.remove(&leaf_id);

        // Find and remove device mapping
        if let Some(_leaf) = self.leaves.get(&leaf_id) {
            // Remove device mapping (scan through mapping)
            let device_to_remove = self
                .device_mapping
                .iter()
                .find(|(_, lid)| **lid == leaf_id)
                .map(|(did, _)| *did);

            if let Some(device_id) = device_to_remove {
                self.device_mapping.remove(&device_id);
            }
        }

        // TODO: Rebalance tree structure
        self.recompute_commitments();

        Ok(())
    }

    /// Update threshold policy
    pub fn update_threshold(&mut self, new_threshold: u16) -> Result<(), aura_core::AuraError> {
        if new_threshold == 0 {
            return Err(aura_core::AuraError::invalid(
                "Threshold cannot be zero".to_string(),
            ));
        }

        if new_threshold as usize > self.active_leaves.len() {
            return Err(aura_core::AuraError::invalid(format!(
                "Threshold {} exceeds number of active leaves {}",
                new_threshold,
                self.active_leaves.len()
            )));
        }

        self.threshold = new_threshold;
        self.recompute_commitments();

        Ok(())
    }

    /// Rotate epoch (invalidates old shares)
    pub fn rotate_epoch(&mut self) -> Result<(), aura_core::AuraError> {
        self.epoch += 1;

        // TODO: Invalidate all cached key shares
        // This would typically involve:
        // 1. Clearing any cached threshold signature shares
        // 2. Updating epoch commitments
        // 3. Notifying devices of epoch change

        self.recompute_commitments();

        Ok(())
    }

    /// Recompute tree commitments after changes
    fn recompute_commitments(&mut self) {
        // TODO: Implement proper tree commitment computation
        // For now, use a simple hash of active state
        use aura_core::hash;

        let mut hasher = hash::hasher();
        hasher.update(&self.epoch.to_le_bytes());
        hasher.update(&self.threshold.to_le_bytes());

        // Hash active leaves
        for leaf_id in &self.active_leaves {
            hasher.update(&leaf_id.0.to_le_bytes());
            if let Some(leaf) = self.leaves.get(leaf_id) {
                hasher.update(&leaf.public_key);
            }
        }

        self.root_commitment = hasher.finalize();
    }

    /// Get current threshold
    pub fn get_threshold(&self) -> u16 {
        self.threshold
    }

    /// Get number of active leaves
    pub fn active_leaf_count(&self) -> usize {
        self.active_leaves.len()
    }

    /// Get root public key (for threshold operations)
    pub fn root_public_key(&self) -> Option<Vec<u8>> {
        // TODO: Derive from tree structure
        // For now, return first active leaf's key
        self.active_leaves
            .iter()
            .filter_map(|id| self.leaves.get(id))
            .next()
            .map(|leaf| leaf.public_key.clone())
    }
}

impl Default for AuthorityTreeState {
    fn default() -> Self {
        Self::new()
    }
}
