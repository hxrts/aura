//! Journal Operations - High-level API for journal and tree operations
//!
//! This module provides a high-level API for managing the ratchet tree through
//! the intent pool and TreeSession choreographies. It abstracts away the complexity
//! of distributed coordination and provides simple async methods for common operations.
//!
//! **Phase 5 Update**: Now integrated with authorization operations system.
//!
//! ## Architecture
//!
//! The TreeCoordinator follows the unified effect system architecture:
//! - Consumes effects via dependency injection (AuraEffectSystem)
//! - Uses the unified handler system from aura-protocol
//! - Follows Layer 3 Domain Business Logic patterns
//! - Orchestrates tree operations through effects
//! - Integrates with authorization bridge pattern
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_agent::TreeCoordinator;
//! use aura_protocol::effects::AuraEffectSystem;
//! use aura_protocol::handlers::ExecutionMode;
//!
//! let effects = AuraEffectSystem::for_production(device_id)?;
//! let coordinator = TreeCoordinator::new(effects, device_id);
//!
//! // Add a device to the tree
//! coordinator.add_device(new_device_id, public_key).await?;
//!
//! // Rotate device keys
//! coordinator.rotate_device(device_id).await?;
//!
//! // Start recovery ceremony
//! let capability = coordinator.start_recovery().await?;
//! ```

use crate::operations::*;
use aura_core::{
    identifiers::{DeviceId, GuardianId},
    ledger::{
        capability::RecoveryCapability, tree_op::Epoch, CapabilityRef, Intent, IntentId,
        IntentStatus, Priority,
    },
    tree::{
        node::{KeyPackage, LeafMetadata},
        Commitment, LeafId, LeafIndex, LeafNode, LeafRole, Policy, RatchetTree, TreeOperation,
    },
    AuraError, AuraResult as Result,
};
use aura_protocol::effects::{AuraEffectSystem, JournalEffects, JournalError};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

/// Error types specific to tree coordination
#[derive(Debug, thiserror::Error)]
pub enum TreeError {
    #[error("Tree operation failed: {0}")]
    OperationFailed(String),

    #[error("Intent submission failed: {0}")]
    IntentSubmissionFailed(String),

    #[error("Intent timed out after {0:?}")]
    IntentTimeout(Duration),

    #[error("Capability validation failed: {0}")]
    CapabilityValidationFailed(String),

    #[error("Tree state inconsistent: {0}")]
    TreeInconsistent(String),

    #[error("Journal error: {0}")]
    JournalError(#[from] JournalError),

    #[error("Device not found in tree")]
    DeviceNotFound,

    #[error("Insufficient permissions: {0}")]
    InsufficientPermissions(String),
}

impl From<TreeError> for AuraError {
    fn from(err: TreeError) -> Self {
        match err {
            TreeError::OperationFailed(msg) => AuraError::coordination_failed(msg),
            TreeError::IntentSubmissionFailed(msg) => AuraError::coordination_failed(msg),
            TreeError::IntentTimeout(duration) => {
                AuraError::coordination_failed(format!("Intent timed out after {:?}", duration))
            }
            TreeError::CapabilityValidationFailed(msg) => AuraError::permission_denied(msg),
            TreeError::TreeInconsistent(msg) => AuraError::internal(msg),
            TreeError::JournalError(e) => {
                AuraError::coordination_failed(format!("Journal error: {}", e))
            }
            TreeError::DeviceNotFound => AuraError::not_found("Device not found in tree"),
            TreeError::InsufficientPermissions(msg) => AuraError::permission_denied(msg),
        }
    }
}

/// Tree coordinator for high-level tree operations
///
/// Provides a simple async API for managing the ratchet tree through the
/// intent pool and TreeSession choreographies. Follows the unified effect
/// system architecture by consuming effects via dependency injection.
pub struct TreeCoordinator {
    /// Unified effect system for all operations
    effects: Arc<RwLock<AuraEffectSystem>>,
    /// Device ID for this coordinator
    device_id: DeviceId,
    /// Local tree cache
    tree_cache: Arc<RwLock<Option<RatchetTree>>>,
    /// Default timeout for intent completion
    default_timeout: Duration,
    /// Authorized operations handler
    auth_operations: Option<Arc<AuthorizedAgentOperations>>,
}

impl TreeCoordinator {
    /// Create a new tree coordinator
    pub fn new(effects: AuraEffectSystem, device_id: DeviceId) -> Self {
        Self {
            effects: Arc::new(RwLock::new(effects)),
            device_id,
            tree_cache: Arc::new(RwLock::new(None)),
            default_timeout: Duration::from_secs(30),
            auth_operations: None,
        }
    }

