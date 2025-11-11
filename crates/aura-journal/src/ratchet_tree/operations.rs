//! Tree Operation Processing and Validation
//!
//! This module provides high-level operations for processing tree operations,
//! including validation, application, and state synchronization.

use super::{
    application::{apply_verified, validate_invariants, ApplicationError},
    reduction::{reduce, ReductionError},
    TreeState,
};
use aura_core::{AttestedOp, Hash32, LeafId, NodeIndex, TreeOp};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// Comprehensive operation processor for tree operations
#[derive(Debug, Clone)]
pub struct TreeOperationProcessor {
    /// Current tree state (derived from OpLog)
    current_state: TreeState,
    /// Set of processed operation hashes (for deduplication)
    processed_ops: BTreeSet<Hash32>,
    /// Operation history for debugging and auditing
    operation_history: Vec<ProcessedOperation>,
}

/// Record of a processed operation with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessedOperation {
    /// Hash of the operation for deduplication
    pub operation_hash: Hash32,
    /// Epoch when the operation was processed
    pub processed_at_epoch: u64,
    /// Whether the operation was successfully applied
    pub success: bool,
    /// Error message if operation failed
    pub error: Option<String>,
    /// Nodes affected by this operation
    pub affected_nodes: Vec<NodeIndex>,
}

/// Errors that can occur during operation processing
#[derive(Debug, Error)]
pub enum OperationProcessorError {
    /// Operation has already been processed
    #[error("Operation already processed: {0:?}")]
    AlreadyProcessed(Hash32),

