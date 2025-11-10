//! Tree Authorization using Capability-Based Access Control
//!
//! This module provides authorization for tree operations using proper
//! capability objects and meet-semilattice intersection. Tree operations
//! require threshold signatures, so authorization checks happen BEFORE
//! signing ceremonies, not during.
//!
//! ## Design Principles
//!
//! - **Meet-Guarded Preconditions**: Check `need(op) ≤ effective_caps` before proposing
//! - **Capability Monotonicity**: Authority only shrinks via meet operation
//! - **Pre-Ceremony Authorization**: Authorization happens before signing, not during
//! - **No Author Identity**: Operations authorized by capabilities, not device identity
//!
//! ## Usage
//!
//! ```rust,ignore
//! use aura_wot::{TreeAuthorizationMiddleware, TreeOperation};
//! use aura_core::{LeafNode, NodeIndex, Policy};
//!
//! let mut authz = TreeAuthorizationMiddleware::new();
//! authz.grant_tree_capabilities(device_id, &["tree:add_leaf", "tree:change_policy"]);
//!
//! // Check before creating proposal
//! let op = TreeOperation::AddLeaf { leaf, under };
//! if authz.authorize_operation(device_id, &op)? {
//!     // Proceed with threshold signing ceremony
//! }
//! ```

use crate::{
    evaluate_capabilities, CapabilitySet, EvaluationContext, LocalChecks, Policy, PolicyEngine,
    WotError,
};
use aura_core::{identifiers::DeviceId, LeafId, LeafNode, NodeIndex, Policy as TreePolicy};
use std::collections::HashMap;

/// Tree-specific operations that can be authorized
///
/// Maps 1:1 to `TreeOpKind` from aura-core but lives in authz layer
/// to keep capability logic separate from core types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeOperation {
    /// Add a new leaf to the tree under a specific parent node
    AddLeaf { leaf: LeafNode, under: NodeIndex },

    /// Remove a leaf from the tree with a reason code
    RemoveLeaf { leaf_id: LeafId, reason: u8 },

    /// Change the policy at a specific node (must be stricter or equal)
    ChangePolicy {
        node: NodeIndex,
        new_policy: TreePolicy,
    },

    /// Rotate epoch for affected nodes (invalidates old shares)
    RotateEpoch { affected: Vec<NodeIndex> },
}

impl TreeOperation {
    /// Convert tree operation to capability operation string
    ///
    /// Used for matching against granted capabilities during authorization.
    pub fn to_operation_string(&self) -> String {
        match self {
            TreeOperation::AddLeaf { .. } => "tree:add_leaf".to_string(),
            TreeOperation::RemoveLeaf { .. } => "tree:remove_leaf".to_string(),
            TreeOperation::ChangePolicy { .. } => "tree:change_policy".to_string(),
            TreeOperation::RotateEpoch { .. } => "tree:rotate_epoch".to_string(),
        }
    }

    /// Get the capability required to propose this operation
    ///
    /// Returns a CapabilitySet representing `need(op)` from the spec.
    /// Authorization succeeds if `need(op) ≤ effective_caps`.
    pub fn required_capabilities(&self) -> CapabilitySet {
        match self {
            TreeOperation::AddLeaf { .. } => {
                CapabilitySet::from_permissions(&["tree:add_leaf", "tree:propose"])
            }
            TreeOperation::RemoveLeaf { .. } => {
                CapabilitySet::from_permissions(&["tree:remove_leaf", "tree:propose"])
            }
            TreeOperation::ChangePolicy { .. } => {
                CapabilitySet::from_permissions(&["tree:change_policy", "tree:propose"])
            }
            TreeOperation::RotateEpoch { .. } => {
                CapabilitySet::from_permissions(&["tree:rotate_epoch", "tree:propose"])
            }
        }
    }
}

