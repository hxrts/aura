//! Policy meet operations for tree policy evaluation
//!
//! This module implements meet-semilattice operations for tree policies,
//! ensuring capability intersection follows mathematical laws.

use crate::tree_policy::{Policy, ThresholdConfig};
use crate::{CapabilitySet, TreePolicy, WotError};
use std::collections::BTreeMap;

/// Policy meet operations for tree authorization
pub struct PolicyMeet;

impl PolicyMeet {
    /// Meet operation for capability sets with tree policies
    ///
    /// This combines base capabilities with tree policy requirements,
    /// ensuring the result can only shrink (never expand) capabilities.
    pub fn meet_capabilities_with_tree_policy(
        base_capabilities: &CapabilitySet,
        tree_policy: &TreePolicy,
    ) -> CapabilitySet {
        // Tree policy requirements act as a filter on base capabilities
        base_capabilities.meet(&tree_policy.required_capabilities)
    }

    /// Meet multiple tree policies to find most restrictive
    ///
    /// When multiple policies apply to an operation, we take the meet
    /// (intersection) to get the most restrictive policy.
    pub fn meet_tree_policies(policies: &[TreePolicy]) -> Result<Option<TreePolicy>, WotError> {
        if policies.is_empty() {
            return Ok(None);
        }

        if policies.len() == 1 {
            return Ok(Some(policies[0].clone()));
        }

        // Start with first policy
        let mut result = policies[0].clone();

        // Meet with each additional policy
        for policy in &policies[1..] {
            result = result.meet(policy)?;
        }

        Ok(Some(result))
    }

    /// Evaluate effective capabilities for a set of policies
    ///
    /// This takes base capabilities and applies all relevant tree policies
    /// using meet operations to ensure monotonic restriction.
    pub fn evaluate_effective_capabilities(
        base_capabilities: &CapabilitySet,
        applicable_policies: &[TreePolicy],
    ) -> Result<CapabilitySet, WotError> {
        // If no policies apply, return base capabilities
        if applicable_policies.is_empty() {
            return Ok(base_capabilities.clone());
        }

        // Meet all applicable policies
        let combined_policy = Self::meet_tree_policies(applicable_policies)?;

        match combined_policy {
            Some(policy) => Ok(Self::meet_capabilities_with_tree_policy(
                base_capabilities,
                &policy,
            )),
            None => Ok(base_capabilities.clone()),
        }
    }

    /// Check if a threshold requirement is met given current signers
    pub fn evaluate_threshold_requirement(
        policy: &Policy,
        total_signers: u16,
        actual_signers: u16,
    ) -> bool {
        match policy {
            Policy::Any => actual_signers >= 1,
            Policy::All => actual_signers == total_signers,
            Policy::Threshold { m, n: _ } => actual_signers >= *m,
        }
    }

    /// Combine multiple threshold configurations using meet
    ///
    /// When multiple threshold configs apply, we take the most restrictive
    /// (highest threshold requirement).
    pub fn meet_threshold_configs(configs: &[ThresholdConfig]) -> Option<ThresholdConfig> {
        if configs.is_empty() {
            return None;
        }

        // Find config with highest threshold requirement
        let most_restrictive = configs.iter().max_by_key(|config| config.threshold)?;

        Some(most_restrictive.clone())
    }

    /// Create a policy lattice hierarchy for evaluation
    ///
    /// This builds a map of policies ordered by restrictiveness
    /// for efficient policy lookup and evaluation.
    pub fn build_policy_lattice(policies: &[TreePolicy]) -> BTreeMap<u32, Vec<TreePolicy>> {
        let mut lattice: BTreeMap<u32, Vec<TreePolicy>> = BTreeMap::new();

        for policy in policies {
            lattice
                .entry(policy.node.0)
                .or_default()
                .push(policy.clone());
        }

        // Sort each node's policies by restrictiveness
        for policies in lattice.values_mut() {
            policies.sort_by(|a, b| {
                // More restrictive policies come first
                match (&a.policy, &b.policy) {
                    (Policy::All, Policy::Threshold { .. }) => std::cmp::Ordering::Less,
                    (Policy::All, Policy::Any) => std::cmp::Ordering::Less,
                    (Policy::Threshold { m: m1, .. }, Policy::Threshold { m: m2, .. }) => {
                        m1.cmp(m2).reverse() // Higher threshold = more restrictive
                    }
                    (Policy::Threshold { .. }, Policy::Any) => std::cmp::Ordering::Less,
                    _ => std::cmp::Ordering::Equal,
                }
            });
        }

        lattice
    }
}

/// Extensions for capability evaluation with tree policies
pub trait TreePolicyCapabilityExt {
    /// Check if capabilities are sufficient for a tree policy
    fn satisfies_tree_policy(&self, policy: &TreePolicy) -> bool;

    /// Get the intersection with tree policy requirements
    fn intersect_with_tree_policy(&self, policy: &TreePolicy) -> CapabilitySet;
}

impl TreePolicyCapabilityExt for CapabilitySet {
    fn satisfies_tree_policy(&self, policy: &TreePolicy) -> bool {
        // Check if we have all required capabilities
        let intersection = self.meet(&policy.required_capabilities);
        intersection == policy.required_capabilities
    }