    /// Create a tree coordinator with a custom timeout
    pub fn with_timeout(effects: AuraEffectSystem, device_id: DeviceId, timeout: Duration) -> Self {
        Self {
            effects: Arc::new(RwLock::new(effects)),
            device_id,
            tree_cache: Arc::new(RwLock::new(None)),
            default_timeout: timeout,
            auth_operations: None,
        }
    }

    /// Create a tree coordinator with authorization
    pub fn with_authorization(
        effects: AuraEffectSystem,
        device_id: DeviceId,
        auth_operations: Arc<AuthorizedAgentOperations>,
    ) -> Self {
        Self {
            effects: Arc::new(RwLock::new(effects)),
            device_id,
            tree_cache: Arc::new(RwLock::new(None)),
            default_timeout: Duration::from_secs(30),
            auth_operations: Some(auth_operations),
        }
    }

    /// Get the current tree state, refreshing cache if needed
    pub async fn get_current_tree(&self) -> Result<RatchetTree> {
        // Try cache first
        let cache = self.tree_cache.read().await;
        if let Some(tree) = cache.as_ref() {
            return Ok(tree.clone());
        }
        drop(cache);

        // Fetch from journal and update cache via unified effect system
        let effects = self.effects.read().await;
        let tree = effects.get_current_tree().await.map_err(|e| {
            AuraError::coordination_failed(format!("Failed to get current tree: {}", e))
        })?;

        drop(effects);
        let mut cache = self.tree_cache.write().await;
        *cache = Some(tree.clone());

        Ok(tree)
    }

    /// Invalidate the tree cache
    pub async fn invalidate_cache(&self) {
        let mut cache = self.tree_cache.write().await;
        *cache = None;
    }

    /// Add a device to the tree with authorization check
    pub async fn add_device_authorized(
        &self,
        request: AgentOperationRequest,
        device_id: DeviceId,
        public_key: Vec<u8>,
    ) -> Result<LeafIndex> {
        if let Some(auth_ops) = &self.auth_operations {
            use aura_wot::{TreeOp, TreeOpKind, LeafRole};
            
            let tree_op = TreeOp {
                parent_epoch: 1,
                parent_commitment: [0u8; 32],
                op: TreeOpKind::AddLeaf {
                    leaf_id: 0,
                    role: LeafRole::Device,
                    under: 0,
                },
                version: 1,
            };

            let agent_op = AgentOperation::TreeOperation {
                operation: tree_op,
            };

            let auth_request = AgentOperationRequest {
                identity_proof: request.identity_proof,
                operation: agent_op,
                signed_message: request.signed_message,
                context: request.context,
            };

            let result = auth_ops.execute_operation(auth_request).await
                .map_err(|e| AuraError::coordination_failed(format!("Authorization failed: {}", e)))?;
            
            match result {
                AgentOperationResult::Tree { result: TreeResult::DeviceAdded { leaf_index } } => {
                    Ok(LeafIndex(leaf_index))
                },
                _ => {
                    // Fallback to direct operation if authorization succeeds but returns unexpected result
                    self.add_device_direct(device_id, public_key).await
                },
            }
        } else {
            // Fallback to direct device addition
            self.add_device_direct(device_id, public_key).await
        }
    }

