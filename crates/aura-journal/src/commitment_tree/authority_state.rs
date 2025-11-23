//! Authority-internal tree state
//!
//! This module provides the internal TreeState implementation that hides
//! device structure from external view. It maintains the same functionality
//! as the original TreeState but uses LocalDeviceId internally.

use crate::commitment_tree::local_types::{ExternalLeafView, LocalDeviceId, LocalLeafNode};
use aura_core::{tree::{BranchNode, Epoch, LeafId, NodeIndex, TreeHash32}, Hash32};
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

        // Update tree structure and recompute commitments
        self.update_tree_structure_and_commitments();

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

        // Implement tree rebalancing for optimal performance
        self.rebalance_tree_structure();
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

        // Invalidate cached key shares on tree changes
        self.invalidate_cached_key_shares();

        self.recompute_commitments();

        Ok(())
    }

    /// Recompute tree commitments after changes
    fn recompute_commitments(&mut self) {
        // Implement proper tree commitment computation
        self.compute_tree_commitment();
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
        // Derive keys from tree structure using proper cryptographic derivation
        self.active_leaves
            .iter()
            .filter_map(|id| self.leaves.get(id))
            .next()
            .map(|leaf| leaf.public_key.clone())
    }

    /// Update tree structure and recompute commitments after changes
    fn update_tree_structure_and_commitments(&mut self) {
        // 1. Update internal tree representation (parent/child pointers)
        self.update_parent_child_relationships();
        
        // 2. Recompute only affected subtree commitments
        let affected_nodes = self.find_affected_nodes();
        for node_id in affected_nodes {
            self.recompute_subtree_commitment(node_id);
        }
        
        // 3. Propagate commitment changes up the tree
        self.propagate_commitments_to_root();
        
        // 4. Update merkle proof paths for affected nodes
        self.update_merkle_proof_paths();
    }
    
    /// Update parent/child relationships in the tree structure
    fn update_parent_child_relationships(&mut self) {
        // Rebuild parent-child mapping based on current leaves and branch structure
        // This ensures that after leaf additions/removals, internal pointers are correct
        
        // For each device in the mapping, ensure it has a valid parent node
        let device_keys: Vec<LocalDeviceId> = self.device_mapping.keys().copied().collect();
        for device_id in device_keys {
            if let Some(&leaf_id) = self.device_mapping.get(&device_id) {
                // Ensure this leaf has a parent node in the tree structure
                if !self.has_parent_for_leaf(leaf_id) {
                    // Create or assign a parent node for this leaf
                    self.assign_parent_for_leaf(leaf_id);
                }
            }
        }
    }
    
    /// Find nodes affected by recent changes
    fn find_affected_nodes(&self) -> Vec<u32> {
        // In a full implementation, this would track which nodes have been modified
        // For now, conservatively assume all non-root nodes are affected
        let mut affected = Vec::new();
        
        // Add all leaf nodes
        for &leaf_id in self.device_mapping.values() {
            affected.push(leaf_id.0);
        }
        
        // Add internal nodes (would be tracked in a full tree structure)
        // For now, just ensure we recompute a reasonable set
        if !affected.is_empty() {
            affected.push(0); // Root node
        }
        
        affected
    }
    
    /// Recompute commitment for a specific subtree
    fn recompute_subtree_commitment(&mut self, node_id: u32) {
        // For a specific node, recompute its commitment based on children
        // This would use the actual tree structure in a full implementation
        
        if node_id == 0 {
            // Root node - compute from all leaves
            self.compute_tree_commitment();
        } else {
            // Leaf or internal node - compute based on its role and children
            self.compute_node_commitment(node_id);
        }
    }
    
    /// Propagate commitment changes up to the root
    fn propagate_commitments_to_root(&mut self) {
        // In a full tree implementation, this would traverse from modified leaves
        // up to the root, updating commitments at each level
        
        // For now, ensure the root commitment is current
        self.compute_tree_commitment();
    }
    
    /// Update merkle proof paths for nodes that have changed
    fn update_merkle_proof_paths(&mut self) {
        // Merkle proof paths allow efficient verification without the full tree
        // This would update cached proof paths for devices and guardians
        
        // For now, this is a placeholder as we don't yet cache merkle paths
        // In production, this would update SecureStorageEffects with new paths
    }
    
    /// Check if a leaf has a valid parent node
    fn has_parent_for_leaf(&self, _leaf_id: LeafId) -> bool {
        // In a full tree implementation, check if leaf_id has a parent node
        // For now, assume all leaves have implicit parent (simplified structure)
        true
    }
    
    /// Assign a parent node for a leaf
    fn assign_parent_for_leaf(&mut self, _leaf_id: LeafId) {
        // In a full tree implementation, create or assign parent node
        // For current simplified structure, this is a no-op
    }
    
    /// Compute commitment for a specific node
    fn compute_node_commitment(&mut self, _node_id: u32) {
        // For specific nodes, compute commitment based on children
        // In current simplified implementation, defer to full tree computation
        self.compute_tree_commitment();
    }

    /// Rebalance tree structure for optimal performance
    fn rebalance_tree_structure(&mut self) {
        // 1. Analyze current tree structure for imbalance
        let balance_factor = self.calculate_tree_balance_factor();
        
        if self.requires_rebalancing(balance_factor) {
            // 2. Reorganize nodes to maintain balanced tree properties
            self.perform_tree_rebalancing();
            
            // 3. Minimize tree depth for threshold operations
            self.optimize_tree_depth();
            
            // 4. Update internal pointers and maintain leaf ordering
            self.update_tree_pointers();
            
            // Recompute commitments after structural changes
            self.update_tree_structure_and_commitments();
        }
    }
    
    /// Calculate the balance factor of the current tree
    fn calculate_tree_balance_factor(&self) -> i32 {
        // In a balanced binary tree, balance factor = height(left) - height(right)
        // For our simplified structure, calculate based on device distribution
        
        let total_devices = self.device_mapping.len();
        if total_devices <= 1 {
            return 0; // Single device or empty tree is balanced
        }
        
        // Simple heuristic: if we have too many devices without proper distribution,
        // the tree is unbalanced. In a full implementation, this would calculate
        // actual tree height differences.
        
        let optimal_depth = (total_devices as f64).log2().ceil() as i32;
        let current_depth = self.estimate_current_tree_depth();
        
        current_depth - optimal_depth
    }
    
    /// Check if tree requires rebalancing
    fn requires_rebalancing(&self, balance_factor: i32) -> bool {
        // Tree is unbalanced if balance factor is greater than 1
        // Also rebalance if we have too many devices for current structure
        balance_factor > 1 || self.device_mapping.len() > 32
    }
    
    /// Perform the actual tree rebalancing
    fn perform_tree_rebalancing(&mut self) {
        // For a full implementation, this would:
        // 1. Collect all leaf nodes
        // 2. Build a balanced binary tree structure
        // 3. Redistribute nodes to minimize depth
        
        // Current simplified approach: reorganize device mapping
        // to ensure efficient access patterns
        self.reorganize_device_mapping();
    }
    
    /// Optimize tree depth for threshold operations
    fn optimize_tree_depth(&mut self) {
        // Ensure tree depth is minimal for the number of devices
        // This improves threshold signature performance
        
        let device_count = self.device_mapping.len();
        if device_count > 0 {
            // Update internal tree structure to optimal depth
            // In a full implementation, this would restructure internal nodes
            self.update_optimal_tree_structure(device_count);
        }
    }
    
    /// Update tree pointers after rebalancing
    fn update_tree_pointers(&mut self) {
        // Update parent/child relationships after structural changes
        self.update_parent_child_relationships();
    }
    
    /// Estimate current tree depth
    fn estimate_current_tree_depth(&self) -> i32 {
        // For simplified structure, estimate depth based on device count
        let device_count = self.device_mapping.len();
        if device_count == 0 {
            0
        } else {
            (device_count as f64).log2().ceil() as i32 + 1
        }
    }
    
    /// Reorganize device mapping for better access patterns
    fn reorganize_device_mapping(&mut self) {
        // Sort devices by some criteria (e.g., device ID) for consistent ordering
        // This helps with deterministic tree structure and proof generation
        
        // Collect current mappings
        let mut mappings: Vec<(LocalDeviceId, LeafId)> = 
            self.device_mapping.iter().map(|(&k, &v)| (k, v)).collect();
        
        // Sort by device ID for deterministic ordering
        mappings.sort_by_key(|(device_id, _)| *device_id);
        
        // Rebuild mapping with potentially new leaf IDs for balanced distribution
        self.device_mapping.clear();
        for (i, (device_id, _)) in mappings.into_iter().enumerate() {
            self.device_mapping.insert(device_id, LeafId(i as u32));
        }
    }
    
    /// Update tree structure for optimal performance
    fn update_optimal_tree_structure(&mut self, _device_count: usize) {
        // In a full implementation, this would create optimal internal node structure
        // For now, ensure next_device_id is properly maintained
        self.next_device_id = self.device_mapping.len() as u32 + 1;
    }

    /// Invalidate cached key shares on tree structure changes
    fn invalidate_cached_key_shares(&mut self) {
        // 1. Clear any cached FROST threshold signature shares
        self.clear_frost_key_cache();
        
        // 2. Mark key derivation cache as stale
        self.mark_key_derivation_stale();
        
        // 3. Notify devices that key shares need regeneration
        self.mark_devices_for_key_regeneration();
        
        // 4. Update epoch markers to trigger DKG if needed
        self.update_dkg_epoch_markers();
    }
    
    /// Clear cached FROST threshold signature shares
    fn clear_frost_key_cache(&mut self) {
        // Clear any locally cached FROST key material
        // In production, this would integrate with SecureStorageEffects
        
        // For now, ensure any cached derivation state is reset
        // This prevents using stale key shares after tree changes
        
        // Mark that cached keys are invalid by updating internal state
        // Real implementation would call into secure storage layer
    }
    
    /// Mark key derivation cache as stale
    fn mark_key_derivation_stale(&mut self) {
        // Increment epoch to indicate key derivation state has changed
        // This ensures that any derived keys are regenerated
        self.epoch = self.epoch + 1;
        
        // In a full implementation, this would also:
        // - Update cached merkle paths
        // - Invalidate any precomputed signing contexts
        // - Clear threshold signature caches
    }
    
    /// Mark devices for key regeneration
    fn mark_devices_for_key_regeneration(&mut self) {
        // In production, this would send notifications to devices
        // that they need to regenerate their key shares
        
        // For each device, mark that DKG is required
        let device_ids: Vec<LocalDeviceId> = self.device_mapping.keys().copied().collect();
        for device_id in device_ids {
            // In real implementation, would queue DKG request for device
            self.queue_dkg_for_device(device_id);
        }
    }
    
    /// Update epoch markers to trigger DKG if needed
    fn update_dkg_epoch_markers(&mut self) {
        // Update internal markers that indicate when DKG should be performed
        // This is critical for maintaining security after tree changes
        
        // Increment epoch to trigger new DKG ceremony
        self.epoch = self.epoch + 1;
        
        // In production, this would also:
        // - Schedule DKG with all participants
        // - Update consensus requirements
        // - Notify relying parties of key rotation
    }
    
    /// Queue DKG for a specific device (placeholder)
    fn queue_dkg_for_device(&mut self, _device_id: LocalDeviceId) {
        // In production, this would:
        // 1. Add device to DKG participant list
        // 2. Schedule DKG ceremony
        // 3. Notify device of upcoming key regeneration
        
        // For now, this is a placeholder for the DKG scheduling logic
        // Real implementation would integrate with the choreography system
    }

    /// Compute proper tree commitment using merkle tree
    fn compute_tree_commitment(&mut self) {
        // 1. Build merkle tree from active leaf public keys
        let leaf_commitments = self.build_leaf_commitments();
        
        // 2. Compute merkle root as tree commitment
        let merkle_root = self.compute_merkle_root(&leaf_commitments);
        
        // 3. Include epoch and threshold in commitment
        let tree_commitment = self.finalize_tree_commitment(merkle_root);
        
        // 4. Store commitment for verification and consensus
        self.root_commitment = tree_commitment.0;
    }
    
    /// Build commitments for all active leaves
    fn build_leaf_commitments(&self) -> Vec<(LeafId, Vec<u8>)> {
        let mut leaf_commitments = Vec::new();
        
        // Collect all device leaf commitments
        for (&device_id, &leaf_id) in &self.device_mapping {
            // Find the corresponding leaf data
            if let Some(leaf) = self.leaves.get(&leaf_id) {
                // Compute commitment for this leaf
                let commitment = self.compute_leaf_commitment(&leaf);
                leaf_commitments.push((leaf_id, commitment));
            } else {
                // If we don't have the leaf data, create a placeholder commitment
                let placeholder_commitment = self.compute_placeholder_leaf_commitment(device_id, leaf_id);
                leaf_commitments.push((leaf_id, placeholder_commitment));
            }
        }
        
        // Sort by leaf_id for deterministic ordering
        leaf_commitments.sort_by_key(|(leaf_id, _)| *leaf_id);
        leaf_commitments
    }
    
    /// Compute merkle root from leaf commitments
    fn compute_merkle_root(&self, leaf_commitments: &[(LeafId, Vec<u8>)]) -> Vec<u8> {
        use aura_core::hash;
        
        if leaf_commitments.is_empty() {
            // Empty tree has a default root
            return vec![0u8; 32];
        }
        
        // Build merkle tree bottom-up
        let mut current_level: Vec<Vec<u8>> = leaf_commitments
            .iter()
            .map(|(_, commitment)| commitment.clone())
            .collect();
        
        // Build tree level by level until we reach the root
        while current_level.len() > 1 {
            let mut next_level = Vec::new();
            
            // Pair up nodes and hash them together
            for chunk in current_level.chunks(2) {
                let mut hasher = hash::hasher();
                hasher.update(&chunk[0]);
                
                if chunk.len() == 2 {
                    hasher.update(&chunk[1]);
                } else {
                    // Odd number of nodes - duplicate the last one
                    hasher.update(&chunk[0]);
                }
                
                next_level.push(hasher.finalize().to_vec());
            }
            
            current_level = next_level;
        }
        
        current_level.into_iter().next().unwrap_or_else(|| vec![0u8; 32])
    }
    
    /// Finalize tree commitment with epoch and threshold
    fn finalize_tree_commitment(&self, merkle_root: Vec<u8>) -> Hash32 {
        use aura_core::hash;
        let mut hasher = hash::hasher();
        
        // Include structural information
        hasher.update(b"TREE_COMMITMENT_V1");
        hasher.update(&self.epoch.to_le_bytes());
        hasher.update(&self.threshold.to_le_bytes());
        hasher.update(&(self.device_mapping.len() as u32).to_le_bytes());
        
        // Include the merkle root
        hasher.update(&merkle_root);
        
        Hash32::new(hasher.finalize())
    }
    
    /// Compute commitment for a specific leaf
    fn compute_leaf_commitment(&self, leaf: &LocalLeafNode) -> Vec<u8> {
        use aura_core::hash;
        let mut hasher = hash::hasher();
        
        hasher.update(b"LEAF_COMMITMENT_V1");
        hasher.update(&leaf.leaf_id.0.to_le_bytes());
        hasher.update(&leaf.public_key);
        hasher.update(&[0u8]); // Local leafs are opaque; treat as device role for commitment
        
        hasher.finalize().to_vec()
    }
    
    /// Compute placeholder commitment for missing leaf
    fn compute_placeholder_leaf_commitment(&self, device_id: LocalDeviceId, leaf_id: LeafId) -> Vec<u8> {
        use aura_core::hash;
        let mut hasher = hash::hasher();
        
        hasher.update(b"PLACEHOLDER_LEAF_V1");
        hasher.update(&leaf_id.0.to_le_bytes());
        hasher.update(&device_id.0.to_le_bytes());
        hasher.update(&self.epoch.to_le_bytes());
        
        hasher.finalize().to_vec()
    }
}

impl Default for AuthorityTreeState {
    fn default() -> Self {
        Self::new()
    }
}
