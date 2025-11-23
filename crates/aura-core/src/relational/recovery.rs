//! Recovery operation types for guardian-based recovery
//!
//! This module defines the domain types for recovery operations
//! that can be performed by guardians on behalf of account authorities.

use crate::Hash32;
use serde::{Deserialize, Serialize};

/// Recovery grant allowing a guardian to modify an account
///
/// This grant represents approval for a specific recovery operation
/// and must be agreed upon through consensus. It is a pure domain type
/// that contains the essential information for a recovery operation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecoveryGrant {
    /// Previous commitment of the account being recovered
    pub account_old: Hash32,
    /// New commitment after recovery operation
    pub account_new: Hash32,
    /// Guardian authority granting the recovery
    pub guardian: Hash32,
    /// Type of recovery operation being performed
    pub operation: RecoveryOp,
    /// Consensus proof showing agreement on this grant
    pub consensus_proof: super::consensus::ConsensusProof,
}

impl RecoveryGrant {
    /// Create a new recovery grant
    pub fn new(
        account_old: Hash32,
        account_new: Hash32,
        guardian: Hash32,
        operation: RecoveryOp,
        consensus_proof: super::consensus::ConsensusProof,
    ) -> Self {
        Self {
            account_old,
            account_new,
            guardian,
            operation,
            consensus_proof,
        }
    }

    /// Check if this is an emergency recovery operation
    pub fn is_emergency(&self) -> bool {
        self.operation.is_emergency()
    }

    /// Get a description of the recovery operation
    pub fn operation_description(&self) -> &'static str {
        self.operation.description()
    }

    /// Check if this grant modifies the account structure
    pub fn modifies_account_structure(&self) -> bool {
        matches!(
            self.operation,
            RecoveryOp::ReplaceTree { .. }
                | RecoveryOp::AddDevice { .. }
                | RecoveryOp::RemoveDevice { .. }
                | RecoveryOp::UpdatePolicy { .. }
        )
    }

    /// Check if this grant involves key rotation
    pub fn involves_key_rotation(&self) -> bool {
        matches!(
            self.operation,
            RecoveryOp::EmergencyRotation { .. } | RecoveryOp::ReplaceTree { .. }
        )
    }

    /// Get the operation type as a string
    pub fn operation_type(&self) -> &'static str {
        match &self.operation {
            RecoveryOp::ReplaceTree { .. } => "replace_tree",
            RecoveryOp::AddDevice { .. } => "add_device",
            RecoveryOp::RemoveDevice { .. } => "remove_device",
            RecoveryOp::UpdatePolicy { .. } => "update_policy",
            RecoveryOp::EmergencyRotation { .. } => "emergency_rotation",
        }
    }
}

/// Types of recovery operations that can be performed
///
/// These operations represent the different ways a guardian can help
/// recover or modify an account authority's structure.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RecoveryOp {
    /// Replace the entire commitment tree
    ReplaceTree {
        /// New tree root commitment
        new_tree_root: Hash32,
    },
    /// Add a new device to the tree
    AddDevice {
        /// Public key of the new device
        device_public_key: Vec<u8>,
    },
    /// Remove a device from the tree
    RemoveDevice {
        /// Leaf index of device to remove
        leaf_index: u32,
    },
    /// Update the threshold policy
    UpdatePolicy {
        /// New threshold value
        new_threshold: u16,
    },
    /// Emergency key rotation
    EmergencyRotation {
        /// New epoch after rotation
        new_epoch: u64,
    },
}

impl RecoveryOp {
    /// Check if this operation requires immediate action (emergency)
    pub fn is_emergency(&self) -> bool {
        matches!(self, RecoveryOp::EmergencyRotation { .. })
    }

    /// Get a human-readable description of the operation
    pub fn description(&self) -> &'static str {
        match self {
            RecoveryOp::ReplaceTree { .. } => "Replace entire tree structure",
            RecoveryOp::AddDevice { .. } => "Add new device",
            RecoveryOp::RemoveDevice { .. } => "Remove device",
            RecoveryOp::UpdatePolicy { .. } => "Update threshold policy",
            RecoveryOp::EmergencyRotation { .. } => "Emergency key rotation",
        }
    }

    /// Check if this operation adds or removes devices
    pub fn modifies_device_set(&self) -> bool {
        matches!(
            self,
            RecoveryOp::AddDevice { .. } | RecoveryOp::RemoveDevice { .. }
        )
    }

    /// Check if this operation changes cryptographic material
    pub fn changes_cryptographic_material(&self) -> bool {
        matches!(
            self,
            RecoveryOp::ReplaceTree { .. } | RecoveryOp::EmergencyRotation { .. }
        )
    }

    /// Check if this operation requires tree restructuring
    pub fn requires_tree_restructuring(&self) -> bool {
        matches!(
            self,
            RecoveryOp::ReplaceTree { .. }
                | RecoveryOp::AddDevice { .. }
                | RecoveryOp::RemoveDevice { .. }
        )
    }

    /// Get the severity level of this operation
    pub fn severity_level(&self) -> RecoverySeverity {
        match self {
            RecoveryOp::UpdatePolicy { .. } => RecoverySeverity::Low,
            RecoveryOp::AddDevice { .. } => RecoverySeverity::Medium,
            RecoveryOp::RemoveDevice { .. } => RecoverySeverity::Medium,
            RecoveryOp::ReplaceTree { .. } => RecoverySeverity::High,
            RecoveryOp::EmergencyRotation { .. } => RecoverySeverity::Critical,
        }
    }

    /// Create a replace tree operation
    pub fn replace_tree(new_tree_root: Hash32) -> Self {
        Self::ReplaceTree { new_tree_root }
    }

    /// Create an add device operation
    pub fn add_device(device_public_key: Vec<u8>) -> Self {
        Self::AddDevice { device_public_key }
    }

    /// Create a remove device operation
    pub fn remove_device(leaf_index: u32) -> Self {
        Self::RemoveDevice { leaf_index }
    }

    /// Create an update policy operation
    pub fn update_policy(new_threshold: u16) -> Self {
        Self::UpdatePolicy { new_threshold }
    }

    /// Create an emergency rotation operation
    pub fn emergency_rotation(new_epoch: u64) -> Self {
        Self::EmergencyRotation { new_epoch }
    }
}

