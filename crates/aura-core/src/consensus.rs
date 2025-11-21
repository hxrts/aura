//! Core consensus types for Aura
//!
//! This module provides foundational consensus types that are used across
//! multiple layers of the Aura architecture. These types capture the essential
//! mathematical properties of consensus without implementation details.

use crate::content::Hash32;
use crate::hash;
use crate::identifiers::AuthorityId;
use serde::{Deserialize, Serialize};

/// Prestate representing the combined state of authorities
///
/// The prestate captures the current state commitments of all
/// participating authorities plus the relational context state.
/// This is a foundational consensus concept that binds operations
/// to specific states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prestate {
    /// Commitments from each participating authority
    pub authority_commitments: Vec<(AuthorityId, Hash32)>,
    /// Commitment of the relational context
    pub context_commitment: Hash32,
}

impl Prestate {
    /// Create a new prestate
    pub fn new(
        authority_commitments: Vec<(AuthorityId, Hash32)>,
        context_commitment: Hash32,
    ) -> Self {
        Self {
            authority_commitments,
            context_commitment,
        }
    }

    /// Compute the hash of this prestate
    ///
    /// The hash is computed deterministically by:
    /// 1. Sorting authority commitments by AuthorityId
    /// 2. Hashing the sorted data along with context commitment
    pub fn compute_hash(&self) -> Hash32 {
        let mut h = hash::hasher();
        h.update(b"AURA_PRESTATE_V1");

        // Sort for determinism
        let mut sorted = self.authority_commitments.clone();
        sorted.sort_by_key(|(id, _)| *id);

        // Hash number of authorities
        h.update(&(sorted.len() as u32).to_le_bytes());

        // Hash each authority commitment
        for (id, commitment) in sorted {
            h.update(&id.to_bytes());
            h.update(&commitment.0);
        }

        // Hash context commitment
        h.update(&self.context_commitment.0);

        Hash32(h.finalize())
    }

    /// Check if this prestate includes a specific authority
    pub fn has_authority(&self, authority_id: &AuthorityId) -> bool {
        self.authority_commitments
            .iter()
            .any(|(id, _)| id == authority_id)
    }

    /// Get the commitment for a specific authority
    pub fn get_authority_commitment(&self, authority_id: &AuthorityId) -> Option<Hash32> {
        self.authority_commitments
            .iter()
            .find(|(id, _)| id == authority_id)
            .map(|(_, commitment)| *commitment)
    }

    /// Create a binding for an operation
    ///
    /// This creates a unique identifier that binds an operation
    /// to this specific prestate.
    pub fn bind_operation<T: Serialize>(&self, operation: &T) -> Hash32 {
        let mut h = hash::hasher();
        h.update(b"AURA_OP_BINDING");

        // Include prestate hash
        let prestate_hash = self.compute_hash();
        h.update(&prestate_hash.0);

        // Include serialized operation
        if let Ok(op_bytes) = serde_json::to_vec(operation) {
            h.update(&op_bytes);
        }

        Hash32(h.finalize())
    }
}

/// Builder for constructing prestates
pub struct PrestateBuilder {
    authority_commitments: Vec<(AuthorityId, Hash32)>,
    context_commitment: Option<Hash32>,
}

impl PrestateBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            authority_commitments: Vec::new(),
            context_commitment: None,
        }
    }

    /// Add an authority commitment
    pub fn add_authority(mut self, id: AuthorityId, commitment: Hash32) -> Self {
        self.authority_commitments.push((id, commitment));
        self
    }

    /// Set the context commitment
    pub fn context(mut self, commitment: Hash32) -> Self {
        self.context_commitment = Some(commitment);
        self
    }

    /// Build the prestate
    pub fn build(self) -> Result<Prestate, &'static str> {
        let context_commitment = self
            .context_commitment
            .ok_or("Context commitment required")?;

        if self.authority_commitments.is_empty() {
            return Err("At least one authority commitment required");
        }

        Ok(Prestate::new(
            self.authority_commitments,
            context_commitment,
        ))
    }
}

impl Default for PrestateBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prestate_hash_deterministic() {
        let auth1 = AuthorityId::new();
        let auth2 = AuthorityId::new();
        let commit1 = Hash32([1u8; 32]);
        let commit2 = Hash32([2u8; 32]);
        let context = Hash32([3u8; 32]);

        // Create two prestates with same data but different order
        let prestate1 = Prestate::new(vec![(auth1, commit1), (auth2, commit2)], context);

        let prestate2 = Prestate::new(vec![(auth2, commit2), (auth1, commit1)], context);

        // Hashes should be identical due to sorting
        assert_eq!(prestate1.compute_hash(), prestate2.compute_hash());
    }

    #[test]
    fn test_prestate_builder() {
        let auth = AuthorityId::new();
        let commit = Hash32::default();
        let context = Hash32([1u8; 32]);

        let prestate = PrestateBuilder::new()
            .add_authority(auth, commit)
            .context(context)
            .build()
            .unwrap();

        assert_eq!(prestate.authority_commitments.len(), 1);
        assert_eq!(prestate.context_commitment, context);
        assert!(prestate.has_authority(&auth));
    }

    #[test]
    fn test_bind_operation() {
        let auth = AuthorityId::new();
        let commit = Hash32::default();
        let context = Hash32([1u8; 32]);

        let prestate = Prestate::new(vec![(auth, commit)], context);

        #[derive(Serialize)]
        struct TestOp {
            value: u32,
        }

        let op1 = TestOp { value: 42 };
        let op2 = TestOp { value: 43 };

        let binding1 = prestate.bind_operation(&op1);
        let binding2 = prestate.bind_operation(&op2);

        // Different operations should produce different bindings
        assert_ne!(binding1, binding2);
    }
}
