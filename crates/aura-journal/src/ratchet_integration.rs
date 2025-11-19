//! Integration between ratchet tree operations and fact-based journal
//!
//! This module bridges the existing ratchet tree implementation with
//! the new fact-based journal model.

use crate::{
    fact::{AttestedOp, Fact, FactContent, FactId},
    ratchet_tree::{AttestedOp as CoreAttestedOp, TreeOpKind},
};
use aura_core::Hash32;

/// Convert a core AttestedOp to a fact-based AttestedOp
impl From<CoreAttestedOp> for AttestedOp {
    fn from(op: CoreAttestedOp) -> Self {
        AttestedOp {
            tree_op: crate::fact::TreeOpKind::from(op.op),
            parent_commitment: op.parent_commitment,
            new_commitment: op.new_commitment,
            witness_threshold: op.witness_threshold,
            signature: op.signature.to_vec(),
        }
    }
}

/// Convert TreeOpKind to fact-based TreeOpKind
impl From<TreeOpKind> for crate::fact::TreeOpKind {
    fn from(op: TreeOpKind) -> Self {
        match op {
            TreeOpKind::AddDevice { .. } => crate::fact::TreeOpKind::AddDevice,
            TreeOpKind::RemoveDevice { .. } => crate::fact::TreeOpKind::RemoveDevice,
            TreeOpKind::RotateKey { .. } => crate::fact::TreeOpKind::RotateKey,
            TreeOpKind::UpdatePolicy { .. } => crate::fact::TreeOpKind::UpdatePolicy,
        }
    }
}

/// Convert a tree operation to a fact
impl From<CoreAttestedOp> for Fact {
    fn from(op: CoreAttestedOp) -> Self {
        let attested = AttestedOp::from(op);

        Fact {
            fact_id: FactId::new(),
            content: FactContent::AttestedOp(attested),
        }
    }
}

/// Helper trait for converting tree operations to facts
pub trait ToFact {
    /// Convert this operation to a fact
    fn to_fact(self) -> Fact;
}

impl ToFact for CoreAttestedOp {
    fn to_fact(self) -> Fact {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_attested_op_conversion() {
        // Create a dummy CoreAttestedOp
        let core_op = CoreAttestedOp {
            op: TreeOpKind::AddDevice {
                device_id: aura_core::DeviceId::new(),
                public_key: vec![0u8; 32],
            },
            parent_commitment: Hash32::default(),
            new_commitment: Hash32::default(),
            witness_threshold: 2,
            signature: vec![0u8; 64],
        };

        // Convert to fact-based AttestedOp
        let fact_op = AttestedOp::from(core_op.clone());
        assert_eq!(fact_op.parent_commitment, core_op.parent_commitment);
        assert_eq!(fact_op.new_commitment, core_op.new_commitment);
        assert_eq!(fact_op.witness_threshold, core_op.witness_threshold);
    }

    #[test]
    fn test_tree_op_to_fact() {
        let core_op = CoreAttestedOp {
            op: TreeOpKind::RemoveDevice {
                device_id: aura_core::DeviceId::new(),
            },
            parent_commitment: Hash32::default(),
            new_commitment: Hash32::default(),
            witness_threshold: 3,
            signature: vec![0u8; 64],
        };

        let fact = core_op.to_fact();
        match fact.content {
            FactContent::AttestedOp(op) => {
                assert_eq!(op.tree_op, crate::fact::TreeOpKind::RemoveDevice);
            }
            _ => panic!("Expected AttestedOp fact"),
        }
    }
}