/// Severity levels for recovery operations
///
/// Indicates the impact and risk level of different recovery operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RecoverySeverity {
    /// Low impact - configuration changes
    Low,
    /// Medium impact - device addition/removal
    Medium,
    /// High impact - structural changes
    High,
    /// Critical impact - emergency operations
    Critical,
}

impl RecoverySeverity {
    /// Check if this severity level meets a minimum requirement
    pub fn meets_minimum(&self, minimum: RecoverySeverity) -> bool {
        *self >= minimum
    }

    /// Get all severity levels in order
    pub fn all() -> &'static [RecoverySeverity] {
        &[
            RecoverySeverity::Low,
            RecoverySeverity::Medium,
            RecoverySeverity::High,
            RecoverySeverity::Critical,
        ]
    }

    /// Get human-readable description
    pub fn description(&self) -> &'static str {
        match self {
            RecoverySeverity::Low => "Low impact configuration change",
            RecoverySeverity::Medium => "Medium impact device modification",
            RecoverySeverity::High => "High impact structural change",
            RecoverySeverity::Critical => "Critical emergency operation",
        }
    }
}

impl std::fmt::Display for RecoverySeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RecoverySeverity::Low => write!(f, "low"),
            RecoverySeverity::Medium => write!(f, "medium"),
            RecoverySeverity::High => write!(f, "high"),
            RecoverySeverity::Critical => write!(f, "critical"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recovery_op_emergency_detection() {
        let replace = RecoveryOp::replace_tree(Hash32::default());
        assert!(!replace.is_emergency());

        let emergency = RecoveryOp::emergency_rotation(42);
        assert!(emergency.is_emergency());
    }

    #[test]
    fn test_recovery_op_severity_levels() {
        let update_policy = RecoveryOp::update_policy(3);
        assert_eq!(update_policy.severity_level(), RecoverySeverity::Low);

        let add_device = RecoveryOp::add_device(vec![1, 2, 3]);
        assert_eq!(add_device.severity_level(), RecoverySeverity::Medium);

        let replace_tree = RecoveryOp::replace_tree(Hash32::default());
        assert_eq!(replace_tree.severity_level(), RecoverySeverity::High);

        let emergency = RecoveryOp::emergency_rotation(1);
        assert_eq!(emergency.severity_level(), RecoverySeverity::Critical);
    }

    #[test]
    fn test_recovery_op_categorization() {
        let add_device = RecoveryOp::add_device(vec![1, 2, 3]);
        assert!(add_device.modifies_device_set());
        assert!(add_device.requires_tree_restructuring());
        assert!(!add_device.changes_cryptographic_material());

        let emergency = RecoveryOp::emergency_rotation(1);
        assert!(!emergency.modifies_device_set());
        assert!(!emergency.requires_tree_restructuring());
        assert!(emergency.changes_cryptographic_material());
    }

    #[test]
    fn test_recovery_grant_properties() {
        use super::super::consensus::ConsensusProof;
        use crate::AuthorityId;

        let proof = ConsensusProof {
            prestate_hash: Hash32::default(),
            operation_hash: Hash32::default(),
            threshold_signature: None,
            attester_set: vec![AuthorityId::new()],
            threshold_met: true,
        };

        let grant = RecoveryGrant::new(
            Hash32::default(),
            Hash32::default(),
            Hash32::default(),
            RecoveryOp::emergency_rotation(1),
            proof,
        );

        assert!(grant.is_emergency());
        assert!(grant.involves_key_rotation());
        assert_eq!(grant.operation_type(), "emergency_rotation");
    }

    #[test]
    fn test_severity_ordering() {
        assert!(RecoverySeverity::Critical > RecoverySeverity::High);
        assert!(RecoverySeverity::High > RecoverySeverity::Medium);
        assert!(RecoverySeverity::Medium > RecoverySeverity::Low);

        assert!(RecoverySeverity::Critical.meets_minimum(RecoverySeverity::Low));
        assert!(!RecoverySeverity::Low.meets_minimum(RecoverySeverity::High));
    }
}