    /// Application error occurred
    #[error("Application error: {0}")]
    ApplicationError(#[from] ApplicationError),

    /// Reduction error occurred
    #[error("Reduction error: {0}")]
    ReductionError(#[from] ReductionError),

    /// State synchronization failed
    #[error("State synchronization failed: {reason}")]
    SyncFailed {
        /// The reason for sync failure
        reason: String,
    },

    /// Invalid operation sequence
    #[error("Invalid operation sequence")]
    InvalidSequence,
}

impl TreeOperationProcessor {
    /// Create a new operation processor with empty state
    pub fn new() -> Self {
        Self {
            current_state: TreeState::new(),
            processed_ops: BTreeSet::new(),
            operation_history: Vec::new(),
        }
    }

    /// Create processor from existing state
    pub fn from_state(state: TreeState) -> Self {
        Self {
            current_state: state,
            processed_ops: BTreeSet::new(),
            operation_history: Vec::new(),
        }
    }

    /// Get the current tree state (read-only view)
    pub fn current_state(&self) -> &TreeState {
        &self.current_state
    }

    /// Get mutable access to current state (for testing)
    pub fn current_state_mut(&mut self) -> &mut TreeState {
        &mut self.current_state
    }

    /// Get operation processing history
    pub fn operation_history(&self) -> &[ProcessedOperation] {
        &self.operation_history
    }

    /// Check if an operation has already been processed
    pub fn is_processed(&self, operation_hash: &Hash32) -> bool {
        self.processed_ops.contains(operation_hash)
    }

    /// Process a single attested operation
    ///
    /// This is the main entry point for processing operations. It handles:
    /// - Deduplication (skips already processed operations)
    /// - Validation and application
    /// - State updates
    /// - History tracking
    pub fn process_operation(
        &mut self,
        attested: &AttestedOp,
    ) -> Result<ProcessedOperation, OperationProcessorError> {
        let operation_hash = self.compute_operation_hash(attested);

        // Check for duplicates
        if self.is_processed(&operation_hash) {
            return Err(OperationProcessorError::AlreadyProcessed(operation_hash));
        }

        let initial_epoch = self.current_state.current_epoch();
        let mut success = true;
        let mut error_msg = None;
        let mut affected_nodes = Vec::new();

        // Attempt to apply the operation
        match apply_verified(&mut self.current_state, attested) {
            Ok(()) => {
                // Extract affected nodes based on operation type
                affected_nodes = self.extract_affected_nodes(&attested.op);

                tracing::info!(
                    "Successfully applied operation {:?} at epoch {}",
                    hex::encode(&operation_hash.0[..8]),
                    self.current_state.current_epoch()
                );
            }
            Err(e) => {
                success = false;
                error_msg = Some(e.to_string());

                tracing::warn!(
                    "Failed to apply operation {:?}: {}",
                    hex::encode(&operation_hash.0[..8]),
                    e
                );
            }
        }

        // Create processing record
        let processed = ProcessedOperation {
            operation_hash,
            processed_at_epoch: initial_epoch,
            success,
            error: error_msg,
            affected_nodes,
        };

        // Update tracking state
        if success {
            self.processed_ops.insert(operation_hash);
        }
        self.operation_history.push(processed.clone());

        Ok(processed)
    }

    /// Process multiple operations in sequence
    ///
    /// Processes operations one by one, stopping on first failure if `fail_fast` is true.
    /// Returns a vector of results for each operation.
    pub fn process_operations(
        &mut self,
        operations: &[AttestedOp],
        fail_fast: bool,
    ) -> Vec<Result<ProcessedOperation, OperationProcessorError>> {
        let mut results = Vec::new();

        for op in operations {
            let result = self.process_operation(op);
            let should_continue = result.is_ok();

            results.push(result);

            if !should_continue && fail_fast {
                break;
            }
        }

        results
    }

    /// Synchronize state from a complete OpLog
    ///
    /// This rebuilds the tree state from scratch using the reduction algorithm.
    /// Used for:
    /// - Initial state loading from persistent storage
    /// - Recovery after state corruption
    /// - Synchronization with remote peers
    pub fn sync_from_oplog(&mut self, ops: &[AttestedOp]) -> Result<(), OperationProcessorError> {
        // Reduce the complete OpLog to get the correct state
        let new_state = reduce(ops)?;

        // Update our state
        self.current_state = new_state;

        // Clear and rebuild processed operations set
        self.processed_ops.clear();
        for op in ops {
            let op_hash = self.compute_operation_hash(op);
            self.processed_ops.insert(op_hash);
        }

        // Clear operation history (it's no longer accurate after sync)
        self.operation_history.clear();

        tracing::info!(
            "Synchronized state from {} operations, epoch {}",
            ops.len(),
            self.current_state.current_epoch()
        );

        Ok(())
    }

    /// Validate current state integrity
    ///
    /// Performs comprehensive validation of the current tree state.
    /// Should be called periodically to detect corruption.
    pub fn validate_state(&self) -> Result<(), OperationProcessorError> {
        validate_invariants(&self.current_state).map_err(OperationProcessorError::ApplicationError)
    }

    /// Get processing statistics
    pub fn get_stats(&self) -> ProcessingStats {
        let successful = self
            .operation_history
            .iter()
            .filter(|op| op.success)
            .count();
        let failed = self.operation_history.len() - successful;

        ProcessingStats {
            current_epoch: self.current_state.current_epoch(),
            total_operations: self.operation_history.len(),
            successful_operations: successful,
            failed_operations: failed,
            unique_processed: self.processed_ops.len(),
            num_leaves: self.current_state.num_leaves(),
            num_branches: self.current_state.num_branches(),
        }
    }

    /// Reset processor to initial state
    pub fn reset(&mut self) {
        self.current_state = TreeState::new();
        self.processed_ops.clear();
        self.operation_history.clear();
    }

    /// Extract affected nodes from an operation
    fn extract_affected_nodes(&self, op: &TreeOp) -> Vec<NodeIndex> {
        let mut affected = Vec::new();

        match &op.op {
            aura_core::TreeOpKind::AddLeaf { under, .. } => {
                affected.push(*under);
            }
            aura_core::TreeOpKind::RemoveLeaf { leaf, .. } => {
                // Find parent of the leaf being removed
                if let Some(parent) = self.current_state.get_leaf_parent(*leaf) {
                    affected.push(parent);
                }
            }
            aura_core::TreeOpKind::ChangePolicy { node, .. } => {
                affected.push(*node);
            }
            aura_core::TreeOpKind::RotateEpoch { affected: nodes } => {
                affected.extend(nodes.iter());
            }
        }

        affected
    }

    /// Compute hash for operation deduplication
    fn compute_operation_hash(&self, attested: &AttestedOp) -> Hash32 {
        use blake3::Hasher;

        let mut hasher = Hasher::new();

        // Hash operation content
        hasher.update(&attested.op.parent_epoch.to_le_bytes());
        hasher.update(&attested.op.parent_commitment);
        hasher.update(&attested.op.version.to_le_bytes());

        // Hash operation type and data
        match &attested.op.op {
            aura_core::TreeOpKind::AddLeaf { leaf, under } => {
                hasher.update(b"AddLeaf");
                hasher.update(&leaf.leaf_id.0.to_le_bytes());
                hasher.update(&under.0.to_le_bytes());
                hasher.update(&leaf.public_key);
            }
            aura_core::TreeOpKind::RemoveLeaf { leaf, reason } => {
                hasher.update(b"RemoveLeaf");
                hasher.update(&leaf.0.to_le_bytes());
                hasher.update(&[*reason]);
            }
            aura_core::TreeOpKind::ChangePolicy { node, new_policy } => {
                hasher.update(b"ChangePolicy");
                hasher.update(&node.0.to_le_bytes());
                hasher.update(&aura_core::policy_hash(new_policy));
            }
            aura_core::TreeOpKind::RotateEpoch { affected } => {
                hasher.update(b"RotateEpoch");
                for node in affected {
                    hasher.update(&node.0.to_le_bytes());
                }
            }
        }

        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(hash.as_bytes());
        Hash32(result)
    }
}

impl Default for TreeOperationProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about operation processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingStats {
    /// Current epoch of the tree
    pub current_epoch: u64,
    /// Total number of operations processed
    pub total_operations: usize,
    /// Number of successfully applied operations
    pub successful_operations: usize,
    /// Number of failed operations
    pub failed_operations: usize,
    /// Number of unique operations in processed set
    pub unique_processed: usize,
    /// Current number of leaves in tree
    pub num_leaves: usize,
    /// Current number of branches in tree
    pub num_branches: usize,
}

/// Batch processor for handling multiple operations efficiently
#[derive(Debug)]
pub struct BatchProcessor {
    /// The operation processor
    processor: TreeOperationProcessor,
    /// Batch size for processing
    batch_size: usize,
    /// Whether to validate after each batch
    validate_batches: bool,
}

impl BatchProcessor {
    /// Create a new batch processor
    pub fn new(batch_size: usize, validate_batches: bool) -> Self {
        Self {
            processor: TreeOperationProcessor::new(),
            batch_size,
            validate_batches,
        }
    }

    /// Process operations in batches
    pub fn process_batched(
        &mut self,
        operations: &[AttestedOp],
    ) -> Result<Vec<ProcessedOperation>, OperationProcessorError> {
        let mut results = Vec::new();

        for chunk in operations.chunks(self.batch_size) {
            let batch_results = self.processor.process_operations(chunk, false);

            // Extract successful operations
            for result in batch_results {
                match result {
                    Ok(processed) => results.push(processed),
                    Err(e) => return Err(e),
                }
            }

            // Validate state after each batch if requested
            if self.validate_batches {
                self.processor.validate_state()?;
            }

            tracing::debug!("Processed batch of {} operations", chunk.len());
        }

        Ok(results)
    }

    /// Get the underlying processor
    pub fn processor(&self) -> &TreeOperationProcessor {
        &self.processor
    }

    /// Get mutable access to the underlying processor
    pub fn processor_mut(&mut self) -> &mut TreeOperationProcessor {
        &mut self.processor
    }
}

/// Query interface for tree state
pub struct TreeStateQuery<'a> {
    state: &'a TreeState,
}

impl<'a> TreeStateQuery<'a> {
    /// Create a new query interface
    pub fn new(state: &'a TreeState) -> Self {
        Self { state }
    }

