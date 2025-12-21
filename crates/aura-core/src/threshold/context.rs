//! Signing Context Types
//!
//! Defines what is being signed and the approval context.

use crate::tree::{TreeCommitment, TreeOp};
use crate::AuthorityId;
use serde::{Deserialize, Serialize};

/// Context for a threshold signing operation.
///
/// This unified context is used for all threshold signing scenarios:
/// - Multi-device personal signing
/// - Guardian recovery approvals
/// - Group operation approvals
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningContext {
    /// The authority whose keys are signing this operation
    pub authority: AuthorityId,

    /// What participants are signing
    pub operation: SignableOperation,

    /// Context for audit/display - why this signature is being requested
    pub approval_context: ApprovalContext,
}

/// Operations that can be threshold-signed.
///
/// All operations are signed the same way; the enum distinguishes
/// what is being signed for verification and audit purposes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignableOperation {
    /// Tree operation (commitment tree update)
    ///
    /// Used for: adding devices, changing policies, key rotation
    TreeOp(TreeOp),

    /// Recovery approval
    ///
    /// Used when guardians approve recovery of another authority
    RecoveryApproval {
        /// Authority being recovered
        target: AuthorityId,
        /// Proposed new tree root after recovery
        new_root: TreeCommitment,
    },

    /// Group proposal approval
    ///
    /// Used for multi-party group decisions
    GroupProposal {
        /// Group authority making the decision
        group: AuthorityId,
        /// Action being approved
        action: GroupAction,
    },

    /// Arbitrary message signing
    ///
    /// Used for signing messages outside the tree/recovery/group contexts
    Message {
        /// Domain separator for the message
        domain: String,
        /// Message payload
        payload: Vec<u8>,
    },

    /// OTA activation commitment
    ///
    /// Used when devices sign their commitment to a software upgrade activation.
    /// Each device signs its readiness commitment with its authority's keys.
    OTAActivation {
        /// Unique ceremony identifier
        ceremony_id: [u8; 32],
        /// Hash of the upgrade package being activated
        upgrade_hash: [u8; 32],
        /// Hash of the device's prestate at commitment time
        prestate_hash: [u8; 32],
        /// Epoch at which activation will occur
        activation_epoch: u64,
        /// Whether the device is ready for activation
        ready: bool,
    },
}

/// Group action types for group proposals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GroupAction {
    /// Add a new member to the group
    AddMember { member: AuthorityId },
    /// Remove a member from the group
    RemoveMember { member: AuthorityId },
    /// Change the group's threshold policy
    ChangePolicy { new_threshold: u16 },
    /// Execute a custom group action
    Custom { action_type: String, data: Vec<u8> },
}

/// Context for why a signature is being requested.
///
/// This is used for audit/logging and UI display, not for cryptographic purposes.
/// The same FROST signing code handles all contexts identically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApprovalContext {
    /// Regular operation on your own authority
    ///
    /// Multi-device signing where all participants are your own devices.
    SelfOperation,

    /// Recovery assistance for another authority
    ///
    /// You are acting as a guardian to help someone recover their account.
    RecoveryAssistance {
        /// Authority being recovered
        recovering: AuthorityId,
        /// Session identifier for this recovery attempt
        session_id: String,
    },

    /// Group decision
    ///
    /// You are participating in a group's threshold decision.
    GroupDecision {
        /// Group authority making the decision
        group: AuthorityId,
        /// Proposal being voted on
        proposal_id: String,
    },

    /// High-value operation requiring elevated approval
    ///
    /// Operations that require additional confirmation or mixed
    /// device + guardian approval.
    ElevatedOperation {
        /// Type of operation (for display)
        operation_type: String,
        /// Additional context about why this is elevated
        value_context: Option<String>,
    },
}

