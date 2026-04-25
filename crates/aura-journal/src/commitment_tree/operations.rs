//! Tree Operation Processing and Validation
//!
//! This module provides high-level operations for processing tree operations,
//! including validation, application, and state synchronization.

use super::{
    application::{apply_verified, validate_invariants, ApplicationError},
    reduction::{reduce, ReductionError},
    TreeState,
};
use aura_core::effects::CryptoEffects;
use aura_core::{AttestedOp, Epoch, Hash32, LeafId, NodeIndex, TreeOp};
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
    pub processed_at_epoch: Epoch,
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
    pub async fn process_operation(
        &mut self,
        attested: &AttestedOp,
        crypto_effects: &dyn CryptoEffects,
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

        // Attempt to apply the operation with full FROST verification.
        match apply_verified(&mut self.current_state, attested, crypto_effects).await {
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
    pub async fn process_operations(
        &mut self,
        operations: &[AttestedOp],
        fail_fast: bool,
        crypto_effects: &dyn CryptoEffects,
    ) -> Vec<Result<ProcessedOperation, OperationProcessorError>> {
        let mut results = Vec::new();

        for op in operations {
            let result = self.process_operation(op, crypto_effects).await;
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
    pub async fn sync_from_oplog(
        &mut self,
        ops: &[AttestedOp],
        crypto_effects: &dyn CryptoEffects,
    ) -> Result<(), OperationProcessorError> {
        let mut verified_processor = TreeOperationProcessor::from_state(self.current_state.clone());
        for op in ops {
            verified_processor
                .process_operation(op, crypto_effects)
                .await?;
        }

        let reduced_state = reduce(ops)?;
        self.current_state = reduced_state;
        self.processed_ops = verified_processor.processed_ops;
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
            current_epoch: self.current_state.current_epoch().into(),
            total_operations: self.operation_history.len() as u64,
            successful_operations: successful as u64,
            failed_operations: failed as u64,
            unique_processed: self.processed_ops.len() as u64,
            num_leaves: self.current_state.num_leaves() as u32,
            num_branches: self.current_state.num_branches() as u32,
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
        let mut hasher = aura_core::hash::hasher();

        // Hash operation content
        hasher.update(&u64::from(attested.op.parent_epoch).to_le_bytes());
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

        Hash32(hasher.finalize())
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
    pub total_operations: u64,
    /// Number of successfully applied operations
    pub successful_operations: u64,
    /// Number of failed operations
    pub failed_operations: u64,
    /// Number of unique operations in processed set
    pub unique_processed: u64,
    /// Current number of leaves in tree
    pub num_leaves: u32,
    /// Current number of branches in tree
    pub num_branches: u32,
}

/// Batch processor for handling multiple operations efficiently
#[derive(Debug)]
pub struct BatchProcessor {
    /// The operation processor
    processor: TreeOperationProcessor,
    /// Batch size for processing
    batch_size: u32,
    /// Whether to validate after each batch
    validate_batches: bool,
}

impl BatchProcessor {
    /// Create a new batch processor
    pub fn new(batch_size: u32, validate_batches: bool) -> Self {
        Self {
            processor: TreeOperationProcessor::new(),
            batch_size,
            validate_batches,
        }
    }

    /// Process operations in batches
    pub async fn process_batched(
        &mut self,
        operations: &[AttestedOp],
        crypto_effects: &dyn CryptoEffects,
    ) -> Result<Vec<ProcessedOperation>, OperationProcessorError> {
        let mut results = Vec::new();

        for chunk in operations.chunks(self.batch_size as usize) {
            let batch_results = self
                .processor
                .process_operations(chunk, false, crypto_effects)
                .await;

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
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use aura_core::effects::{
        CryptoCoreEffects, CryptoError, CryptoExtendedEffects, RandomCoreEffects,
    };
    use aura_core::tree::BranchSigningKey;
    use aura_core::{BranchNode, LeafId, LeafNode, LeafRole, Policy, TreeOp, TreeOpKind};

    struct MockCrypto {
        frost_valid: bool,
    }

    #[async_trait]
    impl RandomCoreEffects for MockCrypto {
        async fn random_bytes(&self, len: usize) -> Vec<u8> {
            vec![0; len]
        }

        async fn random_bytes_32(&self) -> [u8; 32] {
            [0u8; 32]
        }

        async fn random_u64(&self) -> u64 {
            0
        }
    }

    #[async_trait]
    impl CryptoCoreEffects for MockCrypto {
        async fn kdf_derive(
            &self,
            _: &[u8],
            _: &[u8],
            _: &[u8],
            output_len: u32,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![0; output_len as usize])
        }

        async fn derive_key(
            &self,
            _: &[u8],
            _: &aura_core::effects::crypto::KeyDerivationContext,
        ) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![])
        }

        async fn ed25519_generate_keypair(&self) -> Result<(Vec<u8>, Vec<u8>), CryptoError> {
            Ok((vec![1u8; 32], vec![2u8; 32]))
        }

        async fn ed25519_sign(&self, _: &[u8], _: &[u8]) -> Result<Vec<u8>, CryptoError> {
            Ok(vec![1u8; 64])
        }

        async fn ed25519_verify(&self, _: &[u8], _: &[u8], _: &[u8]) -> Result<bool, CryptoError> {
            Ok(true)
        }

        fn is_simulated(&self) -> bool {
            true
        }

        fn crypto_capabilities(&self) -> Vec<String> {
            vec!["mock".to_string()]
        }

        fn constant_time_eq(&self, a: &[u8], b: &[u8]) -> bool {
            a == b
        }

        fn secure_zero(&self, data: &mut [u8]) {
            data.fill(0);
        }
    }

    #[async_trait]
    impl CryptoExtendedEffects for MockCrypto {
        async fn frost_verify(&self, _: &[u8], _: &[u8], _: &[u8]) -> Result<bool, CryptoError> {
            Ok(self.frost_valid)
        }
    }

    fn create_test_leaf(id: u32) -> LeafNode {
        LeafNode::new_device(
            LeafId(id),
            aura_core::DeviceId(uuid::Uuid::from_bytes([9u8; 16])),
            vec![id as u8; 32],
        )
        .expect("valid leaf")
    }

    fn test_processor() -> TreeOperationProcessor {
        let mut state = TreeState::new();
        state.add_branch(BranchNode {
            node: NodeIndex(0),
            policy: Policy::Any,
            commitment: [0u8; 32],
        });
        state.add_branch_with_parent(
            BranchNode {
                node: NodeIndex(1),
                policy: Policy::Any,
                commitment: [1u8; 32],
            },
            Some(NodeIndex(0)),
        );
        state.set_signing_key(
            NodeIndex(0),
            BranchSigningKey::new([3u8; 32], Epoch::initial()),
        );
        TreeOperationProcessor::from_state(state)
    }

    fn create_test_operation(leaf_id: u32, parent_epoch: Epoch) -> AttestedOp {
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
        let mut processor = test_processor();
        let op = create_test_operation(1, Epoch::initial());
        let crypto = MockCrypto { frost_valid: true };

        let result = processor.process_operation(&op, &crypto).await;
        assert!(result.is_ok());

        let processed = result.unwrap();
        assert!(processed.success);
        assert!(processed.error.is_none());
        assert_eq!(processor.current_state().num_leaves(), 1);
    }

    #[tokio::test]
    async fn test_duplicate_operation_rejection() {
        let mut processor = test_processor();
        let op = create_test_operation(1, Epoch::initial());
        let crypto = MockCrypto { frost_valid: true };

        // Process first time - should succeed
        let result1 = processor.process_operation(&op, &crypto).await;
        assert!(result1.is_ok());

        // Process second time - should fail
        let result2 = processor.process_operation(&op, &crypto).await;
        assert!(result2.is_err());
        assert!(matches!(
            result2.unwrap_err(),
            OperationProcessorError::AlreadyProcessed(_)
        ));
    }

    #[tokio::test]
    async fn test_batch_processing() {
        let mut batch_processor = BatchProcessor {
            processor: test_processor(),
            batch_size: 2,
            validate_batches: true,
        };
        let crypto = MockCrypto { frost_valid: true };

        let ops = vec![
            create_test_operation(1, Epoch::initial()),
            create_test_operation(2, Epoch::initial()),
            create_test_operation(3, Epoch::initial()),
        ];

        let results = batch_processor.process_batched(&ops, &crypto).await;
        assert!(results.is_ok());

        let processed = results.unwrap();
        assert_eq!(processed.len(), 3);
        assert!(processed.iter().all(|p| p.success));
    }

    #[tokio::test]
    async fn test_sync_from_oplog() {
        let mut processor = test_processor();
        let crypto = MockCrypto { frost_valid: true };

        let ops = vec![
            create_test_operation(1, Epoch::initial()),
            create_test_operation(2, Epoch::initial()),
            create_test_operation(3, Epoch::initial()),
        ];

        let result = processor.sync_from_oplog(&ops, &crypto).await;
        assert!(result.is_ok());
        assert_eq!(processor.current_state().num_leaves(), 1);
    }

    #[tokio::test]
    async fn test_query_interface() {
        let mut processor = test_processor();
        let op = create_test_operation(1, Epoch::initial());
        let crypto = MockCrypto { frost_valid: true };

        processor.process_operation(&op, &crypto).await.unwrap();

        let query = TreeStateQuery::new(processor.current_state());
        let leaves = query.all_leaves();
        assert_eq!(leaves.len(), 1);

        let device_leaves = query.leaves_by_role(LeafRole::Device);
        assert_eq!(device_leaves.len(), 1);
    }

    #[tokio::test]
    async fn test_processing_stats() {
        let mut processor = test_processor();
        let crypto = MockCrypto { frost_valid: true };

        let op1 = create_test_operation(1, Epoch::initial());
        let op2 = create_test_operation(2, Epoch::initial());

        processor.process_operation(&op1, &crypto).await.unwrap();
        processor.process_operation(&op2, &crypto).await.unwrap();

        let stats = processor.get_stats();
        assert_eq!(stats.successful_operations, 2);
        assert_eq!(stats.failed_operations, 0);
        assert_eq!(stats.num_leaves, 2);
    }

    #[tokio::test]
    async fn test_process_operation_rejects_forged_signature() {
        let mut processor = test_processor();
        let op = create_test_operation(1, Epoch::initial());
        let crypto = MockCrypto { frost_valid: false };

        let processed = processor
            .process_operation(&op, &crypto)
            .await
            .expect("verification failure is recorded as a processed result");

        assert!(!processed.success);
        assert!(processed
            .error
            .as_deref()
            .is_some_and(|error| error.contains("FROST signature verification failed")));
        assert_eq!(processor.current_state().num_leaves(), 0);
    }
}
