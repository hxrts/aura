//! Tree operation authorization evaluation
//!
//! This module provides authorization evaluation for ratchet tree operations,
//! integrating tree policies with capability checks.

use crate::tree_policy::{Policy, ThresholdConfig};
use crate::{CapabilitySet, TreePolicy, WotError};
use aura_core::{AccountId, DeviceId, GuardianId};
use std::collections::{BTreeMap, BTreeSet};

/// Types of tree operations from ratchet tree spec
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TreeOpKind {
    /// Add a new leaf (device or guardian) under a branch
    AddLeaf {
        leaf_id: u32,
        role: LeafRole,
        under: u32, // NodeIndex
    },
    /// Remove a leaf from the tree
    RemoveLeaf { leaf_id: u32, reason: u8 },
    /// Change policy at a node (must be more restrictive or equal)
    ChangePolicy {
        node: u32, // NodeIndex
        new_policy: Policy,
    },
    /// Rotate epoch for affected nodes
    RotateEpoch {
        affected: Vec<u32>, // NodeIndex list
    },
}

/// Leaf role in the tree
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LeafRole {
    Device,
    Guardian,
}

/// Tree operation with metadata
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TreeOp {
    /// Parent epoch this operation builds on
    pub parent_epoch: u64,
    /// Parent tree commitment
    pub parent_commitment: [u8; 32],
    /// The operation to perform
    pub op: TreeOpKind,
    /// Version for compatibility
    pub version: u16,
}

/// Authorization context for tree operations
#[derive(Debug, Clone)]
pub struct TreeAuthzContext {
    /// Current tree policies by node
    pub policies: BTreeMap<u32, TreePolicy>, // NodeIndex -> TreePolicy
    /// Account this tree belongs to
    pub account_id: AccountId,
    /// Current epoch
    pub current_epoch: u64,
}

impl TreeAuthzContext {
    /// Create new tree authorization context
    pub fn new(account_id: AccountId, current_epoch: u64) -> Self {
        Self {
            policies: BTreeMap::new(),
            account_id,
            current_epoch,
        }
    }

    /// Add a tree policy for a node
    pub fn add_policy(&mut self, node: u32, policy: TreePolicy) {
        self.policies.insert(node, policy);
    }

    /// Get policy for a node
    pub fn get_policy(&self, node: u32) -> Option<&TreePolicy> {
        self.policies.get(&node)
    }

    /// Get effective policy for operation (may inherit from parent)
    pub fn effective_policy(&self, target_node: u32) -> Option<&TreePolicy> {
        // TODO fix - For now, return exact match. In full implementation,
        // would walk up tree to find inherited policy
        self.get_policy(target_node)
    }
}

/// Authorization request for tree operations
#[derive(Debug, Clone)]
pub struct TreeAuthzRequest {
    /// The operation to authorize
    pub operation: TreeOp,
    /// Base capabilities of the requesting entity
    pub base_capabilities: CapabilitySet,
    /// Number of signers participating
    pub signer_count: u16,
    /// Specific signers (for audit/verification)
    pub signers: BTreeSet<DeviceId>,
    /// Guardian signers (for recovery operations)
    pub guardian_signers: BTreeSet<GuardianId>,
}

/// Result of tree authorization evaluation
#[derive(Debug, Clone)]
pub struct TreeAuthzResult {
    /// Whether the operation is authorized
    pub authorized: bool,
    /// Effective capabilities after policy intersection
    pub effective_capabilities: CapabilitySet,
    /// Policy that was evaluated
    pub evaluated_policy: Option<TreePolicy>,
    /// Reason if authorization failed
    pub failure_reason: Option<String>,
}

impl TreeAuthzResult {
    /// Create successful authorization result
    pub fn authorized(effective_capabilities: CapabilitySet, evaluated_policy: TreePolicy) -> Self {
        Self {
            authorized: true,
            effective_capabilities,
            evaluated_policy: Some(evaluated_policy),
            failure_reason: None,
        }
    }

