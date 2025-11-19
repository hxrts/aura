//! AttestedOp converter for fact-based journal
//!
//! This module provides conversion from tree operations to journal facts,
//! enabling the ratchet tree to integrate with the fact-based journal model.

use crate::fact::{AttestedOp, Fact, FactContent, FactId, TreeOpKind};
use aura_core::{
    tree::{AttestedOp as CoreAttestedOp, TreeHash32},
    AuthorityId, Hash32,
};
use serde::{Deserialize, Serialize};

/// Tree operation with context for conversion
pub struct TreeOp {
    /// Operation type
    pub kind: TreeOpKind,

    /// Parent commitment
    pub parent: Hash32,

    /// New commitment after operation
    pub commitment: Hash32,

    /// Witness signatures
    pub witnesses: Vec<Vec<u8>>,

    /// Aggregate signature
    pub aggregate_sig: Vec<u8>,

    /// Authority performing the operation
    pub authority_id: AuthorityId,
}

impl From<TreeOp> for Fact {
    fn from(op: TreeOp) -> Self {
        let attested = AttestedOp {
            tree_op: op.kind,
            parent_commitment: op.parent,
            new_commitment: op.commitment,
            witness_threshold: op.witnesses.len() as u16,
            signature: op.aggregate_sig,
        };

        Fact {
            fact_id: FactId::new(),
            authority_id: op.authority_id,
            content: FactContent::AttestedOp(attested),
        }
    }
}

/// Convert from core AttestedOp to fact-based AttestedOp
impl From<CoreAttestedOp> for AttestedOp {
    fn from(core_op: CoreAttestedOp) -> Self {
        // Map core operation types to our fact-based types
        let tree_op = match &core_op.operation {
            // TODO: Map specific core operations to TreeOpKind
            // For now, use a placeholder
            _ => TreeOpKind::RotateEpoch,
        };

        // Convert TreeHash32 to Hash32
        let parent_commitment = Hash32::new(core_op.parent_commitment);
        let new_commitment = Hash32::new([0; 32]); // TODO: Compute from operation

        AttestedOp {
            tree_op,
            parent_commitment,
            new_commitment,
            witness_threshold: 1,  // TODO: Get from operation context
            signature: Vec::new(), // TODO: Extract from core op
        }
    }
}