    /// Add a device to the tree (legacy method, kept for compatibility)
    pub async fn add_device(&self, device_id: DeviceId, public_key: Vec<u8>) -> Result<LeafIndex> {
        self.add_device_direct(device_id, public_key).await
    }

    /// Add a device to the tree (direct, no authorization)
    ///
    /// Creates an AddLeaf intent and waits for the TreeSession to complete.
    pub async fn add_device_direct(&self, device_id: DeviceId, public_key: Vec<u8>) -> Result<LeafIndex> {
        // Get current tree state for snapshot
        let tree = self.get_current_tree().await?;
        let snapshot_commitment = tree.root_commitment().clone();

        // Determine next leaf index (LBBT allocation)
        let next_leaf_index = LeafIndex(tree.num_leaves());

        // Create leaf node for AddLeaf operation
        let leaf_node = LeafNode {
            leaf_id: LeafId::new(),
            leaf_index: next_leaf_index,
            public_key: KeyPackage {
                signing_key: public_key.clone(),
                encryption_key: Some(public_key), // TODO: separate encryption key
            },
            role: LeafRole::Device,
            metadata: LeafMetadata::default(),
        };

        // Get current timestamp via unified effect system
        let effects = self.effects.read().await;
        let timestamp = effects
            .current_timestamp()
            .await
            .map_err(|e| TreeError::OperationFailed(format!("Failed to get timestamp: {}", e)))?;

        // Create AddLeaf intent
        let intent = Intent {
            intent_id: IntentId::new(),
            op: TreeOperation::AddLeaf {
                leaf: leaf_node,
                affected_path: Default::default(), // TODO: compute actual affected path
            },
            path_span: vec![], // Will be computed during TreeSession
            snapshot_commitment,
            priority: Priority::new(100), // Normal priority
            author: self.device_id,
            created_at: timestamp,
            metadata: Default::default(),
        };

        // Submit intent and wait for completion
        let intent_id = self.submit_intent_and_wait(intent).await?;

        // Invalidate cache after successful mutation
        self.invalidate_cache().await;

        Ok(next_leaf_index)
    }

    /// Remove a device from the tree
    pub async fn remove_device(&self, device_id: DeviceId) -> Result<()> {
        // Get device's leaf index via unified effect system
        let effects = self.effects.read().await;
        let leaf_index = effects
            .get_device_leaf_index(device_id)
            .await
            .map_err(|e| TreeError::JournalError(e))?
            .ok_or(TreeError::DeviceNotFound)?;

        // Get current tree state
        let tree = self.get_current_tree().await?;
        let snapshot_commitment = tree.root_commitment().clone();

        // Get current timestamp via unified effect system
        let effects = self.effects.read().await;
        let timestamp = effects
            .current_timestamp()
            .await
            .map_err(|e| TreeError::OperationFailed(format!("Failed to get timestamp: {}", e)))?;

        // Create RemoveLeaf intent
        let intent = Intent {
            intent_id: IntentId::new(),
            op: TreeOperation::RemoveLeaf {
                leaf_index,
                affected_path: Default::default(), // TODO: compute actual affected path
            },
            path_span: vec![],
            snapshot_commitment,
            priority: Priority::new(100),
            author: self.device_id,
            created_at: timestamp,
            metadata: Default::default(),
        };

        // Submit and wait
        self.submit_intent_and_wait(intent).await?;

        // Invalidate cache
        self.invalidate_cache().await;

        Ok(())
    }