    /// Create failed authorization result
    pub fn denied(reason: String) -> Self {
        Self {
            authorized: false,
            effective_capabilities: CapabilitySet::empty(),
            evaluated_policy: None,
            failure_reason: Some(reason),
        }
    }
}

/// Evaluate authorization for a tree operation
pub fn evaluate_tree_operation(
    request: TreeAuthzRequest,
    context: &TreeAuthzContext,
) -> Result<TreeAuthzResult, WotError> {
    // Determine target node for authorization
    let target_node = match &request.operation.op {
        TreeOpKind::AddLeaf { under, .. } => *under,
        TreeOpKind::RemoveLeaf { leaf_id, .. } => {
            // For leaf removal, need to find which node contains this leaf
            // TODO fix - For now, assume leaf_id maps to node (TODO fix - Simplified)
            *leaf_id
        }
        TreeOpKind::ChangePolicy { node, .. } => *node,
        TreeOpKind::RotateEpoch { affected } => {
            // For epoch rotation, check root or first affected node
            affected.first().copied().unwrap_or(0)
        }
    };

    // Get effective policy for the target node
    let policy = match context.effective_policy(target_node) {
        Some(p) => p,
        None => {
            return Ok(TreeAuthzResult::denied(format!(
                "No policy found for node {}",
                target_node
            )));
        }
    };

    // Check basic capability requirements
    let effective_caps = policy.effective_capabilities(&request.base_capabilities);

    // Check if base capabilities include required permissions
    if !effective_caps.permits("tree:propose") {
        return Ok(TreeAuthzResult::denied(
            "Missing tree:propose capability".to_string(),
        ));
    }

    // Check threshold requirements based on operation type
    let threshold_met = match &request.operation.op {
        TreeOpKind::AddLeaf { .. } => {
            // Adding leaves typically requires threshold agreement
            if !effective_caps.permits("tree:modify") {
                return Ok(TreeAuthzResult::denied(
                    "Missing tree:modify capability for AddLeaf".to_string(),
                ));
            }
            policy.evaluate_signers(request.signer_count)
        }

        TreeOpKind::RemoveLeaf { .. } => {
            // Removing leaves requires higher threshold
            if !effective_caps.permits("tree:modify") {
                return Ok(TreeAuthzResult::denied(
                    "Missing tree:modify capability for RemoveLeaf".to_string(),
                ));
            }
            policy.evaluate_signers(request.signer_count)
        }

        TreeOpKind::ChangePolicy { new_policy, .. } => {
            // Policy changes require special permission and must be more restrictive
            if !effective_caps.permits("tree:admin") {
                return Ok(TreeAuthzResult::denied(
                    "Missing tree:admin capability for ChangePolicy".to_string(),
                ));
            }

            // Verify new policy is more restrictive or equal
            if !new_policy.is_more_restrictive_than(&policy.policy) && new_policy != &policy.policy
            {
                return Ok(TreeAuthzResult::denied(
                    "New policy must be more restrictive than current".to_string(),
                ));
            }

            policy.evaluate_signers(request.signer_count)
        }

        TreeOpKind::RotateEpoch { .. } => {
            // Epoch rotation requires threshold but lower privilege
            policy.evaluate_signers(request.signer_count)
        }
    };

    if !threshold_met {
        return Ok(TreeAuthzResult::denied(format!(
            "Threshold not met: {} signers required, {} provided",
            policy.threshold_config.threshold, request.signer_count
        )));
    }

    // All checks passed
    Ok(TreeAuthzResult::authorized(effective_caps, policy.clone()))
}

