//! Authority-internal tree state
//!
//! This module provides the internal TreeState implementation that hides
//! device structure from external view. It maintains the same functionality
//! as the original TreeState but uses LocalDeviceId internally.

use crate::ratchet_tree::local_types::{ExternalLeafView, LocalDeviceId, LocalLeafNode};
use aura_core::tree::{BranchNode, Epoch, LeafId, NodeIndex, Policy, TreeHash32};
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
        }
    }

    /// Add a device with public key, returns the assigned leaf index
    pub fn add_device(&mut self, public_key: Vec<u8>) -> LeafId {
        let local_id = LocalDeviceId::new(self.next_device_id);
        self.next_device_id += 1;

        let leaf_id = LeafId(self.leaves.len() as u32);
        let leaf = LocalLeafNode::new(leaf_id, local_id, public_key);

        self.leaves.insert(leaf_id, leaf);
        self.device_mapping.insert(local_id, leaf_id);

        // TODO: Update tree structure and recompute commitments

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

    /// Internal: Look up leaf by local device ID
    fn get_leaf_by_device(&self, device_id: LocalDeviceId) -> Option<&LocalLeafNode> {
        self.device_mapping
            .get(&device_id)
            .and_then(|leaf_id| self.leaves.get(leaf_id))
    }

    /// Get root public key (for threshold operations)
    pub fn root_public_key(&self) -> Option<Vec<u8>> {
        // TODO: Derive from tree structure
        // For now, return first leaf's key
        self.leaves
            .values()
            .next()
            .map(|leaf| leaf.public_key.clone())
    }
}

impl Default for AuthorityTreeState {
    fn default() -> Self {
        Self::new()
    }
}
