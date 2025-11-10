//! Capability evaluation for tree operations
//!
//! This module provides high-level capability evaluation functions that integrate
//! tree policies, delegation chains, and meet operations.

use crate::tree_policy::{Policy, ThresholdConfig};
use crate::{
    CapabilitySet, DelegationChain, NodeIndex, PolicyMeet, TreeAuthzContext, TreeOp, TreePolicy,
    TreePolicyCapabilityExt, WotError,
};
use aura_core::{AccountId, DeviceId, GuardianId};
use std::collections::BTreeSet;

/// Complete capability evaluation context
#[derive(Debug, Clone)]
pub struct CapabilityEvaluationContext {
    /// Base capabilities granted to the entity
    pub base_capabilities: CapabilitySet,
    /// Delegation chains that may enhance capabilities
    pub delegation_chains: Vec<DelegationChain>,
    /// Tree authorization context
    pub tree_context: TreeAuthzContext,
    /// Local policy constraints
    pub local_policy: Option<CapabilitySet>,
}

impl CapabilityEvaluationContext {
    /// Create new capability evaluation context
    pub fn new(base_capabilities: CapabilitySet, tree_context: TreeAuthzContext) -> Self {
        Self {
            base_capabilities,
            delegation_chains: Vec::new(),
            tree_context,
            local_policy: None,
        }
    }

    /// Add delegation chain to context
    pub fn with_delegation_chain(mut self, chain: DelegationChain) -> Self {
        self.delegation_chains.push(chain);
        self
    }

    /// Set local policy constraints
    pub fn with_local_policy(mut self, policy: CapabilitySet) -> Self {
        self.local_policy = Some(policy);
        self
    }
}

/// Evaluation request for tree operations
#[derive(Debug, Clone)]
pub struct TreeCapabilityRequest {
    /// The tree operation to evaluate
    pub operation: TreeOp,
    /// Entity requesting the operation
    pub requester: EntityId,
    /// Current signers for the operation
    pub signers: BTreeSet<DeviceId>,
    /// Guardian signers (for recovery operations)
    pub guardian_signers: BTreeSet<GuardianId>,
}

/// Entity identifier for capability evaluation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityId {
    Device(DeviceId),
    Guardian(GuardianId),
    Account(AccountId),
}

/// Result of capability evaluation
#[derive(Debug, Clone)]
pub struct CapabilityEvaluationResult {
    /// Whether the operation is permitted
    pub permitted: bool,
    /// Final effective capabilities after all meets
    pub effective_capabilities: CapabilitySet,
    /// Tree policies that were evaluated
    pub evaluated_policies: Vec<TreePolicy>,
    /// Reason for denial if not permitted
    pub denial_reason: Option<String>,
}

impl CapabilityEvaluationResult {
    /// Create successful evaluation result
    pub fn permitted(
        effective_capabilities: CapabilitySet,
        evaluated_policies: Vec<TreePolicy>,
    ) -> Self {
        Self {
            permitted: true,
            effective_capabilities,
            evaluated_policies,
            denial_reason: None,
        }
    }

    /// Create denied evaluation result
    pub fn denied(reason: String) -> Self {
        Self {
            permitted: false,
            effective_capabilities: CapabilitySet::empty(),
            evaluated_policies: Vec::new(),
            denial_reason: Some(reason),
        }
    }
}

/// Evaluate tree operation capabilities with full policy integration
pub fn evaluate_tree_operation_capabilities(
    request: TreeCapabilityRequest,
    context: CapabilityEvaluationContext,
) -> Result<CapabilityEvaluationResult, WotError> {
    // Step 1: Determine which tree policies apply to this operation
    let applicable_policies = get_applicable_policies(&request.operation, &context.tree_context);

    if applicable_policies.is_empty() {
        return Ok(CapabilityEvaluationResult::denied(
            "No applicable tree policies found".to_string(),
        ));
    }

    // Step 2: Evaluate base capabilities with delegations
    let mut working_capabilities = context.base_capabilities.clone();

    // Apply delegation chains (each delegation can only restrict, never expand)
    for _delegation_chain in &context.delegation_chains {
        // In a full implementation, this would walk the delegation chain
        // and compute effective capabilities. TODO fix - For now, we assume the
        // delegation chain has been pre-computed to a capability set.
        // working_capabilities = working_capabilities.meet(&delegation_chain.effective_capabilities());
    }

    // Step 3: Apply local policy constraints
    if let Some(local_policy) = &context.local_policy {
        working_capabilities = working_capabilities.meet(local_policy);
    }

    // Step 4: Meet with tree policy requirements
    let effective_capabilities =
        PolicyMeet::evaluate_effective_capabilities(&working_capabilities, &applicable_policies)?;

    // Step 5: Check if effective capabilities are sufficient for the operation
    let operation_requirements = crate::tree_operation_capabilities(&request.operation.op);

    if !effective_capabilities.satisfies_tree_policy(
        &TreePolicy::new(
            crate::NodeIndex(0),
            context.tree_context.account_id,
            Policy::Any,
            crate::ThresholdConfig::new(1, std::collections::BTreeSet::new()),
        )
        .with_required_capabilities(operation_requirements),
    ) {
        return Ok(CapabilityEvaluationResult::denied(
            "Insufficient capabilities for operation".to_string(),
        ));
    }

    // Step 6: Verify threshold requirements are met
    let signer_count = (request.signers.len() + request.guardian_signers.len()) as u16;

    for policy in &applicable_policies {
        if !policy.evaluate_signers(signer_count) {
            return Ok(CapabilityEvaluationResult::denied(format!(
                "Threshold not met for policy at node {}: need {}, got {}",
                policy.node.0, policy.threshold_config.threshold, signer_count
            )));
        }
    }

    // All checks passed
    Ok(CapabilityEvaluationResult::permitted(
        effective_capabilities,
        applicable_policies,
    ))
}