/// Evaluate capability requirements for tree operations
pub fn tree_operation_capabilities(op: &TreeOpKind) -> CapabilitySet {
    match op {
        TreeOpKind::AddLeaf { .. } => {
            CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:modify"])
        }
        TreeOpKind::RemoveLeaf { .. } => {
            CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:modify"])
        }
        TreeOpKind::ChangePolicy { .. } => {
            CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:admin"])
        }
        TreeOpKind::RotateEpoch { .. } => {
            CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:sign"])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeIndex, Policy};

    #[test]
    fn test_tree_operation_authorization() {
        let account_id = AccountId::from_bytes([1u8; 32]);
        let mut context = TreeAuthzContext::new(account_id, 1);

        // Create a threshold policy for node 0
        let participants = BTreeSet::from([
            DeviceId::from_bytes([1u8; 32]),
            DeviceId::from_bytes([2u8; 32]),
            DeviceId::from_bytes([3u8; 32]),
        ]);

        let threshold_config = ThresholdConfig::new(2, participants.clone());
        let policy = TreePolicy::new(
            NodeIndex(0),
            account_id,
            Policy::Threshold { m: 2, n: 3 },
            threshold_config,
        );

        context.add_policy(0, policy);

        // Create an AddLeaf operation
        let operation = TreeOp {
            parent_epoch: 1,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf_id: 4,
                role: LeafRole::Device,
                under: 0,
            },
            version: 1,
        };

        // Request with sufficient capabilities and signers
        let request = TreeAuthzRequest {
            operation,
            base_capabilities: CapabilitySet::from_permissions(&[
                "tree:read",
                "tree:propose",
                "tree:modify",
            ]),
            signer_count: 2,
            signers: participants.into_iter().take(2).collect(),
            guardian_signers: BTreeSet::new(),
        };

        let result = evaluate_tree_operation(request, &context).unwrap();
        assert!(result.authorized);
        assert!(result.effective_capabilities.permits("tree:modify"));
    }

    #[test]
    fn test_threshold_not_met() {
        let account_id = AccountId::from_bytes([1u8; 32]);
        let mut context = TreeAuthzContext::new(account_id, 1);

        let participants = BTreeSet::from([
            DeviceId::from_bytes([1u8; 32]),
            DeviceId::from_bytes([2u8; 32]),
            DeviceId::from_bytes([3u8; 32]),
        ]);

        let threshold_config = ThresholdConfig::new(2, participants.clone());
        let policy = TreePolicy::new(
            NodeIndex(0),
            account_id,
            Policy::Threshold { m: 2, n: 3 },
            threshold_config,
        );

        context.add_policy(0, policy);

        let operation = TreeOp {
            parent_epoch: 1,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf_id: 4,
                role: LeafRole::Device,
                under: 0,
            },
            version: 1,
        };

        // Request with insufficient signers
        let request = TreeAuthzRequest {
            operation,
            base_capabilities: CapabilitySet::from_permissions(&[
                "tree:read",
                "tree:propose",
                "tree:modify",
            ]),
            signer_count: 1, // Not enough!
            signers: participants.into_iter().take(1).collect(),
            guardian_signers: BTreeSet::new(),
        };

        let result = evaluate_tree_operation(request, &context).unwrap();
        assert!(!result.authorized);
        assert!(result.failure_reason.unwrap().contains("Threshold not met"));
    }

    #[test]
    fn test_missing_capabilities() {
        let account_id = AccountId::from_bytes([1u8; 32]);
        let mut context = TreeAuthzContext::new(account_id, 1);

        let participants = BTreeSet::from([
            DeviceId::from_bytes([1u8; 32]),
            DeviceId::from_bytes([2u8; 32]),
        ]);

        let threshold_config = ThresholdConfig::new(1, participants.clone());
        let policy = TreePolicy::new(NodeIndex(0), account_id, Policy::Any, threshold_config);

        context.add_policy(0, policy);

        let operation = TreeOp {
            parent_epoch: 1,
            parent_commitment: [0u8; 32],
            op: TreeOpKind::AddLeaf {
                leaf_id: 4,
                role: LeafRole::Device,
                under: 0,
            },
            version: 1,
        };

        // Request with insufficient capabilities
        let request = TreeAuthzRequest {
            operation,
            base_capabilities: CapabilitySet::from_permissions(&[
                "tree:read", // Missing tree:propose and tree:modify
            ]),
            signer_count: 1,
            signers: participants.into_iter().take(1).collect(),
            guardian_signers: BTreeSet::new(),
        };

        let result = evaluate_tree_operation(request, &context).unwrap();
        assert!(!result.authorized);
        assert!(result.failure_reason.unwrap().contains("tree:propose"));
    }
}
