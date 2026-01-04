//! AttestedOp converter for fact-based journal
//!
//! This module provides conversion from tree operations to journal facts,
//! enabling the commitment tree to integrate with the fact-based journal model.

use crate::fact::{AttestedOp, Fact, FactContent, TreeOpKind};
use aura_core::{effects::RandomEffects, AuthorityId, Hash32, OrderTime, Result, TimeStamp};

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

impl TreeOp {
    /// Convert tree operation to a journal fact using the effect system for ID generation
    ///
    /// This method properly separates the deterministic conversion from ID generation,
    /// using RandomEffects to generate fact IDs that integrate with the effect system.
    /// This enables deterministic testing and proper effect boundaries.
    pub async fn to_fact(self, random: &dyn RandomEffects) -> Result<Fact> {
        let attested = AttestedOp {
            tree_op: self.kind,
            parent_commitment: self.parent,
            new_commitment: self.commitment,
            witness_threshold: self.witnesses.len() as u16,
            signature: self.aggregate_sig,
        };

        // Use the effect system to generate a proper random fact ID
        let id = OrderTime(random.random_bytes_32().await);
        let ts = TimeStamp::OrderClock(id.clone());

        Ok(Fact::new(
            id,
            ts,
            // Note: authority_id removed - facts are scoped by Journal namespace
            FactContent::AttestedOp(attested),
        ))
    }

    /// Convert tree operation to a journal fact with deterministic ID for testing
    ///
    /// This method creates a deterministic fact ID based on the commitment hash
    /// for use in tests or when deterministic IDs are required.
    pub fn to_fact_deterministic(self) -> Fact {
        let attested = AttestedOp {
            tree_op: self.kind,
            parent_commitment: self.parent,
            new_commitment: self.commitment,
            witness_threshold: self.witnesses.len() as u16,
            signature: self.aggregate_sig,
        };

        // Create deterministic ID based on commitment hash
        let fact_id_bytes = {
            let mut bytes = [0u8; 32];
            let commitment_bytes = self.commitment.as_bytes();
            let len = commitment_bytes.len().min(32);
            bytes[..len].copy_from_slice(&commitment_bytes[..len]);
            bytes
        };

        let id = OrderTime(fact_id_bytes);
        let ts = TimeStamp::OrderClock(id.clone());
        Fact::new(
            id,
            ts,
            // Note: authority_id removed - facts are scoped by Journal namespace
            FactContent::AttestedOp(attested),
        )
    }
}
