//! Tree policy integration for ratchet tree authorization
//!
//! This module integrates the ratchet tree Policy enum with the capability
//! system, providing authorization evaluation for tree operations.

use crate::{CapabilitySet, WotError};
use aura_core::{AccountId, DeviceId, GuardianId};
use std::collections::BTreeSet;

/// Policy configuration from ratchet tree spec
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Policy {
    /// 1-of-n (any participant can authorize)
    Any,
    /// M-of-N threshold requirement
    Threshold { m: u16, n: u16 },
    /// N-of-N (all participants must authorize)
    All,
}

// Re-export as TreePolicyEnum for external use
pub use Policy as TreePolicyEnum;

impl Policy {
    /// Check if this policy is more restrictive than other
    pub fn is_more_restrictive_than(&self, other: &Policy) -> bool {
        use Policy::*;
        match (self, other) {
            (All, Threshold { m, n }) => *m == *n,
            (All, Any) => true,
            (Threshold { m: m1, n: n1 }, Threshold { m: m2, n: n2 }) => n1 == n2 && m1 >= m2,
            (Threshold { m, .. }, Any) => *m >= 1,
            _ => false,
        }
    }

    /// Meet operation for policies (selects more restrictive)
    pub fn meet(&self, other: &Policy) -> Policy {
        if self.is_more_restrictive_than(other) {
            self.clone()
        } else if other.is_more_restrictive_than(self) {
            other.clone()
        } else {
            // If neither is more restrictive, choose the one with higher threshold
            match (self, other) {
                (Policy::Threshold { m: m1, .. }, Policy::Threshold { m: m2, .. }) => {
                    if m1 >= m2 {
                        self.clone()
                    } else {
                        other.clone()
                    }
                }
                _ => self.clone(), // Default to self if incomparable
            }
        }
    }
}

/// Node index in the ratchet tree
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NodeIndex(pub u32);

/// Threshold configuration for a tree node
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ThresholdConfig {
    /// Required number of signatures
    pub threshold: u16,
    /// Total number of participants
    pub total: u16,
    /// Participating devices
    pub participants: BTreeSet<DeviceId>,
    /// Participating guardians (for recovery nodes)
    pub guardians: BTreeSet<GuardianId>,
}

impl ThresholdConfig {
    /// Create new threshold config
    pub fn new(threshold: u16, participants: BTreeSet<DeviceId>) -> Self {
        let total = participants.len() as u16;
        Self {
            threshold,
            total,
            participants,
            guardians: BTreeSet::new(),
        }
    }

    /// Add guardian participants
    pub fn with_guardians(mut self, guardians: BTreeSet<GuardianId>) -> Self {
        self.total += guardians.len() as u16;
        self.guardians = guardians;
        self
    }

    /// Check if threshold requirements are met
    pub fn is_threshold_met(&self, signer_count: u16) -> bool {
        signer_count >= self.threshold
    }

    /// Convert to ratchet tree Policy
    pub fn to_policy(&self) -> Policy {
        if self.threshold == 1 {
            Policy::Any
        } else if self.threshold == self.total {
            Policy::All
        } else {
            Policy::Threshold {
                m: self.threshold,
                n: self.total,
            }
        }
    }

    /// Create from ratchet tree Policy
    pub fn from_policy(
        policy: &Policy,
        participants: BTreeSet<DeviceId>,
        guardians: BTreeSet<GuardianId>,
    ) -> Self {
        let total = (participants.len() + guardians.len()) as u16;
        let threshold = match policy {
            Policy::Any => 1,
            Policy::All => total,
            Policy::Threshold { m, .. } => *m,
        };

        Self {
            threshold,
            total,
            participants,
            guardians,
        }
    }
}

/// Tree policy connecting tree nodes to capability requirements
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TreePolicy {
    /// Tree node this policy applies to
    pub node: NodeIndex,
    /// Account this tree belongs to
    pub account_id: AccountId,
    /// Policy requirements
    pub policy: Policy,
    /// Threshold configuration
    pub threshold_config: ThresholdConfig,
    /// Required capabilities for operations on this node
    pub required_capabilities: CapabilitySet,
}

