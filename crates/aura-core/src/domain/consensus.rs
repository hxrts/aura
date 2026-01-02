//! Core consensus types for Aura
//!
//! This module provides foundational consensus types that are used across
//! multiple layers of the Aura architecture. These types capture the essential
//! mathematical properties of consensus without implementation details.

use crate::crypto::hash;
use crate::domain::content::Hash32;
use crate::types::identifiers::AuthorityId;
use crate::util::serialization;
use crate::AuraError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Maximum number of authorities that can participate in a single prestate.
pub const MAX_AUTHORITIES_PER_PRESTATE_COUNT: u32 = 256;

/// Prestate representing the combined state of authorities
///
/// The prestate captures the current state commitments of all
/// participating authorities plus the relational context state.
/// This is a foundational consensus concept that binds operations
/// to specific states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prestate {
    /// Commitments from each participating authority
    pub authority_commitments: BTreeMap<AuthorityId, Hash32>,
    /// Commitment of the relational context
    pub context_commitment: Hash32,
}

impl Prestate {
    /// Create a new prestate
    pub fn new(
        authority_commitments: Vec<(AuthorityId, Hash32)>,
        context_commitment: Hash32,
    ) -> Result<Self, PrestateValidationError> {
        let mut commitments = BTreeMap::new();
        for (id, commitment) in authority_commitments {
            if commitments.insert(id, commitment).is_some() {
                return Err(PrestateValidationError::DuplicateAuthority { authority: id });
            }
        }

        let prestate = Self {
            authority_commitments: commitments,
            context_commitment,
        };
        prestate.validate()?;
        Ok(prestate)
    }

    /// Compute the hash of this prestate
    ///
    /// The hash is computed deterministically by:
    /// 1. Sorting authority commitments by AuthorityId
    /// 2. Hashing the sorted data along with context commitment
    pub fn compute_hash(&self) -> Hash32 {
        let mut h = hash::hasher();
        h.update(b"AURA_PRESTATE_V1");

        // Hash number of authorities
        h.update(&(self.authority_commitments.len() as u32).to_le_bytes());

        // Hash each authority commitment
        for (id, commitment) in &self.authority_commitments {
            h.update(&id.to_bytes());
            h.update(&commitment.0);
        }

        // Hash context commitment
        h.update(&self.context_commitment.0);

        Hash32(h.finalize())
    }

    /// Check if this prestate includes a specific authority
    pub fn has_authority(&self, authority_id: &AuthorityId) -> bool {
        self.authority_commitments.contains_key(authority_id)
    }

    /// Get the commitment for a specific authority
    pub fn get_authority_commitment(&self, authority_id: &AuthorityId) -> Option<Hash32> {
        self.authority_commitments
            .get(authority_id)
            .copied()
    }

    /// Validate prestate invariants after deserialization.
    ///
    /// Returns `Ok(())` if the prestate is well-formed, or an error describing
    /// which invariant was violated. Call this after deserializing a prestate
    /// to ensure it meets structural requirements.
    pub fn validate(&self) -> std::result::Result<(), PrestateValidationError> {
        if self.authority_commitments.is_empty() {
            return Err(PrestateValidationError::NoAuthorities);
        }
        if (self.authority_commitments.len() as u32) > MAX_AUTHORITIES_PER_PRESTATE_COUNT {
            return Err(PrestateValidationError::TooManyAuthorities {
                count: self.authority_commitments.len() as u32,
                max: MAX_AUTHORITIES_PER_PRESTATE_COUNT,
            });
        }
        Ok(())
    }

    /// Create a binding for an operation
    ///
    /// This creates a unique identifier that binds an operation
    /// to this specific prestate.
    pub fn bind_operation<T: Serialize>(&self, operation: &T) -> Result<Hash32, AuraError> {
        let mut h = hash::hasher();
        h.update(b"AURA_OP_BINDING");

        // Include prestate hash
        let prestate_hash = self.compute_hash();
        h.update(&prestate_hash.0);

        // Include serialized operation
        let op_bytes =
            serialization::to_vec(operation).map_err(|e| AuraError::serialization(e.to_string()))?;
        h.update(&op_bytes);

        Ok(Hash32(h.finalize()))
    }
}

/// Builder for constructing prestates
pub struct PrestateBuilder {
    authority_commitments: BTreeMap<AuthorityId, Hash32>,
    context_commitment: Option<Hash32>,
}

/// Errors that can occur when building a Prestate.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PrestateBuilderError {
    #[error("Context commitment required")]
    MissingContext,
    #[error("At least one authority commitment required")]
    MissingAuthorities,
    #[error("Duplicate authority commitment for {0}")]
    DuplicateAuthority(AuthorityId),
    #[error("Too many authorities: {count} exceeds maximum {max}")]
    TooManyAuthorities { count: u32, max: u32 },
    #[error("Prestate validation failed: {0}")]
    InvalidPrestate(#[from] PrestateValidationError),
}

/// Errors that can occur during prestate validation.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PrestateValidationError {
    #[error("Prestate must have at least one authority")]
    NoAuthorities,
    #[error("Too many authorities: {count} exceeds maximum {max}")]
    TooManyAuthorities { count: u32, max: u32 },
    #[error("Duplicate authority commitment for {authority}")]
    DuplicateAuthority { authority: AuthorityId },
}

