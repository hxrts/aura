//! Authority-internal tree state
//!
//! This module provides the internal TreeState implementation that hides
//! device structure from external view, using LeafId as the internal handle.

use crate::commitment_tree::local_types::{ExternalLeafView, LocalLeafNode};
use aura_core::{
    tree::{commit_branch, commit_leaf, BranchNode, Epoch, LeafId, NodeIndex, Policy, TreeHash32},
    Hash32,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Child reference in the authority-internal branch topology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
enum ChildRef {
    Branch(NodeIndex),
    Leaf(LeafId),
}

/// Authority-internal tree state with hidden device structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthorityTreeState {
    /// Current epoch.
    pub epoch: Epoch,

    /// Root commitment hash.
    pub root_commitment: TreeHash32,

    /// Branch nodes.
    pub branches: BTreeMap<NodeIndex, BranchNode>,

    /// Leaf nodes keyed by the stable internal leaf handle.
    leaves: BTreeMap<LeafId, LocalLeafNode>,

    /// Leaf commitments.
    leaf_commitments: BTreeMap<LeafId, TreeHash32>,

    /// Next leaf ID counter.
    next_leaf_id: u32,

    /// Current threshold policy parameter.
    threshold: u16,

    /// Active leaf indices.
    active_leaves: BTreeSet<LeafId>,

    /// Cached Merkle proof paths for each leaf.
    merkle_paths: BTreeMap<LeafId, Vec<Vec<u8>>>,

    /// Root for the plain Merkle proof tree built from leaf commitments.
    #[serde(default)]
    merkle_root: TreeHash32,

    /// Cached FROST key shares per leaf (cleared on structural changes).
    frost_key_cache: BTreeMap<LeafId, Vec<u8>>,

    /// Leaves that require a DKG refresh.
    pending_dkg_devices: BTreeSet<LeafId>,

    /// Scheduled DKG runs keyed by epoch for deterministic replay.
    scheduled_dkg_epochs: BTreeMap<Epoch, BTreeSet<LeafId>>,

    /// Recorded notifications for devices and relying parties.
    dkg_notifications: Vec<DkgNotification>,

    /// Root branch in the current topology.
    #[serde(default)]
    root_branch: Option<NodeIndex>,

    /// Parent pointer for each non-root branch.
    #[serde(default)]
    branch_parent: BTreeMap<NodeIndex, NodeIndex>,

    /// Ordered children for each branch.
    #[serde(default)]
    branch_children: BTreeMap<NodeIndex, (ChildRef, ChildRef)>,

    /// Parent branch for each active leaf.
    #[serde(default)]
    leaf_parent: BTreeMap<LeafId, NodeIndex>,

    /// Branch depth cache (root = 0).
    #[serde(default)]
    branch_depth: BTreeMap<NodeIndex, u32>,

    /// Number of unique active leaves in each branch subtree.
    #[serde(default)]
    branch_leaf_counts: BTreeMap<NodeIndex, u32>,

    /// Dirty branches awaiting recomputation.
    #[serde(default)]
    dirty_branches: BTreeSet<NodeIndex>,

    /// Number of branch commitments recomputed in the last flush.
    #[serde(default)]
    last_recomputed_branch_count: u32,

    /// Branches touched during the last flush; used for scoped proof refresh.
    #[serde(default)]
    last_recomputed_branches: BTreeSet<NodeIndex>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DkgNotification {
    device_id: LeafId,
    epoch: Epoch,
    kind: DkgNotificationKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
enum DkgNotificationKind {
    KeyShareStale,
    RotationScheduled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TempBranch {
    left: TempChild,
    right: TempChild,
    unique_leaf_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TempChild {
    Branch(u32),
    Leaf(LeafId),
}

impl AuthorityTreeState {
    /// Create a new empty tree state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Epoch::initial(),
            root_commitment: [0; 32],
            branches: BTreeMap::new(),
            leaves: BTreeMap::new(),
            leaf_commitments: BTreeMap::new(),
            next_leaf_id: 0,
            threshold: 1,
            active_leaves: BTreeSet::new(),
            merkle_paths: BTreeMap::new(),
            merkle_root: [0u8; 32],
            frost_key_cache: BTreeMap::new(),
            pending_dkg_devices: BTreeSet::new(),
            scheduled_dkg_epochs: BTreeMap::new(),
            dkg_notifications: Vec::new(),
            root_branch: None,
            branch_parent: BTreeMap::new(),
            branch_children: BTreeMap::new(),
            leaf_parent: BTreeMap::new(),
            branch_depth: BTreeMap::new(),
            branch_leaf_counts: BTreeMap::new(),
            dirty_branches: BTreeSet::new(),
            last_recomputed_branch_count: 0,
            last_recomputed_branches: BTreeSet::new(),
        }
    }

    /// Add a device with public key, returns the assigned leaf index.
    pub fn add_device(&mut self, public_key: Vec<u8>) -> LeafId {
        let leaf_id = LeafId(self.next_leaf_id);
        self.next_leaf_id = self.next_leaf_id.saturating_add(1);

        let leaf = LocalLeafNode::new(leaf_id, public_key);
        self.leaves.insert(leaf_id, leaf);
        self.active_leaves.insert(leaf_id);

        self.refresh_after_structural_change();
        debug_assert!(self.assert_topology_invariants().is_ok());

        leaf_id
    }

    /// Get external view of leaves (no device info).
    #[must_use]
    pub fn get_external_leaves(&self) -> BTreeMap<LeafId, ExternalLeafView> {
        self.leaves
            .iter()
            .map(|(id, leaf)| (*id, leaf.to_external()))
            .collect()
    }

    /// Get public key for a leaf.
    #[must_use]
    pub fn get_leaf_public_key(&self, leaf_id: LeafId) -> Option<&[u8]> {
        self.leaves
            .get(&leaf_id)
            .map(|leaf| leaf.public_key.as_slice())
    }

    /// Update public key for an existing active leaf.
    pub fn update_leaf_public_key(
        &mut self,
        leaf_id: LeafId,
        public_key: Vec<u8>,
    ) -> Result<(), aura_core::AuraError> {
        if !self.active_leaves.contains(&leaf_id) {
            return Err(aura_core::AuraError::not_found(format!(
                "Leaf {} not found or inactive",
                leaf_id.0
            )));
        }

        let leaf = self.leaves.get_mut(&leaf_id).ok_or_else(|| {
            aura_core::AuraError::not_found(format!("Leaf {} not found", leaf_id.0))
        })?;
        leaf.public_key = public_key;

        self.refresh_leaf_commitment(leaf_id);
        self.update_branch_policies();
        self.mark_dirty_from_leaf(leaf_id);
        self.recompute_subtree_commitment();
        self.update_merkle_proof_paths();

        debug_assert!(self.assert_topology_invariants().is_ok());
        Ok(())
    }

    /// Remove a device by leaf index.
    pub fn remove_device(&mut self, leaf_index: u32) -> Result<(), aura_core::AuraError> {
        let leaf_id = LeafId(leaf_index);

        if !self.active_leaves.contains(&leaf_id) {
            return Err(aura_core::AuraError::not_found(format!(
                "Leaf {leaf_index} not found or already removed"
            )));
        }

        self.active_leaves.remove(&leaf_id);
        self.leaf_commitments.remove(&leaf_id);
        self.leaf_parent.remove(&leaf_id);
        self.frost_key_cache.remove(&leaf_id);
        self.pending_dkg_devices.remove(&leaf_id);

        self.refresh_after_structural_change();
        debug_assert!(self.assert_topology_invariants().is_ok());

        Ok(())
    }

    /// Update threshold policy.
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
        self.update_branch_policies();
        self.mark_all_branches_dirty();
        self.recompute_subtree_commitment();
        self.update_merkle_proof_paths();

        debug_assert!(self.assert_topology_invariants().is_ok());
        Ok(())
    }

    /// Rotate epoch (invalidates old shares).
    pub fn rotate_epoch(&mut self) -> Result<(), aura_core::AuraError> {
        self.epoch = self.epoch.next()?;
        self.invalidate_cached_key_shares();

        // Epoch is part of all commitments; all branches are affected.
        self.refresh_leaf_commitments_for_active();
        self.update_branch_policies();
        self.mark_all_branches_dirty();
        self.recompute_subtree_commitment();
        self.update_merkle_proof_paths();

        debug_assert!(self.assert_topology_invariants().is_ok());
        Ok(())
    }

    /// Get current threshold.
    #[must_use]
    pub fn get_threshold(&self) -> u16 {
        self.threshold
    }

    /// Get number of active leaves.
    #[must_use]
    pub fn active_leaf_count(&self) -> usize {
        self.active_leaves.len()
    }

    /// Get root public key (for threshold operations).
    #[must_use]
    pub fn root_public_key(&self) -> Option<Vec<u8>> {
        self.active_leaves
            .iter()
            .find_map(|leaf_id| self.leaves.get(leaf_id))
            .map(|leaf| leaf.public_key.clone())
    }

    /// Get encoded Merkle proof for one leaf.
    #[must_use]
    pub fn merkle_proof(&self, leaf_id: LeafId) -> Option<&[Vec<u8>]> {
        self.merkle_paths.get(&leaf_id).map(std::vec::Vec::as_slice)
    }

    /// Validate one provided proof path against current tree state.
    #[must_use]
    pub fn verify_merkle_proof_path(&self, leaf_id: LeafId, path: &[Vec<u8>]) -> bool {
        let Some(leaf) = self.leaves.get(&leaf_id) else {
            return false;
        };

        let mut current = commit_leaf(leaf_id, self.epoch, &leaf.public_key);
        for element in path {
            let Some((sibling_is_left, sibling_hash)) = Self::decode_proof_element(element) else {
                return false;
            };

            let mut hasher = aura_core::hash::hasher();
            if sibling_is_left {
                hasher.update(&sibling_hash);
                hasher.update(&current);
            } else {
                hasher.update(&current);
                hasher.update(&sibling_hash);
            }
            current = hasher.finalize();
        }

        current == self.merkle_root
    }

    /// Validate the currently cached proof path for one leaf.
    #[must_use]
    pub fn verify_merkle_proof(&self, leaf_id: LeafId) -> bool {
        self.merkle_paths
            .get(&leaf_id)
            .is_some_and(|path| self.verify_merkle_proof_path(leaf_id, path))
    }

    /// Number of branches recomputed by the last incremental flush.
    #[must_use]
    pub fn last_recomputed_branch_count(&self) -> u32 {
        self.last_recomputed_branch_count
    }

    /// Recompute root commitment using a full reference pass.
    ///
    /// This intentionally ignores dirty-path optimization and recomputes every
    /// branch bottom-up to provide a correctness cross-check for tests.
    #[must_use]
    pub fn recompute_root_commitment_full(&self) -> TreeHash32 {
        let mut leaf_commitments = BTreeMap::new();
        for leaf_id in &self.active_leaves {
            if let Some(leaf) = self.leaves.get(leaf_id) {
                leaf_commitments.insert(
                    *leaf_id,
                    commit_leaf(*leaf_id, self.epoch, &leaf.public_key),
                );
            }
        }

        let mut branch_commitments = BTreeMap::new();
        let mut nodes: Vec<_> = self.branches.keys().copied().collect();
        nodes.sort_by_key(|node| {
            let depth = self.branch_depth.get(node).copied().unwrap_or(0);
            (std::cmp::Reverse(depth), node.0)
        });

        for node in nodes {
            let Some((left, right)) = self.branch_children.get(&node).copied() else {
                continue;
            };
            let left_hash =
                Self::child_commitment_from_maps(left, &leaf_commitments, &branch_commitments);
            let right_hash =
                Self::child_commitment_from_maps(right, &leaf_commitments, &branch_commitments);
            let policy = self
                .branches
                .get(&node)
                .map(|b| b.policy)
                .unwrap_or_else(|| {
                    self.policy_for_subtree(
                        self.branch_leaf_counts.get(&node).copied().unwrap_or(1),
                    )
                });

            let commitment = commit_branch(node, self.epoch, &policy, &left_hash, &right_hash);
            branch_commitments.insert(node, commitment);
        }

        let root_hash = self
            .root_branch
            .and_then(|node| branch_commitments.get(&node).copied())
            .unwrap_or([0u8; 32]);
        self.finalize_tree_commitment(root_hash.to_vec()).0
    }

    /// Validate topology invariants.
    pub fn validate_topology_invariants(&self) -> Result<(), aura_core::AuraError> {
        self.assert_topology_invariants()
            .map_err(aura_core::AuraError::invalid)
    }

    /// Update tree structure and recompute commitments after structural changes.
    fn refresh_after_structural_change(&mut self) {
        self.rebuild_topology_from_active_leaves();
        self.refresh_leaf_commitments_for_active();
        self.update_branch_policies();
        self.mark_all_branches_dirty();
        self.recompute_subtree_commitment();
        self.invalidate_cached_key_shares();
        self.update_merkle_proof_paths();
    }

    fn refresh_leaf_commitment(&mut self, leaf_id: LeafId) {
        let Some(leaf) = self.leaves.get(&leaf_id) else {
            self.leaf_commitments.remove(&leaf_id);
            return;
        };
        let commitment = commit_leaf(leaf_id, self.epoch, &leaf.public_key);
        self.leaf_commitments.insert(leaf_id, commitment);
    }

    fn refresh_leaf_commitments_for_active(&mut self) {
        let active: BTreeSet<_> = self.active_leaves.iter().copied().collect();
        self.leaf_commitments
            .retain(|leaf, _| active.contains(leaf));
        let active_leaves: Vec<_> = self.active_leaves.iter().copied().collect();
        for leaf_id in active_leaves {
            self.refresh_leaf_commitment(leaf_id);
        }
    }

    fn rebuild_topology_from_active_leaves(&mut self) {
        self.root_branch = None;
        self.branch_parent.clear();
        self.branch_children.clear();
        self.leaf_parent.clear();
        self.branch_depth.clear();
        self.branch_leaf_counts.clear();
        self.branches.clear();
        self.dirty_branches.clear();

        let mut leaves: Vec<LeafId> = self.active_leaves.iter().copied().collect();
        leaves.sort();

        if leaves.is_empty() {
            self.root_commitment = self.finalize_tree_commitment(vec![0u8; 32]).0;
            self.merkle_paths.clear();
            self.merkle_root = [0u8; 32];
            return;
        }

        let mut temp_branches: BTreeMap<u32, TempBranch> = BTreeMap::new();
        let mut next_temp_id: u32 = 0;

        let mut level: Vec<TempChild> = leaves.into_iter().map(TempChild::Leaf).collect();

        while level.len() > 1 {
            let mut next_level = Vec::new();
            for pair in level.chunks(2) {
                let left = pair[0];
                let right = pair.get(1).copied().unwrap_or(left);
                let left_count = Self::temp_child_leaf_count(left, &temp_branches);
                let right_count = Self::temp_child_leaf_count(right, &temp_branches);
                let unique_leaf_count = if left == right {
                    left_count
                } else {
                    left_count.saturating_add(right_count)
                };
                temp_branches.insert(
                    next_temp_id,
                    TempBranch {
                        left,
                        right,
                        unique_leaf_count,
                    },
                );
                next_level.push(TempChild::Branch(next_temp_id));
                next_temp_id = next_temp_id.saturating_add(1);
            }
            level = next_level;
        }

        let root_temp = match level.first().copied() {
            Some(TempChild::Branch(root)) => root,
            Some(TempChild::Leaf(leaf)) => {
                let root = next_temp_id;
                temp_branches.insert(
                    root,
                    TempBranch {
                        left: TempChild::Leaf(leaf),
                        right: TempChild::Leaf(leaf),
                        unique_leaf_count: 1,
                    },
                );
                root
            }
            None => return,
        };

        let mut temp_to_branch: BTreeMap<u32, NodeIndex> = BTreeMap::new();
        let mut queue = VecDeque::new();
        temp_to_branch.insert(root_temp, NodeIndex(0));
        self.root_branch = Some(NodeIndex(0));
        self.branch_depth.insert(NodeIndex(0), 0);
        queue.push_back(root_temp);
        let mut next_node_index: u32 = 1;

        while let Some(temp_id) = queue.pop_front() {
            let node_index = temp_to_branch[&temp_id];
            let depth = self.branch_depth.get(&node_index).copied().unwrap_or(0);
            let Some(temp_branch) = temp_branches.get(&temp_id) else {
                continue;
            };

            for child in [temp_branch.left, temp_branch.right] {
                if let TempChild::Branch(child_temp_id) = child {
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        temp_to_branch.entry(child_temp_id)
                    {
                        let child_node = NodeIndex(next_node_index);
                        next_node_index = next_node_index.saturating_add(1);
                        entry.insert(child_node);
                        self.branch_parent.insert(child_node, node_index);
                        self.branch_depth
                            .insert(child_node, depth.saturating_add(1));
                        queue.push_back(child_temp_id);
                    }
                }
            }
        }

        let mut ordered: Vec<(NodeIndex, u32)> = temp_to_branch
            .iter()
            .map(|(temp, node)| (*node, *temp))
            .collect();
        ordered.sort_by_key(|(node, _)| node.0);

        for (node, temp_id) in ordered {
            let Some(temp_branch) = temp_branches.get(&temp_id) else {
                continue;
            };

            let left = Self::map_temp_child(temp_branch.left, &temp_to_branch);
            let right = Self::map_temp_child(temp_branch.right, &temp_to_branch);
            self.branch_children.insert(node, (left, right));

            if let ChildRef::Leaf(leaf) = left {
                self.leaf_parent.insert(leaf, node);
            }
            if let ChildRef::Leaf(leaf) = right {
                self.leaf_parent.insert(leaf, node);
            }

            let leaf_count = temp_branch.unique_leaf_count.max(1);
            self.branch_leaf_counts.insert(node, leaf_count);
            let policy = self.policy_for_subtree(leaf_count);

            self.branches.insert(
                node,
                BranchNode {
                    node,
                    policy,
                    commitment: [0u8; 32],
                },
            );
        }
    }

    fn map_temp_child(child: TempChild, mapping: &BTreeMap<u32, NodeIndex>) -> ChildRef {
        match child {
            TempChild::Branch(temp) => ChildRef::Branch(mapping[&temp]),
            TempChild::Leaf(leaf) => ChildRef::Leaf(leaf),
        }
    }

    fn temp_child_leaf_count(child: TempChild, branches: &BTreeMap<u32, TempBranch>) -> u32 {
        match child {
            TempChild::Leaf(_) => 1,
            TempChild::Branch(branch) => branches
                .get(&branch)
                .map(|entry| entry.unique_leaf_count)
                .unwrap_or(1),
        }
    }

    fn policy_for_subtree(&self, leaf_count: u32) -> Policy {
        let n = u16::try_from(leaf_count.max(1)).unwrap_or(u16::MAX);
        let m = self.threshold.clamp(1, n);

        if n == 1 || m == n {
            Policy::All
        } else if m == 1 {
            Policy::Any
        } else {
            Policy::Threshold { m, n }
        }
    }

    fn update_branch_policies(&mut self) {
        let updates: Vec<_> = self
            .branch_leaf_counts
            .iter()
            .map(|(node, leaf_count)| (*node, self.policy_for_subtree(*leaf_count)))
            .collect();

        for (node, policy) in updates {
            if let Some(branch) = self.branches.get_mut(&node) {
                branch.policy = policy;
            }
        }
    }

    fn mark_all_branches_dirty(&mut self) {
        self.dirty_branches = self.branches.keys().copied().collect();
    }

    fn mark_dirty_from_branch(&mut self, node: NodeIndex) {
        if self.branches.contains_key(&node) {
            self.dirty_branches.insert(node);
        }
    }

    fn mark_dirty_from_leaf(&mut self, leaf_id: LeafId) {
        if let Some(parent) = self.leaf_parent.get(&leaf_id).copied() {
            self.mark_dirty_from_branch(parent);
        }
    }

    fn collect_dirty_paths_to_root(&self) -> BTreeSet<NodeIndex> {
        let mut affected = BTreeSet::new();

        for branch in &self.dirty_branches {
            let mut current = Some(*branch);
            while let Some(node) = current {
                if !affected.insert(node) {
                    break;
                }
                current = self.branch_parent.get(&node).copied();
            }
        }

        affected
    }

    fn flush_dirty_commitments_bottom_up(&mut self) {
        let affected = self.collect_dirty_paths_to_root();
        if affected.is_empty() {
            self.last_recomputed_branch_count = 0;
            self.last_recomputed_branches.clear();
            self.update_root_commitment_from_topology();
            return;
        }

        let mut nodes: Vec<_> = affected.into_iter().collect();
        nodes.sort_by_key(|node| {
            let depth = self.branch_depth.get(node).copied().unwrap_or(0);
            (std::cmp::Reverse(depth), node.0)
        });

        for node in &nodes {
            let commitment = self.recompute_branch_commitment(*node);
            if let Some(branch) = self.branches.get_mut(node) {
                branch.commitment = commitment;
            }
        }

        self.last_recomputed_branch_count = u32::try_from(nodes.len()).unwrap_or(u32::MAX);
        self.last_recomputed_branches = nodes.iter().copied().collect();
        self.dirty_branches.clear();
        self.update_root_commitment_from_topology();
    }

    fn recompute_subtree_commitment(&mut self) {
        self.flush_dirty_commitments_bottom_up();
    }

    fn recompute_branch_commitment(&self, node: NodeIndex) -> TreeHash32 {
        let Some((left, right)) = self.branch_children.get(&node).copied() else {
            return [0u8; 32];
        };

        let left_hash = self.child_commitment(left);
        let right_hash = self.child_commitment(right);
        let policy = self
            .branches
            .get(&node)
            .map(|branch| branch.policy)
            .unwrap_or_else(|| {
                self.policy_for_subtree(self.branch_leaf_counts.get(&node).copied().unwrap_or(1))
            });

        commit_branch(node, self.epoch, &policy, &left_hash, &right_hash)
    }

    fn child_commitment(&self, child: ChildRef) -> TreeHash32 {
        match child {
            ChildRef::Leaf(leaf) => self
                .leaf_commitments
                .get(&leaf)
                .copied()
                .unwrap_or([0u8; 32]),
            ChildRef::Branch(node) => self
                .branches
                .get(&node)
                .map(|branch| branch.commitment)
                .unwrap_or([0u8; 32]),
        }
    }

    fn child_commitment_from_maps(
        child: ChildRef,
        leaf_commitments: &BTreeMap<LeafId, TreeHash32>,
        branch_commitments: &BTreeMap<NodeIndex, TreeHash32>,
    ) -> TreeHash32 {
        match child {
            ChildRef::Leaf(leaf) => leaf_commitments.get(&leaf).copied().unwrap_or([0u8; 32]),
            ChildRef::Branch(branch) => branch_commitments
                .get(&branch)
                .copied()
                .unwrap_or([0u8; 32]),
        }
    }

    fn update_root_commitment_from_topology(&mut self) {
        let root_hash = self
            .root_branch
            .and_then(|root| self.branches.get(&root).map(|branch| branch.commitment))
            .unwrap_or([0u8; 32]);
        self.root_commitment = self.finalize_tree_commitment(root_hash.to_vec()).0;
    }

    fn update_merkle_proof_paths(&mut self) {
        if self.active_leaves.is_empty() {
            self.merkle_paths.clear();
            self.merkle_root = [0u8; 32];
            return;
        }

        let Some(root) = self.root_branch else {
            self.merkle_paths.clear();
            self.merkle_root = [0u8; 32];
            return;
        };

        // Build plain subtree hashes from the materialized topology so proof
        // paths share the same deterministic shape as branch commitments.
        let mut plain_branch_hashes: BTreeMap<NodeIndex, TreeHash32> = BTreeMap::new();
        let mut nodes: Vec<_> = self.branches.keys().copied().collect();
        nodes.sort_by_key(|node| {
            let depth = self.branch_depth.get(node).copied().unwrap_or(0);
            (std::cmp::Reverse(depth), node.0)
        });
        for node in nodes {
            let Some((left, right)) = self.branch_children.get(&node).copied() else {
                continue;
            };
            let left_hash = self.child_plain_hash(left, &plain_branch_hashes);
            let right_hash = self.child_plain_hash(right, &plain_branch_hashes);
            let mut hasher = aura_core::hash::hasher();
            hasher.update(&left_hash);
            hasher.update(&right_hash);
            plain_branch_hashes.insert(node, hasher.finalize());
        }

        self.merkle_root = plain_branch_hashes.get(&root).copied().unwrap_or([0u8; 32]);

        self.merkle_paths
            .retain(|leaf, _| self.active_leaves.contains(leaf));

        let target_leaves = if self.merkle_paths.len() != self.active_leaves.len() {
            self.active_leaves.iter().copied().collect()
        } else {
            self.collect_affected_leaves_for_proof_refresh()
        };

        for leaf in target_leaves {
            let path = self.compute_merkle_proof_for_leaf(leaf, &plain_branch_hashes);
            self.merkle_paths.insert(leaf, path);
        }
    }

    fn child_plain_hash(
        &self,
        child: ChildRef,
        branch_hashes: &BTreeMap<NodeIndex, TreeHash32>,
    ) -> TreeHash32 {
        match child {
            ChildRef::Leaf(leaf) => self
                .leaf_commitments
                .get(&leaf)
                .copied()
                .unwrap_or([0u8; 32]),
            ChildRef::Branch(node) => branch_hashes.get(&node).copied().unwrap_or([0u8; 32]),
        }
    }

    fn collect_affected_leaves_for_proof_refresh(&self) -> BTreeSet<LeafId> {
        let mut affected = BTreeSet::new();

        if self.last_recomputed_branches.is_empty() {
            return affected;
        }

        for branch in &self.last_recomputed_branches {
            self.collect_subtree_leaves(ChildRef::Branch(*branch), &mut affected);

            if let Some(parent) = self.branch_parent.get(branch).copied() {
                let Some((left, right)) = self.branch_children.get(&parent).copied() else {
                    continue;
                };
                let sibling = if left == ChildRef::Branch(*branch) {
                    right
                } else if right == ChildRef::Branch(*branch) {
                    left
                } else {
                    continue;
                };
                self.collect_subtree_leaves(sibling, &mut affected);
            }
        }

        affected
    }

    fn collect_subtree_leaves(&self, child: ChildRef, out: &mut BTreeSet<LeafId>) {
        let mut stack = vec![child];
        while let Some(current) = stack.pop() {
            match current {
                ChildRef::Leaf(leaf) => {
                    out.insert(leaf);
                }
                ChildRef::Branch(branch) => {
                    if let Some((left, right)) = self.branch_children.get(&branch).copied() {
                        stack.push(right);
                        stack.push(left);
                    }
                }
            }
        }
    }

    fn compute_merkle_proof_for_leaf(
        &self,
        leaf: LeafId,
        plain_branch_hashes: &BTreeMap<NodeIndex, TreeHash32>,
    ) -> Vec<Vec<u8>> {
        let mut path = Vec::new();
        let mut child_ref = ChildRef::Leaf(leaf);
        let mut parent = self.leaf_parent.get(&leaf).copied();

        while let Some(branch) = parent {
            let Some((left, right)) = self.branch_children.get(&branch).copied() else {
                break;
            };

            let (sibling_is_left, sibling_ref) = if left == child_ref {
                (false, right)
            } else if right == child_ref {
                (true, left)
            } else {
                break;
            };

            let sibling_hash = self.child_plain_hash(sibling_ref, plain_branch_hashes);
            path.push(Self::encode_proof_element(sibling_is_left, sibling_hash));

            child_ref = ChildRef::Branch(branch);
            parent = self.branch_parent.get(&branch).copied();
        }

        path
    }

    fn encode_proof_element(sibling_is_left: bool, hash: TreeHash32) -> Vec<u8> {
        let mut out = Vec::with_capacity(33);
        out.push(if sibling_is_left { 1 } else { 0 });
        out.extend_from_slice(&hash);
        out
    }

    fn decode_proof_element(raw: &[u8]) -> Option<(bool, TreeHash32)> {
        if raw.len() != 33 {
            return None;
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&raw[1..]);
        Some((raw[0] == 1, hash))
    }

    fn invalidate_cached_key_shares(&mut self) {
        self.clear_frost_key_cache();
        self.mark_key_derivation_stale();
        self.mark_devices_for_key_regeneration();
        self.update_dkg_epoch_markers();
    }

    fn clear_frost_key_cache(&mut self) {
        self.frost_key_cache.clear();
    }

    fn mark_key_derivation_stale(&mut self) {
        // Topology/epoch changes invalidate in-memory derivation artifacts.
        self.merkle_paths.clear();
        self.merkle_root = [0u8; 32];
    }

    fn mark_devices_for_key_regeneration(&mut self) {
        let leaf_ids: Vec<_> = self.active_leaves.iter().copied().collect();
        for leaf_id in leaf_ids {
            self.queue_dkg_for_device(leaf_id);
        }
    }

    fn update_dkg_epoch_markers(&mut self) {
        if !self.pending_dkg_devices.is_empty() {
            self.scheduled_dkg_epochs
                .entry(self.epoch)
                .or_default()
                .extend(self.pending_dkg_devices.iter().copied());

            for device_id in self.pending_dkg_devices.iter().copied() {
                self.dkg_notifications.push(DkgNotification {
                    device_id,
                    epoch: self.epoch,
                    kind: DkgNotificationKind::RotationScheduled,
                });
            }
        }
    }

    fn queue_dkg_for_device(&mut self, device_id: LeafId) {
        self.pending_dkg_devices.insert(device_id);
        self.scheduled_dkg_epochs
            .entry(self.epoch)
            .or_default()
            .insert(device_id);

        self.dkg_notifications.push(DkgNotification {
            device_id,
            epoch: self.epoch,
            kind: DkgNotificationKind::KeyShareStale,
        });
    }

    fn finalize_tree_commitment(&self, root_hash: Vec<u8>) -> Hash32 {
        use aura_core::hash;
        let mut hasher = hash::hasher();

        hasher.update(b"TREE_COMMITMENT_V2");
        hasher.update(&u64::from(self.epoch).to_le_bytes());
        hasher.update(&self.threshold.to_le_bytes());
        hasher.update(&(self.active_leaves.len() as u32).to_le_bytes());
        hasher.update(&root_hash);

        Hash32::new(hasher.finalize())
    }

    fn assert_topology_invariants(&self) -> Result<(), String> {
        if self.active_leaves.is_empty() {
            if self.root_branch.is_some() || !self.branches.is_empty() {
                return Err("empty tree has unexpected root/branches".to_string());
            }
            if !self.leaf_parent.is_empty() || !self.branch_children.is_empty() {
                return Err("empty tree has topology edges".to_string());
            }
            return Ok(());
        }

        let Some(root) = self.root_branch else {
            return Err("non-empty tree missing root branch".to_string());
        };

        if root != NodeIndex(0) {
            return Err(format!("root must be NodeIndex(0), got {}", root.0));
        }

        for branch in self.branches.keys() {
            if *branch != root && !self.branch_parent.contains_key(branch) {
                return Err(format!("branch {} missing parent", branch.0));
            }
            if !self.branch_children.contains_key(branch) {
                return Err(format!("branch {} missing children", branch.0));
            }
            if !self.branch_depth.contains_key(branch) {
                return Err(format!("branch {} missing depth", branch.0));
            }
            if !self.branch_leaf_counts.contains_key(branch) {
                return Err(format!("branch {} missing leaf-count", branch.0));
            }
        }

        for leaf in &self.active_leaves {
            if !self.leaves.contains_key(leaf) {
                return Err(format!("active leaf {} missing node", leaf.0));
            }
            let Some(parent) = self.leaf_parent.get(leaf).copied() else {
                return Err(format!("leaf {} missing parent", leaf.0));
            };
            if !self.branches.contains_key(&parent) {
                return Err(format!(
                    "leaf {} parent {} missing branch",
                    leaf.0, parent.0
                ));
            }
        }

        // Check for cycles / invalid parent pointers.
        for branch in self.branches.keys() {
            let mut seen = BTreeSet::new();
            let mut current = Some(*branch);
            while let Some(node) = current {
                if !seen.insert(node) {
                    return Err(format!("cycle detected from branch {}", branch.0));
                }
                current = self.branch_parent.get(&node).copied();
            }
        }

        // Check child references.
        for (branch, (left, right)) in &self.branch_children {
            for child in [left, right] {
                match child {
                    ChildRef::Branch(child_branch) => {
                        if !self.branches.contains_key(child_branch) {
                            return Err(format!(
                                "branch {} references missing child branch {}",
                                branch.0, child_branch.0
                            ));
                        }
                        let parent = self.branch_parent.get(child_branch).copied();
                        if parent != Some(*branch) && *child_branch != *branch {
                            return Err(format!(
                                "child branch {} has parent {:?}, expected {}",
                                child_branch.0,
                                parent.map(|p| p.0),
                                branch.0
                            ));
                        }
                    }
                    ChildRef::Leaf(leaf) => {
                        if !self.active_leaves.contains(leaf) {
                            return Err(format!(
                                "branch {} references inactive/missing leaf {}",
                                branch.0, leaf.0
                            ));
                        }
                        if self.leaf_parent.get(leaf).copied() != Some(*branch) {
                            return Err(format!(
                                "leaf {} parent mismatch: expected {}",
                                leaf.0, branch.0
                            ));
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

impl Default for AuthorityTreeState {
    fn default() -> Self {
        Self::new()
    }
}