impl TreePolicy {
    /// Create new tree policy
    pub fn new(
        node: NodeIndex,
        account_id: AccountId,
        policy: Policy,
        threshold_config: ThresholdConfig,
    ) -> Self {
        // Default capabilities for tree operations
        let required_capabilities =
            CapabilitySet::from_permissions(&["tree:read", "tree:propose", "tree:sign"]);

        Self {
            node,
            account_id,
            policy,
            threshold_config,
            required_capabilities,
        }
    }

    /// Check if policy requirements are satisfied by signer set
    pub fn evaluate_signers(&self, signer_count: u16) -> bool {
        match &self.policy {
            Policy::Any => signer_count >= 1,
            Policy::All => signer_count == self.threshold_config.total,
            Policy::Threshold { m, .. } => signer_count >= *m,
        }
    }

    /// Get effective capabilities by meeting with policy requirements
    pub fn effective_capabilities(&self, base_capabilities: &CapabilitySet) -> CapabilitySet {
        base_capabilities.meet(&self.required_capabilities)
    }

    /// Meet operation for tree policies (more restrictive wins)
    pub fn meet(&self, other: &TreePolicy) -> Result<TreePolicy, WotError> {
        if self.node != other.node {
            return Err(WotError::invalid(
                "Cannot meet policies from different nodes",
            ));
        }

        if self.account_id != other.account_id {
            return Err(WotError::invalid(
                "Cannot meet policies from different accounts",
            ));
        }

        let policy = self.policy.meet(&other.policy);
        let required_capabilities = self
            .required_capabilities
            .meet(&other.required_capabilities);

        // Use the more restrictive threshold config
        let threshold_config =
            if self.threshold_config.threshold >= other.threshold_config.threshold {
                self.threshold_config.clone()
            } else {
                other.threshold_config.clone()
            };

        Ok(TreePolicy {
            node: self.node,
            account_id: self.account_id,
            policy,
            threshold_config,
            required_capabilities,
        })
    }

    /// Create default recovery trust policy
    pub fn default_recovery_trust() -> Self {
        let default_node = NodeIndex(0);
        let default_account = AccountId::new();
        let recovery_policy = Policy::Threshold { m: 2, n: 3 }; // 2-of-3 threshold for recovery
        
        // Create default participants for recovery policy
        let mut participants = BTreeSet::new();
        participants.insert(DeviceId::new());
        participants.insert(DeviceId::new());
        participants.insert(DeviceId::new());
        
        let threshold_config = ThresholdConfig::new(2, participants);
        
        Self::new(default_node, default_account, recovery_policy, threshold_config)
    }

    /// Get minimum trust score required by this policy
    pub fn minimum_trust_score(&self) -> f64 {
        match &self.policy {
            Policy::Any => 0.3,      // Low trust for any approval
            Policy::Threshold { m, n } => {
                let ratio = *m as f64 / *n as f64;
                0.5 + ratio * 0.3  // Scale from 0.5 to 0.8 based on threshold strictness
            }
            Policy::All => 0.8,     // High trust for unanimous approval
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_meet_laws() {
        let any = Policy::Any;
        let threshold_2_3 = Policy::Threshold { m: 2, n: 3 };
        let threshold_3_3 = Policy::Threshold { m: 3, n: 3 };
        let all = Policy::All;

        // Test that meet selects more restrictive policy
        assert_eq!(any.meet(&threshold_2_3), threshold_2_3);
        assert_eq!(threshold_2_3.meet(&threshold_3_3), threshold_3_3);
        assert_eq!(threshold_3_3.meet(&all), all);

        // Test commutativity
        assert_eq!(any.meet(&threshold_2_3), threshold_2_3.meet(&any));

        // Test idempotency
        assert_eq!(threshold_2_3.meet(&threshold_2_3), threshold_2_3);
    }

    #[test]
    fn test_threshold_config_conversion() {
        let participants = BTreeSet::from([
            DeviceId::from_bytes([1u8; 32]),
            DeviceId::from_bytes([2u8; 32]),
            DeviceId::from_bytes([3u8; 32]),
        ]);

        let config = ThresholdConfig::new(2, participants.clone());
        let policy = config.to_policy();

        assert_eq!(policy, Policy::Threshold { m: 2, n: 3 });

        let reconstructed = ThresholdConfig::from_policy(&policy, participants, BTreeSet::new());
        assert_eq!(reconstructed.threshold, 2);
        assert_eq!(reconstructed.total, 3);
    }
}