impl PrestateBuilder {
    /// Create a new builder
    #[must_use]
    pub fn new() -> Self {
        Self {
            authority_commitments: BTreeMap::new(),
            context_commitment: None,
        }
    }

    /// Add an authority commitment
    pub fn add_authority(
        mut self,
        id: AuthorityId,
        commitment: Hash32,
    ) -> Result<Self, PrestateBuilderError> {
        if self.authority_commitments.contains_key(&id) {
            return Err(PrestateBuilderError::DuplicateAuthority(id));
        }

        let next_count = self.authority_commitments.len() as u32 + 1;
        if next_count > MAX_AUTHORITIES_PER_PRESTATE_COUNT {
            return Err(PrestateBuilderError::TooManyAuthorities {
                count: next_count,
                max: MAX_AUTHORITIES_PER_PRESTATE_COUNT,
            });
        }

        self.authority_commitments.insert(id, commitment);
        Ok(self)
    }

    /// Set the context commitment
    #[must_use]
    pub fn context(mut self, commitment: Hash32) -> Self {
        self.context_commitment = Some(commitment);
        self
    }

    /// Build the prestate
    pub fn build(self) -> Result<Prestate, PrestateBuilderError> {
        let context_commitment = self
            .context_commitment
            .ok_or(PrestateBuilderError::MissingContext)?;

        if self.authority_commitments.is_empty() {
            return Err(PrestateBuilderError::MissingAuthorities);
        }

        Prestate::new(
            self.authority_commitments.into_iter().collect(),
            context_commitment,
        )
        .map_err(PrestateBuilderError::from)
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
        let auth1 = AuthorityId::new_from_entropy([21u8; 32]);
        let auth2 = AuthorityId::new_from_entropy([22u8; 32]);
        let commit1 = Hash32([1u8; 32]);
        let commit2 = Hash32([2u8; 32]);
        let context = Hash32([3u8; 32]);

        // Create two prestates with same data but different order
        let prestate1 =
            Prestate::new(vec![(auth1, commit1), (auth2, commit2)], context).unwrap();

        let prestate2 =
            Prestate::new(vec![(auth2, commit2), (auth1, commit1)], context).unwrap();

        // Hashes should be identical due to sorting
        assert_eq!(prestate1.compute_hash(), prestate2.compute_hash());
    }

    #[test]
    fn test_prestate_builder() {
        let auth = AuthorityId::new_from_entropy([23u8; 32]);
        let commit = Hash32::default();
        let context = Hash32([1u8; 32]);

        let prestate = PrestateBuilder::new()
            .add_authority(auth, commit)
            .unwrap()
            .context(context)
            .build()
            .unwrap();

        assert_eq!(prestate.authority_commitments.len(), 1);
        assert_eq!(prestate.context_commitment, context);
        assert!(prestate.has_authority(&auth));
    }

    #[test]
    fn test_bind_operation() {
        let auth = AuthorityId::new_from_entropy([24u8; 32]);
        let commit = Hash32::default();
        let context = Hash32([1u8; 32]);

        let prestate = Prestate::new(vec![(auth, commit)], context).unwrap();

        #[derive(Serialize)]
        struct TestOp {
            value: u32,
        }

        let op1 = TestOp { value: 42 };
        let op2 = TestOp { value: 43 };

        let binding1 = prestate.bind_operation(&op1).unwrap();
        let binding2 = prestate.bind_operation(&op2).unwrap();

        // Different operations should produce different bindings
        assert_ne!(binding1, binding2);
    }

    #[test]
    fn test_prestate_rejects_duplicate_authorities() {
        let auth = AuthorityId::new_from_entropy([25u8; 32]);
        let commit1 = Hash32([1u8; 32]);
        let commit2 = Hash32([2u8; 32]);
        let context = Hash32([3u8; 32]);

        let err = Prestate::new(vec![(auth, commit1), (auth, commit2)], context).unwrap_err();
        assert!(matches!(
            err,
            PrestateValidationError::DuplicateAuthority { authority: _ }
        ));
    }

    #[test]
    fn test_prestate_rejects_too_many_authorities() {
        let context = Hash32([9u8; 32]);
        let mut commitments = Vec::new();

        for i in 0..=MAX_AUTHORITIES_PER_PRESTATE_COUNT {
            let mut entropy = [0u8; 32];
            entropy[0] = (i & 0xFF) as u8;
            entropy[1] = (i >> 8) as u8;
            let id = AuthorityId::new_from_entropy(entropy);
            commitments.push((id, Hash32([entropy[0]; 32])));
        }

        let err = Prestate::new(commitments, context).unwrap_err();
        assert!(matches!(
            err,
            PrestateValidationError::TooManyAuthorities { .. }
        ));
    }

    #[test]
    fn test_bind_operation_serialization_error() {
        let auth = AuthorityId::new_from_entropy([26u8; 32]);
        let commit = Hash32::default();
        let context = Hash32([4u8; 32]);
        let prestate = Prestate::new(vec![(auth, commit)], context).unwrap();

        struct BadOp;
        impl Serialize for BadOp {
            fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                Err(serde::ser::Error::custom("boom"))
            }
        }

        let err = prestate.bind_operation(&BadOp).unwrap_err();
        assert!(err.to_string().contains("boom"));
    }
}