/// Capability-based tree authorization middleware
///
/// Enforces capability checks before tree operations are proposed for
/// threshold signing. Uses meet-semilattice intersection to compute
/// effective capabilities from policy, delegations, and local checks.
#[derive(Debug)]
pub struct TreeAuthorizationMiddleware {
    policy_engine: PolicyEngine,
    local_checks: LocalChecks,
}

impl TreeAuthorizationMiddleware {
    /// Create new tree authorization middleware with default policy
    pub fn new() -> Self {
        Self {
            policy_engine: PolicyEngine::new(),
            local_checks: LocalChecks::empty(),
        }
    }

    /// Create with custom policy
    pub fn with_policy(policy: Policy) -> Self {
        Self {
            policy_engine: PolicyEngine::with_policy(policy),
            local_checks: LocalChecks::empty(),
        }
    }

    /// Grant tree capabilities to a device
    ///
    /// Valid tree capabilities:
    /// - `tree:add_leaf` - Propose adding new leaves
    /// - `tree:remove_leaf` - Propose removing leaves
    /// - `tree:change_policy` - Propose policy changes
    /// - `tree:rotate_epoch` - Propose epoch rotation
    /// - `tree:propose` - General proposal capability (required for all ops)
    /// - `tree:sign` - Participate in threshold signing (separate from proposal)
    pub fn grant_tree_capabilities(&mut self, device_id: DeviceId, operations: &[&str]) {
        let tree_caps = CapabilitySet::from_permissions(operations);
        self.policy_engine.grant_capabilities(device_id, tree_caps);
    }

    /// Check if a device can propose a tree operation
    ///
    /// This implements the meet-guarded precondition:
    /// `authorize ⟺ need(op) ≤ effective_caps`
    ///
    /// Where `effective_caps = policy ⊓ delegations ⊓ local_checks`
    ///
    /// **CRITICAL**: This check happens BEFORE the signing ceremony, not during.
    /// The ceremony itself verifies threshold, not individual authorization.
    pub fn authorize_operation(
        &self,
        device_id: DeviceId,
        operation: &TreeOperation,
        metadata: &HashMap<String, String>,
    ) -> Result<bool, WotError> {
        // Get required capabilities for this operation
        let required = operation.required_capabilities();

        // Convert to operation string for context
        let operation_string = operation.to_operation_string();

        // Create evaluation context
        let mut context = EvaluationContext::new(device_id, operation_string);
        for (key, value) in metadata {
            context = context.with_metadata(key.clone(), value.clone());
        }

        // Evaluate effective capabilities (policy ⊓ delegations ⊓ local_checks)
        let effective_caps = evaluate_capabilities(
            self.policy_engine.active_policy(),
            &[], // No delegation chains for tree operations (direct policy only)
            &self.local_checks,
            &context,
        )?;

        // Check if effective capabilities include all required capabilities
        // This implements: need(op) ≤ effective_caps
        Ok(required.is_subset_of(&effective_caps))
    }

    /// Add local checks (time restrictions, rate limits, etc.)
    ///
    /// Local checks are intersected with policy via meet operation,
    /// ensuring capabilities can only shrink.
    pub fn with_local_checks(mut self, local_checks: LocalChecks) -> Self {
        self.local_checks = local_checks;
        self
    }

    /// Get current policy engine (for inspection/debugging)
    pub fn policy_engine(&self) -> &PolicyEngine {
        &self.policy_engine
    }

    /// Check if device can participate in threshold signing
    ///
    /// This is separate from proposal authorization. A device may be able
    /// to sign operations without being able to propose them.
    pub fn authorize_signing(&self, device_id: DeviceId) -> Result<bool, WotError> {
        let context = EvaluationContext::new(device_id, "tree:sign".to_string());

        let effective_caps = evaluate_capabilities(
            self.policy_engine.active_policy(),
            &[],
            &self.local_checks,
            &context,
        )?;

        Ok(effective_caps.permits("tree:sign"))
    }
}