impl SigningContext {
    /// Create a context for a personal tree operation
    pub fn self_tree_op(authority: AuthorityId, op: TreeOp) -> Self {
        Self {
            authority,
            operation: SignableOperation::TreeOp(op),
            approval_context: ApprovalContext::SelfOperation,
        }
    }

    /// Create a context for a recovery approval
    pub fn recovery_approval(
        guardian_authority: AuthorityId,
        target: AuthorityId,
        new_root: TreeCommitment,
        session_id: String,
    ) -> Self {
        Self {
            authority: guardian_authority,
            operation: SignableOperation::RecoveryApproval { target, new_root },
            approval_context: ApprovalContext::RecoveryAssistance {
                recovering: target,
                session_id,
            },
        }
    }

    /// Create a context for a group decision
    pub fn group_decision(group: AuthorityId, action: GroupAction, proposal_id: String) -> Self {
        Self {
            authority: group,
            operation: SignableOperation::GroupProposal { group, action },
            approval_context: ApprovalContext::GroupDecision { group, proposal_id },
        }
    }

    /// Create a context for an OTA activation commitment
    ///
    /// Used when a device signs its commitment to a software upgrade activation.
    /// The signing uses the device's authority keys (may be 1-of-1 Ed25519 or FROST).
    pub fn ota_activation(
        authority: AuthorityId,
        ceremony_id: [u8; 32],
        upgrade_hash: [u8; 32],
        prestate_hash: [u8; 32],
        activation_epoch: u64,
        ready: bool,
    ) -> Self {
        Self {
            authority,
            operation: SignableOperation::OTAActivation {
                ceremony_id,
                upgrade_hash,
                prestate_hash,
                activation_epoch,
                ready,
            },
            // OTA activation is an elevated operation - it's a hard fork commitment
            approval_context: ApprovalContext::ElevatedOperation {
                operation_type: "ota_activation".to_string(),
                value_context: Some(format!(
                    "Hard fork activation at epoch {}",
                    activation_epoch
                )),
            },
        }
    }

    /// Create a context for an arbitrary message signing
    ///
    /// Used for signing messages outside the tree/recovery/group/OTA contexts.
    pub fn message(authority: AuthorityId, domain: String, payload: Vec<u8>) -> Self {
        Self {
            authority,
            operation: SignableOperation::Message { domain, payload },
            approval_context: ApprovalContext::SelfOperation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_authority() -> AuthorityId {
        AuthorityId::new_from_entropy([1u8; 32])
    }

    fn test_tree_op() -> TreeOp {
        TreeOp {
            parent_epoch: 0,
            parent_commitment: [0u8; 32],
            op: crate::tree::TreeOpKind::RotateEpoch { affected: vec![] },
            version: 1,
        }
    }

    #[test]
    fn test_self_tree_op_context() {
        let ctx = SigningContext::self_tree_op(test_authority(), test_tree_op());
        assert!(matches!(
            ctx.approval_context,
            ApprovalContext::SelfOperation
        ));
        assert!(matches!(ctx.operation, SignableOperation::TreeOp(_)));
    }

    #[test]
    fn test_recovery_approval_context() {
        let guardian = test_authority();
        let target = AuthorityId::new_from_entropy([2u8; 32]);
        let root = TreeCommitment([3u8; 32]);

        let ctx =
            SigningContext::recovery_approval(guardian, target, root, "session-123".to_string());

        assert!(matches!(
            ctx.approval_context,
            ApprovalContext::RecoveryAssistance { .. }
        ));
        assert!(matches!(
            ctx.operation,
            SignableOperation::RecoveryApproval { .. }
        ));
    }

    #[test]
    fn test_context_serialization() {
        let ctx = SigningContext::self_tree_op(test_authority(), test_tree_op());
        let json = serde_json::to_string(&ctx).unwrap();
        let restored: SigningContext = serde_json::from_str(&json).unwrap();
        assert!(matches!(
            restored.approval_context,
            ApprovalContext::SelfOperation
        ));
    }
}
