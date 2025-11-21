//! AttestedOp converter for fact-based journal
//!
//! This module provides conversion from tree operations to journal facts,
//! enabling the commitment tree to integrate with the fact-based journal model.

use crate::fact::{AttestedOp, Fact, FactContent, FactId, TreeOpKind};
use aura_core::{
    AuthorityId, Hash32,
};

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
            // Note: authority_id removed - facts are scoped by Journal namespace
            content: FactContent::AttestedOp(attested),
        }
    }
}

// NOTE: From<CoreAttestedOp> for AttestedOp implementation moved to commitment_integration.rs
// to avoid duplicate trait implementations. See commitment_integration.rs for the canonical conversion.
