//! Guardian relationship types for recovery contexts
//!
//! This module defines the types used for guardian configuration
//! and recovery operations in relational contexts.

use crate::consensus::ConsensusProof;
use aura_core::Hash32;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Guardian binding between an account and guardian authority
///
/// This binding establishes a guardian relationship that allows
/// the guardian to participate in recovery operations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GuardianBinding {
    /// Commitment hash of the account authority
    pub account_commitment: Hash32,
    /// Commitment hash of the guardian authority
    pub guardian_commitment: Hash32,
    /// Parameters governing this guardian relationship
    pub parameters: GuardianParameters,
    /// Optional consensus proof if binding required agreement
    pub consensus_proof: Option<ConsensusProof>,
}

/// Parameters for guardian relationships
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GuardianParameters {
    /// Time delay required before recovery can be executed
    pub recovery_delay: Duration,
    /// Whether notification to the account is required
    pub notification_required: bool,
    /// Optional expiration time for this binding
    pub expiration: Option<chrono::DateTime<chrono::Utc>>,
}

impl Default for GuardianParameters {
    fn default() -> Self {
        Self {
            recovery_delay: Duration::from_secs(24 * 60 * 60), // 24 hours
            notification_required: true,
            expiration: None,
        }
    }
}

/// Recovery grant allowing a guardian to modify an account
///
/// This grant represents approval for a specific recovery operation
/// and must be agreed upon through consensus.
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
    pub consensus_proof: ConsensusProof,
}

/// Types of recovery operations
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RecoveryOp {
    /// Replace the entire ratchet tree
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
    /// Check if this operation requires immediate action
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
}

/// Builder for creating guardian bindings
pub struct GuardianBindingBuilder {
    account_commitment: Option<Hash32>,
    guardian_commitment: Option<Hash32>,
    parameters: GuardianParameters,
}

impl GuardianBindingBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            account_commitment: None,
            guardian_commitment: None,
            parameters: GuardianParameters::default(),
        }
    }

    /// Set the account commitment
    pub fn account(mut self, commitment: Hash32) -> Self {
        self.account_commitment = Some(commitment);
        self
    }

    /// Set the guardian commitment
    pub fn guardian(mut self, commitment: Hash32) -> Self {
        self.guardian_commitment = Some(commitment);
        self
    }

    /// Set the recovery delay
    pub fn recovery_delay(mut self, delay: Duration) -> Self {
        self.parameters.recovery_delay = delay;
        self
    }

    /// Set notification requirement
    pub fn notification_required(mut self, required: bool) -> Self {
        self.parameters.notification_required = required;
        self
    }

    /// Set expiration time
    pub fn expires_at(mut self, expiration: chrono::DateTime<chrono::Utc>) -> Self {
        self.parameters.expiration = Some(expiration);
        self
    }

    /// Build the guardian binding
    pub fn build(self) -> Result<GuardianBinding, &'static str> {
        let account_commitment = self
            .account_commitment
            .ok_or("Account commitment required")?;
        let guardian_commitment = self
            .guardian_commitment
            .ok_or("Guardian commitment required")?;

        Ok(GuardianBinding {
            account_commitment,
            guardian_commitment,
            parameters: self.parameters,
            consensus_proof: None,
        })
    }
}

impl Default for GuardianBindingBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_guardian_binding_builder() {
        let account = Hash32::default();
        let guardian = Hash32([1u8; 32]);

        let binding = GuardianBindingBuilder::new()
            .account(account)
            .guardian(guardian)
            .recovery_delay(Duration::from_secs(3600))
            .notification_required(false)
            .build()
            .unwrap();

        assert_eq!(binding.account_commitment, account);
        assert_eq!(binding.guardian_commitment, guardian);
        assert_eq!(binding.parameters.recovery_delay, Duration::from_secs(3600));
        assert!(!binding.parameters.notification_required);
    }

    #[test]
    fn test_recovery_op_emergency() {
        let replace = RecoveryOp::ReplaceTree {
            new_tree_root: Hash32::default(),
        };
        assert!(!replace.is_emergency());

        let emergency = RecoveryOp::EmergencyRotation { new_epoch: 42 };
        assert!(emergency.is_emergency());
    }
}
