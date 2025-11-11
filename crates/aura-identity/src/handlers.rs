//! Application-level Tree Operation Handlers
//!
//! This module provides concrete implementations of tree operation handlers
//! that can be used by applications, replacing the old hand-coded handlers
//! with choreography-based implementations.

use crate::verification::{DeviceInfo, DeviceStatus, IdentityResult, IdentityVerifier};
use async_trait::async_trait;
use aura_core::{
    tree::{AttestedOp, LeafId, LeafNode, NodeIndex, TreeOp, TreeOpKind},
    AccountId, AuraError, AuraResult, DeviceId, Policy,
};
use aura_mpst::AuraRuntime;
use std::collections::HashMap;

/// Application-level tree handler that uses choreographies
#[derive(Debug)]
pub struct ChoreographicTreeHandler {
    /// Tree operation coordinator
    // coordinator: TreeOperationCoordinator,  // TODO: Implement this type
    /// Identity verifier
    verifier: IdentityVerifier,
    /// Local device ID
    device_id: DeviceId,
    /// Handler configuration
    config: TreeHandlerConfig,
}

/// Configuration for the tree handler
#[derive(Debug, Clone)]
pub struct TreeHandlerConfig {
    /// Default threshold for tree operations
    pub default_threshold: usize,
    /// Maximum participants allowed
    pub max_participants: usize,
    /// Enable strict verification
    pub strict_verification: bool,
}

impl Default for TreeHandlerConfig {
    fn default() -> Self {
        Self {
            default_threshold: 2,
            max_participants: 10,
            strict_verification: true,
        }
    }
}

/// Tree operation context
#[derive(Debug, Clone)]
pub struct TreeOperationContext {
    /// Operation requester
    pub requester: DeviceId,
    /// Target account
    pub account_id: AccountId,
    /// Required participants
    pub participants: Vec<DeviceId>,
    /// Operation metadata
    pub metadata: HashMap<String, String>,
}

impl ChoreographicTreeHandler {
    /// Create a new choreographic tree handler
    pub fn new(_runtime: AuraRuntime, device_id: DeviceId, config: TreeHandlerConfig) -> Self {
        Self {
            // coordinator: TreeOperationCoordinator::new(_runtime),  // TODO: Implement this
            verifier: IdentityVerifier::new(),
            device_id,
            config,
        }
    }

    /// Register a device with the handler
    pub fn register_device(&mut self, device_info: DeviceInfo) -> IdentityResult<()> {
        self.verifier.register_device(device_info)
    }

    /// Execute a tree operation using choreographies
    pub async fn execute_tree_operation(
        &mut self,
        operation: TreeOp,
        context: TreeOperationContext,
    ) -> IdentityResult<AttestedOp> {
        // Verify the operation
        if self.config.strict_verification {
            let verification = self
                .verifier
                .verify_tree_operation(&operation, context.requester)?;
            if !verification.verified {
                return Err(AuraError::invalid(verification.details));
            }
        }

        // Check participant count
        if context.participants.len() > self.config.max_participants {
            return Err(AuraError::invalid(
                "Too many participants for tree operation",
            ));
        }

        // Create tree operation request (TODO: Define TreeOpRequest type)
        let request = format!(
            "TreeOpRequest {{ operation: {:?}, participants: {:?}, threshold: {} }}",
            operation, context.participants, self.config.default_threshold
        );

        // Execute via choreography (TODO: Implement TreeOperationCoordinator)
        tracing::info!(
            "Would execute tree operation via choreography: {:?}",
            request
        );

        // TODO fix - For now, return a placeholder attested operation
        Ok(AttestedOp {
            op: operation,
            agg_sig: vec![0u8; 64],
            signer_count: self.config.default_threshold as u16,
        })
    }

    /// Add a leaf node to the tree
    pub async fn add_leaf(
        &mut self,
        leaf: LeafNode,
        under: NodeIndex,
        context: TreeOperationContext,
    ) -> IdentityResult<AttestedOp> {
        let operation = TreeOp {
            parent_epoch: 0,              // TODO: Get current epoch
            parent_commitment: [0u8; 32], // TODO: Get current commitment
            op: TreeOpKind::AddLeaf { leaf, under },
            version: 1, // Current protocol version
        };
        self.execute_tree_operation(operation, context).await
    }