impl Default for TreeAuthorizationMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to create a CapabilitySet for common tree operations
///
/// Returns capabilities needed for standard tree management operations.
pub fn tree_management_capabilities() -> CapabilitySet {
    CapabilitySet::from_permissions(&[
        "tree:add_leaf",
        "tree:remove_leaf",
        "tree:change_policy",
        "tree:rotate_epoch",
        "tree:propose",
        "tree:sign",
    ])
}

/// Helper function to create a CapabilitySet for read-only tree access
///
/// Returns capabilities for inspecting tree state without modification.
pub fn tree_read_capabilities() -> CapabilitySet {
    CapabilitySet::from_permissions(&["tree:read"])
}

/// Helper function to create a CapabilitySet for device lifecycle operations
///
/// Returns capabilities for adding/removing devices (leaves) only.
pub fn tree_device_lifecycle_capabilities() -> CapabilitySet {
    CapabilitySet::from_permissions(&["tree:add_leaf", "tree:remove_leaf", "tree:propose"])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_operation_to_string() {
        let op = TreeOperation::AddLeaf {
            leaf: LeafNode::new_device(LeafId(1), vec![0u8; 32]),
            under: NodeIndex(0),
        };
        assert_eq!(op.to_operation_string(), "tree:add_leaf");

        let op = TreeOperation::RemoveLeaf {
            leaf_id: LeafId(1),
            reason: 0,
        };
        assert_eq!(op.to_operation_string(), "tree:remove_leaf");
    }

    #[test]
    fn test_required_capabilities() {
        let op = TreeOperation::AddLeaf {
            leaf: LeafNode::new_device(LeafId(1), vec![0u8; 32]),
            under: NodeIndex(0),
        };
        let caps = op.required_capabilities();
        assert!(caps.permits("tree:add_leaf"));
        assert!(caps.permits("tree:propose"));
    }

    #[test]
    fn test_authorization_with_sufficient_capabilities() {
        let device_id = DeviceId::new();
        let mut authz = TreeAuthorizationMiddleware::new();

        // Grant capabilities
        authz.grant_tree_capabilities(device_id, &["tree:add_leaf", "tree:propose"]);

        // Should authorize
        let op = TreeOperation::AddLeaf {
            leaf: LeafNode::new_device(LeafId(1), vec![0u8; 32]),
            under: NodeIndex(0),
        };
        let result = authz.authorize_operation(device_id, &op, &HashMap::new());
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_authorization_with_insufficient_capabilities() {
        let device_id = DeviceId::new();
        let mut authz = TreeAuthorizationMiddleware::new();

        // Grant only read capability
        authz.grant_tree_capabilities(device_id, &["tree:read"]);

        // Should not authorize write operation
        let op = TreeOperation::AddLeaf {
            leaf: LeafNode::new_device(LeafId(1), vec![0u8; 32]),
            under: NodeIndex(0),
        };
        let result = authz.authorize_operation(device_id, &op, &HashMap::new());
        assert!(result.is_ok());
        assert!(!result.unwrap()); // Should be false (not authorized)
    }

    #[test]
    fn test_signing_authorization() {
        let device_id = DeviceId::new();
        let mut authz = TreeAuthorizationMiddleware::new();

        // Grant signing capability
        authz.grant_tree_capabilities(device_id, &["tree:sign"]);

        let result = authz.authorize_signing(device_id);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_helper_capabilities() {
        let mgmt_caps = tree_management_capabilities();
        assert!(mgmt_caps.permits("tree:add_leaf"));
        assert!(mgmt_caps.permits("tree:sign"));

        let read_caps = tree_read_capabilities();
        assert!(read_caps.permits("tree:read"));
        assert!(!read_caps.permits("tree:add_leaf"));

        let lifecycle_caps = tree_device_lifecycle_capabilities();
        assert!(lifecycle_caps.permits("tree:add_leaf"));
        assert!(lifecycle_caps.permits("tree:remove_leaf"));
        assert!(!lifecycle_caps.permits("tree:change_policy"));
    }
}