    fn intersect_with_tree_policy(&self, policy: &TreePolicy) -> CapabilitySet {
        PolicyMeet::meet_capabilities_with_tree_policy(self, policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree_policy::{NodeIndex, Policy};
    use aura_core::AccountId;

    #[test]
    fn test_capability_meet_with_tree_policy() {
        let account_id = AccountId::from_bytes([1u8; 32]);
        let threshold_config = ThresholdConfig::new(2, std::collections::BTreeSet::new());

        let mut tree_policy = TreePolicy::new(
            NodeIndex(0),
            account_id,
            Policy::Threshold { m: 2, n: 3 },
            threshold_config,
        );

        // Override required capabilities for test
        tree_policy.required_capabilities =
            CapabilitySet::from_permissions(&["tree:read", "tree:modify"]);

        let base_capabilities = CapabilitySet::from_permissions(&[
            "tree:read",
            "tree:modify",
            "tree:admin", // Extra capability
        ]);

        let result =
            PolicyMeet::meet_capabilities_with_tree_policy(&base_capabilities, &tree_policy);

        // Result should only contain capabilities present in both
        assert!(result.permits("tree:read"));
        assert!(result.permits("tree:modify"));
        assert!(!result.permits("tree:admin")); // Not in tree policy requirements
    }

    #[test]
    fn test_meet_tree_policies() {
        let account_id = AccountId::from_bytes([1u8; 32]);

        let policy1 = TreePolicy::new(
            NodeIndex(0),
            account_id,
            Policy::Any,
            ThresholdConfig::new(1, std::collections::BTreeSet::new()),
        );

        let policy2 = TreePolicy::new(
            NodeIndex(0),
            account_id,
            Policy::Threshold { m: 2, n: 3 },
            ThresholdConfig::new(2, std::collections::BTreeSet::new()),
        );

        let result = PolicyMeet::meet_tree_policies(&[policy1, policy2])
            .unwrap()
            .unwrap();

        // Should get the more restrictive policy
        assert_eq!(result.policy, Policy::Threshold { m: 2, n: 3 });
        assert_eq!(result.threshold_config.threshold, 2);
    }

    #[test]
    fn test_threshold_requirement_evaluation() {
        // Test Any policy
        assert!(PolicyMeet::evaluate_threshold_requirement(
            &Policy::Any,
            3,
            1
        ));
        assert!(!PolicyMeet::evaluate_threshold_requirement(
            &Policy::Any,
            3,
            0
        ));

        // Test All policy
        assert!(PolicyMeet::evaluate_threshold_requirement(
            &Policy::All,
            3,
            3
        ));
        assert!(!PolicyMeet::evaluate_threshold_requirement(
            &Policy::All,
            3,
            2
        ));

        // Test Threshold policy
        let threshold = Policy::Threshold { m: 2, n: 3 };
        assert!(PolicyMeet::evaluate_threshold_requirement(&threshold, 3, 2));
        assert!(PolicyMeet::evaluate_threshold_requirement(&threshold, 3, 3));
        assert!(!PolicyMeet::evaluate_threshold_requirement(
            &threshold, 3, 1
        ));
    }

    #[test]
    fn test_policy_lattice_ordering() {
        let account_id = AccountId::from_bytes([1u8; 32]);

        let policies = vec![
            TreePolicy::new(
                NodeIndex(0),
                account_id,
                Policy::Any,
                ThresholdConfig::new(1, std::collections::BTreeSet::new()),
            ),
            TreePolicy::new(
                NodeIndex(0),
                account_id,
                Policy::Threshold { m: 2, n: 3 },
                ThresholdConfig::new(2, std::collections::BTreeSet::new()),
            ),
            TreePolicy::new(
                NodeIndex(0),
                account_id,
                Policy::All,
                ThresholdConfig::new(3, std::collections::BTreeSet::new()),
            ),
        ];

        let lattice = PolicyMeet::build_policy_lattice(&policies);
        let node_policies = &lattice[&0];

        // Should be ordered by restrictiveness (most restrictive first)
        assert_eq!(node_policies[0].policy, Policy::All);
        assert_eq!(node_policies[1].policy, Policy::Threshold { m: 2, n: 3 });
        assert_eq!(node_policies[2].policy, Policy::Any);
    }

    #[test]
    fn test_capability_extension_trait() {
        let account_id = AccountId::from_bytes([1u8; 32]);
        let threshold_config = ThresholdConfig::new(2, std::collections::BTreeSet::new());

        let mut tree_policy = TreePolicy::new(
            NodeIndex(0),
            account_id,
            Policy::Threshold { m: 2, n: 3 },
            threshold_config,
        );

        tree_policy.required_capabilities =
            CapabilitySet::from_permissions(&["tree:read", "tree:modify"]);

        let sufficient_caps =
            CapabilitySet::from_permissions(&["tree:read", "tree:modify", "tree:admin"]);

        let insufficient_caps = CapabilitySet::from_permissions(&[
            "tree:read",
            // Missing tree:modify
        ]);

        assert!(sufficient_caps.satisfies_tree_policy(&tree_policy));
        assert!(!insufficient_caps.satisfies_tree_policy(&tree_policy));

        let intersection = sufficient_caps.intersect_with_tree_policy(&tree_policy);
        assert!(intersection.permits("tree:read"));
        assert!(intersection.permits("tree:modify"));
        assert!(!intersection.permits("tree:admin"));
    }
}