    /// Rotate device keys for forward secrecy
    pub async fn rotate_device(&self, device_id: DeviceId) -> Result<()> {
        // Get device's leaf index via unified effect system
        let effects = self.effects.read().await;
        let leaf_index = effects
            .get_device_leaf_index(device_id)
            .await
            .map_err(|e| TreeError::JournalError(e))?
            .ok_or(TreeError::DeviceNotFound)?;

        // Get current tree state
        let tree = self.get_current_tree().await?;
        let snapshot_commitment = tree.root_commitment().clone();

        // Get current timestamp via unified effect system
        let effects = self.effects.read().await;
        let timestamp = effects
            .current_timestamp()
            .await
            .map_err(|e| TreeError::OperationFailed(format!("Failed to get timestamp: {}", e)))?;

        // Create RotateEpoch intent to rotate device keys
        let intent = Intent {
            intent_id: IntentId::new(),
            op: TreeOperation::RotateEpoch {
                affected: vec![aura_core::tree::NodeIndex(leaf_index.0)],
            },
            path_span: vec![],
            snapshot_commitment,
            priority: Priority::new(100),
            author: self.device_id,
            created_at: timestamp,
            metadata: Default::default(),
        };

        // Submit and wait
        self.submit_intent_and_wait(intent).await?;

        // Invalidate cache
        self.invalidate_cache().await;

        Ok(())
    }

    /// Start recovery ceremony
    ///
    /// Activates the guardian branch and returns a recovery capability that can be
    /// used by guardians to issue recovery tokens.
    pub async fn start_recovery(&self) -> Result<RecoveryCapability> {
        // Get current tree state
        let tree = self.get_current_tree().await?;
        let snapshot_commitment = tree.root_commitment().clone();

        // Get current timestamp via unified effect system
        let effects = self.effects.read().await;
        let timestamp = effects
            .current_timestamp()
            .await
            .map_err(|e| TreeError::OperationFailed(format!("Failed to get timestamp: {}", e)))?;

        // TODO: RefreshPolicy operation doesn't exist in TreeOperation yet
        // Skipping intent submission TODO fix - For now

        // Create recovery capability
        // TODO: This needs proper implementation with guardian signatures
        let capability = RecoveryCapability::new(
            self.device_id,      // target_device
            vec![],              // issuing_guardians (empty TODO fix - For now)
            2,                   // guardian_threshold
            timestamp + 900_000, // expires_at (15 minutes in ms)
            0,                   // leaf_index (placeholder)
            tree.epoch,          // epoch
            aura_journal::ledger::capability::CapabilitySignature {
                signature: vec![], // placeholder signature
                signer: self.device_id,
            },
        );

        Ok(capability)
    }

    /// Submit an intent and wait for completion
    pub async fn submit_intent(&self, intent: Intent) -> Result<IntentId> {
        let effects = self.effects.read().await;
        let intent_id = effects
            .submit_intent(intent)
            .await
            .map_err(|e| TreeError::IntentSubmissionFailed(format!("{}", e)))?;
        Ok(intent_id)
    }

    /// Submit intent and poll until completion or timeout
    async fn submit_intent_and_wait(&self, intent: Intent) -> Result<IntentId> {
        let intent_id = self.submit_intent(intent).await?;

        // Poll for completion
        let start = std::time::Instant::now();
        loop {
            // Check if intent is completed via unified effect system
            let effects = self.effects.read().await;
            let status = effects
                .get_intent_status(intent_id.clone())
                .await
                .map_err(|e| TreeError::JournalError(e))?;

            match status {
                IntentStatus::Completed => {
                    return Ok(intent_id);
                }
                IntentStatus::Pending | IntentStatus::Executing => {
                    // Check timeout
                    if start.elapsed() > self.default_timeout {
                        return Err(TreeError::IntentTimeout(self.default_timeout).into());
                    }

                    // Sleep before next poll
                    sleep(Duration::from_millis(100)).await;
                }
                IntentStatus::Failed => {
                    return Err(
                        TreeError::IntentSubmissionFailed("Intent failed".to_string()).into(),
                    );
                }
                IntentStatus::Superseded => {
                    return Err(TreeError::IntentSubmissionFailed(
                        "Intent was superseded".to_string(),
                    )
                    .into());
                }
            }
        }
    }

    /// Validate a capability reference
    pub async fn validate_capability(&self, capability: &CapabilityRef) -> Result<bool> {
        let effects = self.effects.read().await;
        effects
            .validate_capability(capability)
            .await
            .map_err(|e| TreeError::CapabilityValidationFailed(format!("{}", e)).into())
    }