    /// Get all leaves in the tree
    pub fn all_leaves(&self) -> BTreeMap<LeafId, &'a aura_core::LeafNode> {
        self.state
            .leaves
            .iter()
            .map(|(id, node)| (*id, node))
            .collect()
    }

    /// Get all branches in the tree
    pub fn all_branches(&self) -> BTreeMap<NodeIndex, &'a aura_core::BranchNode> {
        self.state
            .branches
            .iter()
            .map(|(idx, node)| (*idx, node))
            .collect()
    }

    /// Find leaves by role
    pub fn leaves_by_role(
        &self,
        role: aura_core::LeafRole,
    ) -> Vec<(LeafId, &'a aura_core::LeafNode)> {
        self.state
            .leaves
            .iter()
            .filter(|(_, leaf)| leaf.role == role)
            .map(|(id, leaf)| (*id, leaf))
            .collect()
    }

    /// Find branches by policy type
    pub fn branches_by_policy(
        &self,
        policy: &aura_core::Policy,
    ) -> Vec<(NodeIndex, &'a aura_core::BranchNode)> {
        self.state
            .branches
            .iter()
            .filter(|(_, branch)| &branch.policy == policy)
            .map(|(idx, branch)| (*idx, branch))
            .collect()
    }

    /// Get path from leaf to root
    pub fn leaf_to_root_path(&self, leaf_id: LeafId) -> Vec<NodeIndex> {
        self.state.get_leaf_path_to_root(leaf_id)
    }

    /// Get all children of a node
    pub fn node_children(&self, node: NodeIndex) -> BTreeSet<NodeIndex> {
        self.state.get_children(node)
    }

    /// Check if tree is valid (basic integrity)
    pub fn is_valid(&self) -> bool {
        validate_invariants(self.state).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{LeafId, LeafNode, LeafRole, TreeOp, TreeOpKind};

    fn create_test_leaf(id: u32) -> LeafNode {
        LeafNode::new_device(LeafId(id), aura_core::DeviceId::new(), vec![id as u8; 32])
    }

    async fn create_test_operation(leaf_id: u32, parent_epoch: u64) -> AttestedOp {
        let tree_op = TreeOp {
            parent_epoch,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf: create_test_leaf(leaf_id),
                under: NodeIndex(0),
            },
            version: 1,
        };
        
        // Create test attested op with dummy signature
        AttestedOp {
            op: tree_op,
            agg_sig: vec![1u8; 64], // Test signature
            signer_count: 1,
        }
    }

    #[test]
    fn test_operation_processor_creation() {
        let processor = TreeOperationProcessor::new();
        assert_eq!(processor.current_state().num_leaves(), 0);
        assert_eq!(processor.operation_history().len(), 0);
    }

    #[tokio::test]
    async fn test_process_single_operation() {
        let mut processor = TreeOperationProcessor::new();
        let op = create_test_operation(1, 0).await;

        let result = processor.process_operation(&op);
        assert!(result.is_ok());

        let processed = result.unwrap();
        assert!(processed.success);
        assert!(processed.error.is_none());
        assert_eq!(processor.current_state().num_leaves(), 1);
    }

    #[tokio::test]
    async fn test_duplicate_operation_rejection() {
        let mut processor = TreeOperationProcessor::new();
        let op = create_test_operation(1, 0).await;

        // Process first time - should succeed
        let result1 = processor.process_operation(&op);
        assert!(result1.is_ok());

        // Process second time - should fail
        let result2 = processor.process_operation(&op);
        assert!(result2.is_err());
        assert!(matches!(
            result2.unwrap_err(),
            OperationProcessorError::AlreadyProcessed(_)
        ));
    }

    #[tokio::test]
    async fn test_batch_processing() {
        let mut batch_processor = BatchProcessor::new(2, true);

        let ops = vec![
            create_test_operation(1, 0).await,
            create_test_operation(2, 0).await,
            create_test_operation(3, 0).await,
        ];

        let results = batch_processor.process_batched(&ops);
        assert!(results.is_ok());

        let processed = results.unwrap();
        assert_eq!(processed.len(), 3);
        assert!(processed.iter().all(|p| p.success));
    }

    #[tokio::test]
    async fn test_sync_from_oplog() {
        let mut processor = TreeOperationProcessor::new();

        let ops = vec![
            create_test_operation(1, 0).await,
            create_test_operation(2, 0).await,
            create_test_operation(3, 0).await,
        ];

        let result = processor.sync_from_oplog(&ops);
        assert!(result.is_ok());
        assert_eq!(processor.current_state().num_leaves(), 1);
    }

    #[tokio::test]
    async fn test_query_interface() {
        let mut processor = TreeOperationProcessor::new();
        let op = create_test_operation(1, 0).await;

        processor.process_operation(&op).unwrap();

        let query = TreeStateQuery::new(processor.current_state());
        let leaves = query.all_leaves();
        assert_eq!(leaves.len(), 1);

        let device_leaves = query.leaves_by_role(LeafRole::Device);
        assert_eq!(device_leaves.len(), 1);
    }

    #[tokio::test]
    async fn test_processing_stats() {
        let mut processor = TreeOperationProcessor::new();

        let op1 = create_test_operation(1, 0).await;
        let op2 = create_test_operation(2, 0).await;

        processor.process_operation(&op1).unwrap();
        processor.process_operation(&op2).unwrap();

        let stats = processor.get_stats();
        assert_eq!(stats.successful_operations, 2);
        assert_eq!(stats.failed_operations, 0);
        assert_eq!(stats.num_leaves, 2);
    }
}
