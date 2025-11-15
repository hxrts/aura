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

use crate::CapabilitySet;
use aura_core::{LeafId, LeafNode, NodeIndex, Policy as TreePolicy};

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

// Middleware implementation removed - migrated to AuthorizationEffects pattern
// TODO: Complete migration by implementing TreeAuthorizationHandler in aura-effects

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
            leaf: LeafNode::new_device(LeafId(1), aura_core::DeviceId::new(), vec![0u8; 32]),
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
            leaf: LeafNode::new_device(LeafId(1), aura_core::DeviceId::new(), vec![0u8; 32]),
            under: NodeIndex(0),
        };
        let caps = op.required_capabilities();
        assert!(caps.permits("tree:add_leaf"));
        assert!(caps.permits("tree:propose"));
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

    // TODO: Implement tests for new AuthorizationEffects-based tree authorization
    // These tests should use dependency injection with AuthorizationEffects trait
}