    /// Remove a leaf node from the tree
    pub async fn remove_leaf(
        &mut self,
        leaf: LeafId,
        reason: u8,
        context: TreeOperationContext,
    ) -> IdentityResult<AttestedOp> {
        let operation = TreeOp {
            parent_epoch: 0,              // TODO: Get current epoch
            parent_commitment: [0u8; 32], // TODO: Get current commitment
            op: TreeOpKind::RemoveLeaf { leaf, reason },
            version: 1, // Current protocol version
        };
        self.execute_tree_operation(operation, context).await
    }

    /// Change the threshold policy of a branch node
    pub async fn change_policy(
        &mut self,
        node: NodeIndex,
        new_policy: Policy,
        context: TreeOperationContext,
    ) -> IdentityResult<AttestedOp> {
        let operation = TreeOp {
            parent_epoch: 0,              // TODO: Get current epoch
            parent_commitment: [0u8; 32], // TODO: Get current commitment
            op: TreeOpKind::ChangePolicy { node, new_policy },
            version: 1, // Current protocol version
        };
        self.execute_tree_operation(operation, context).await
    }

    /// Get the identity verifier
    pub fn verifier(&self) -> &IdentityVerifier {
        &self.verifier
    }

    /// Get mutable identity verifier
    pub fn verifier_mut(&mut self) -> &mut IdentityVerifier {
        &mut self.verifier
    }

    // TODO: Implement TreeOperationCoordinator type and these methods
    // /// Get the tree operation coordinator
    // pub fn coordinator(&self) -> &TreeOperationCoordinator {
    //     &self.coordinator
    // }

    // /// Get mutable tree operation coordinator
    // pub fn coordinator_mut(&mut self) -> &mut TreeOperationCoordinator {
    //     &mut self.coordinator
    // }

    /// Get handler configuration
    pub fn config(&self) -> &TreeHandlerConfig {
        &self.config
    }

    /// Update handler configuration
    pub fn update_config(&mut self, config: TreeHandlerConfig) {
        self.config = config;
    }
}

/// Legacy tree handler trait for compatibility
///
/// This trait defines the interface that existing applications expect
/// from tree handlers. The ChoreographicTreeHandler implements this
/// to provide a drop-in replacement for old hand-coded handlers.
#[async_trait]
pub trait LegacyTreeHandler {
    /// Handle a tree operation
    async fn handle_operation(&mut self, operation: TreeOp) -> IdentityResult<AttestedOp>;

    /// Validate a tree operation
    fn validate_operation(&self, operation: &TreeOp) -> IdentityResult<()>;
}

#[async_trait]
impl LegacyTreeHandler for ChoreographicTreeHandler {
    async fn handle_operation(&mut self, operation: TreeOp) -> IdentityResult<AttestedOp> {
        // Create a basic context for legacy compatibility
        let context = TreeOperationContext {
            requester: self.device_id,
            account_id: AccountId::new(), // Placeholder
            participants: vec![self.device_id],
            metadata: HashMap::new(),
        };

        self.execute_tree_operation(operation, context).await
    }

    fn validate_operation(&self, operation: &TreeOp) -> IdentityResult<()> {
        let verification = self
            .verifier
            .verify_tree_operation(operation, self.device_id)?;
        if verification.verified {
            Ok(())
        } else {
            Err(AuraError::invalid(verification.details))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::{tree::LeafNode, Cap, DeviceId, Journal};

    fn create_test_handler() -> ChoreographicTreeHandler {
        let device_id = DeviceId::new();
        let runtime = AuraRuntime::new(device_id, Cap::top(), Journal::new());
        let config = TreeHandlerConfig::default();

        ChoreographicTreeHandler::new(runtime, device_id, config)
    }

    #[tokio::test]
    async fn test_handler_creation() {
        let handler = create_test_handler();
        assert_eq!(handler.config.default_threshold, 2);
        assert_eq!(handler.config.max_participants, 10);
        assert!(handler.config.strict_verification);
    }

    #[tokio::test]
    async fn test_device_registration() {
        let mut handler = create_test_handler();
        let device_id = DeviceId::new();

        let device_info = DeviceInfo {
            device_id,
            public_key: vec![1, 2, 3, 4],
            capabilities: Cap::top(),
            status: DeviceStatus::Active,
        };

        assert!(handler.register_device(device_info).is_ok());
        assert!(handler.verifier().known_devices().contains_key(&device_id));
    }

    #[test]
    fn test_config_update() {
        let mut handler = create_test_handler();

        let new_config = TreeHandlerConfig {
            default_threshold: 3,
            max_participants: 5,
            strict_verification: false,
        };

        handler.update_config(new_config);
        assert_eq!(handler.config().default_threshold, 3);
        assert_eq!(handler.config().max_participants, 5);
        assert!(!handler.config().strict_verification);
    }
}