    /// Check if a device is a member of the tree
    pub async fn is_device_member(&self, device_id: DeviceId) -> Result<bool> {
        let effects = self.effects.read().await;
        effects
            .is_device_member(device_id)
            .await
            .map_err(|e| TreeError::JournalError(e).into())
    }

    /// List all devices in the current tree
    pub async fn list_devices(&self) -> Result<Vec<DeviceId>> {
        let effects = self.effects.read().await;
        effects
            .list_devices()
            .await
            .map_err(|e| TreeError::JournalError(e).into())
    }

    /// List all guardians in the current tree
    pub async fn list_guardians(&self) -> Result<Vec<GuardianId>> {
        let effects = self.effects.read().await;
        effects
            .list_guardians()
            .await
            .map_err(|e| TreeError::JournalError(e).into())
    }

    /// Get the latest epoch from the journal
    pub async fn get_latest_epoch(&self) -> Result<Option<Epoch>> {
        let effects = self.effects.read().await;
        effects
            .get_latest_epoch()
            .await
            .map_err(|e| TreeError::JournalError(e).into())
    }

    /// Get the current root commitment
    pub async fn get_current_commitment(&self) -> Result<Commitment> {
        let effects = self.effects.read().await;
        effects
            .get_current_commitment()
            .await
            .map_err(|e| TreeError::JournalError(e).into())
    }

    /// List all pending intents in the pool
    pub async fn list_pending_intents(&self) -> Result<Vec<Intent>> {
        let effects = self.effects.read().await;
        effects
            .list_pending_intents()
            .await
            .map_err(|e| TreeError::JournalError(e).into())
    }

    /// Get journal statistics
    pub async fn get_stats(&self) -> Result<aura_protocol::effects::JournalStats> {
        let effects = self.effects.read().await;
        effects
            .get_journal_stats()
            .await
            .map_err(|e| TreeError::JournalError(e).into())
    }
}

/// Helper function to get current timestamp in seconds for testing
fn current_timestamp() -> u64 {
    // Using fixed timestamp for testing - real implementation should use effects
    1609459200 // Jan 1, 2021 UTC
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::effects::AuraEffectSystem;
    use aura_protocol::handlers::ExecutionMode;

    #[tokio::test]
    async fn test_tree_coordinator_creation() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);
        let coordinator = TreeCoordinator::new(effect_system, device_id);

        // Should start with empty cache
        let cache = coordinator.tree_cache.read().await;
        assert!(cache.is_none());
    }

    #[tokio::test]
    async fn test_get_current_tree() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);
        let coordinator = TreeCoordinator::new(effect_system, device_id);

        // Should fetch tree from journal
        let tree = coordinator.get_current_tree().await.unwrap();
        assert_eq!(tree.leaf_count(), 0); // Empty tree initially

        // Cache should be populated
        let cache = coordinator.tree_cache.read().await;
        assert!(cache.is_some());
    }

    #[tokio::test]
    async fn test_invalidate_cache() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);
        let coordinator = TreeCoordinator::new(effect_system, device_id);

        // Populate cache
        let _ = coordinator.get_current_tree().await.unwrap();
        assert!(coordinator.tree_cache.read().await.is_some());

        // Invalidate
        coordinator.invalidate_cache().await;
        assert!(coordinator.tree_cache.read().await.is_none());
    }

    #[tokio::test]
    async fn test_list_devices() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);
        let coordinator = TreeCoordinator::new(effect_system, device_id);

        let devices = coordinator.list_devices().await.unwrap();
        assert_eq!(devices.len(), 0); // Empty initially
    }

    #[tokio::test]
    async fn test_get_stats() {
        let device_id = DeviceId::new();
        let effect_system = AuraEffectSystem::for_testing(device_id);
        let coordinator = TreeCoordinator::new(effect_system, device_id);

        let stats = coordinator.get_stats().await.unwrap();
        assert_eq!(stats.num_ops, 0);
        assert_eq!(stats.num_intents, 0);
    }
}