/// Get applicable policies for a tree operation
fn get_applicable_policies(operation: &TreeOp, context: &TreeAuthzContext) -> Vec<TreePolicy> {
    let target_nodes = match &operation.op {
        crate::TreeOpKind::AddLeaf { under, .. } => vec![*under],
        crate::TreeOpKind::RemoveLeaf { leaf_id, .. } => vec![*leaf_id], // TODO fix - Simplified
        crate::TreeOpKind::ChangePolicy { node, .. } => vec![*node],
        crate::TreeOpKind::RotateEpoch { affected } => affected.clone(),
    };

    let mut applicable_policies = Vec::new();

    for node in target_nodes {
        if let Some(policy) = context.get_policy(node) {
            applicable_policies.push(policy.clone());
        }
    }

    applicable_policies
}

/// Extensions for TreePolicy to support capability evaluation
impl TreePolicy {
    /// Set required capabilities for this policy
    pub fn with_required_capabilities(mut self, capabilities: CapabilitySet) -> Self {
        self.required_capabilities = capabilities;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeIndex, Policy, ThresholdConfig};

    #[test]
    fn test_complete_capability_evaluation() {
        let account_id = AccountId::from_bytes([1u8; 32]);
        let device_id = DeviceId::from_bytes([1u8; 32]);

        // Set up tree context
        let mut tree_context = TreeAuthzContext::new(account_id, 1);

        let participants = BTreeSet::from([device_id]);
        let threshold_config = ThresholdConfig::new(1, participants.clone());
        let tree_policy = TreePolicy::new(NodeIndex(0), account_id, Policy::Any, threshold_config);

        tree_context.add_policy(0, tree_policy);

        // Set up capability context
        let base_capabilities =
            CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:modify"]);

        let context = CapabilityEvaluationContext::new(base_capabilities, tree_context);

        // Create operation request
        let operation = TreeOp {
            parent_epoch: 1,
            parent_commitment: [0u8; 32],
            op: crate::TreeOpKind::AddLeaf {
                leaf_id: 2,
                role: crate::LeafRole::Device,
                under: 0,
            },
            version: 1,
        };

        let request = TreeCapabilityRequest {
            operation,
            requester: EntityId::Device(device_id),
            signers: participants,
            guardian_signers: BTreeSet::new(),
        };

        let result = evaluate_tree_operation_capabilities(request, context).unwrap();

        assert!(result.permitted);
        assert!(result.effective_capabilities.permits("tree:read"));
        assert!(result.effective_capabilities.permits("tree:propose"));
        assert!(result.effective_capabilities.permits("tree:modify"));
        assert_eq!(result.evaluated_policies.len(), 1);
    }

    #[test]
    fn test_insufficient_threshold() {
        let account_id = AccountId::from_bytes([1u8; 32]);
        let device1 = DeviceId::from_bytes([1u8; 32]);
        let device2 = DeviceId::from_bytes([2u8; 32]);

        // Set up tree context with 2-of-2 threshold
        let mut tree_context = TreeAuthzContext::new(account_id, 1);

        let participants = BTreeSet::from([device1, device2]);
        let threshold_config = ThresholdConfig::new(2, participants);
        let tree_policy = TreePolicy::new(
            NodeIndex(0),
            account_id,
            Policy::All, // Requires all signers
            threshold_config,
        );

        tree_context.add_policy(0, tree_policy);

        let context = CapabilityEvaluationContext::new(
            CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:modify"]),
            tree_context,
        );

        let operation = TreeOp {
            parent_epoch: 1,
            parent_commitment: [0u8; 32],
            op: crate::TreeOpKind::AddLeaf {
                leaf_id: 3,
                role: crate::LeafRole::Device,
                under: 0,
            },
            version: 1,
        };

        let request = TreeCapabilityRequest {
            operation,
            requester: EntityId::Device(device1),
            signers: BTreeSet::from([device1]), // Only 1 of 2 required signers
            guardian_signers: BTreeSet::new(),
        };

        let result = evaluate_tree_operation_capabilities(request, context).unwrap();

        assert!(!result.permitted);
        assert!(result.denial_reason.unwrap().contains("Threshold not met"));
    }
}
