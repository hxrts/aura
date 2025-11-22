//! RelationalContext implementation for cross-authority coordination
//!
//! This crate implements the RelationalContext abstraction that manages
//! relationships between authorities without exposing internal structure.

use aura_core::{
    identifiers::{AuthorityId, ContextId},
    Hash32, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub mod consensus_adapter;
pub mod guardian;
// Consensus implementation moved to aura-protocol
// Domain types moved to aura-core/relational/

// Re-export types from aura-core for API compatibility
pub use aura_core::relational::{
    ConsensusProof, GuardianBinding, GuardianParameters, RecoveryGrant, RecoveryOp,
    RelationalFact, GenericBinding,
};
// Re-export consensus functions from adapter
pub use consensus_adapter::{run_consensus, run_consensus_with_config, ConsensusConfig};
// Re-export Prestate from aura-core for compatibility
pub use aura_core::Prestate;

/// RelationalContext manages cross-authority relationships
///
/// Contexts provide a way for multiple authorities to coordinate
/// without revealing their internal device structure or identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationalContext {
    /// Unique identifier for this context
    pub context_id: ContextId,
    /// Authorities participating in this context
    pub participants: Vec<AuthorityId>,
    /// Journal storing relational facts
    pub journal: RelationalJournal,
}

impl RelationalContext {
    /// Create a new relational context
    pub fn new(participants: Vec<AuthorityId>) -> Self {
        let context_id = ContextId::new();
        let journal = RelationalJournal::new(context_id);

        Self {
            context_id,
            participants,
            journal,
        }
    }

    /// Create with a specific context ID
    pub fn with_id(context_id: ContextId, participants: Vec<AuthorityId>) -> Self {
        let journal = RelationalJournal::new(context_id);

        Self {
            context_id,
            participants,
            journal,
        }
    }

    /// Add a relational fact to the context
    pub fn add_fact(&mut self, fact: RelationalFact) -> Result<()> {
        self.journal.add_fact(fact)
    }

    /// Get all guardian bindings in this context
    pub fn guardian_bindings(&self) -> Vec<&GuardianBinding> {
        self.journal.guardian_bindings()
    }

    /// Get guardian binding for a specific authority
    pub fn get_guardian_binding(&self, authority_id: AuthorityId) -> Option<&GuardianBinding> {
        self.guardian_bindings().into_iter().find(|b| {
            // Compare authority IDs directly instead of converting to Hash32
            // since account_commitment should be derived from the authority ID
            b.account_commitment() == &Hash32::from_bytes(&authority_id.to_bytes())
        })
    }

    /// Get all recovery grants in this context
    pub fn recovery_grants(&self) -> Vec<&RecoveryGrant> {
        self.journal.recovery_grants()
    }

    /// Check if an authority is a participant
    pub fn has_participant(&self, authority_id: &AuthorityId) -> bool {
        self.participants.contains(authority_id)
    }

    /// Check if an authority is a participant (alias for has_participant)
    pub fn is_participant(&self, authority_id: &AuthorityId) -> bool {
        self.has_participant(authority_id)
    }

    /// Get all participants in this context
    pub fn get_participants(&self) -> &[AuthorityId] {
        &self.participants
    }

    /// Compute the current prestate for consensus
    pub fn compute_prestate(
        &self,
        authority_commitments: Vec<(AuthorityId, Hash32)>,
    ) -> aura_core::Prestate {
        aura_core::Prestate::new(authority_commitments, self.journal.compute_commitment())
    }
}

/// Journal specific to relational contexts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelationalJournal {
    /// Facts stored in this journal
    pub facts: BTreeSet<RelationalFact>,
    /// Context this journal belongs to
    context_id: ContextId,
}

impl RelationalJournal {
    /// Create a new relational journal
    pub fn new(context_id: ContextId) -> Self {
        Self {
            facts: BTreeSet::new(),
            context_id,
        }
    }

    /// Add a fact to the journal
    pub fn add_fact(&mut self, fact: RelationalFact) -> Result<()> {
        self.facts.insert(fact);
        Ok(())
    }

    /// Get all guardian bindings
    pub fn guardian_bindings(&self) -> Vec<&GuardianBinding> {
        self.facts
            .iter()
            .filter_map(|f| match f {
                RelationalFact::GuardianBinding(binding) => Some(binding),
                _ => None,
            })
            .collect()
    }

    /// Get all recovery grants
    pub fn recovery_grants(&self) -> Vec<&RecoveryGrant> {
        self.facts
            .iter()
            .filter_map(|f| match f {
                RelationalFact::RecoveryGrant(grant) => Some(grant),
                _ => None,
            })
            .collect()
    }

    /// Compute commitment hash of current state
    pub fn compute_commitment(&self) -> Hash32 {
        use aura_core::hash;
        let mut hasher = hash::hasher();

        hasher.update(b"RELATIONAL_JOURNAL");
        hasher.update(self.context_id.uuid().as_bytes());

        // Hash facts using canonical serialization for deterministic ordering
        for fact in &self.facts {
            // Use serde_json for deterministic serialization
            // In production, this could be replaced with DAG-CBOR for better efficiency
            if let Ok(fact_bytes) = serde_json::to_vec(fact) {
                hasher.update(&fact_bytes);
            } else {
                // Fallback to debug formatting if serialization fails
                // (should never happen since RelationalFact implements Serialize)
                hasher.update(format!("{:?}", fact).as_bytes());
            }
        }

        Hash32(hasher.finalize())
    }
}

// RelationalFact and GenericBinding moved to aura-core/src/relational/
// They are re-exported at the top of this file for API compatibility

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_relational_context_creation() {
        let auth1 = AuthorityId::new();
        let auth2 = AuthorityId::new();

        let context = RelationalContext::new(vec![auth1, auth2]);

        assert_eq!(context.participants.len(), 2);
        assert!(context.has_participant(&auth1));
        assert!(context.has_participant(&auth2));
    }

    #[test]
    fn test_add_guardian_binding() {
        let auth1 = AuthorityId::new();
        let auth2 = AuthorityId::new();

        let mut context = RelationalContext::new(vec![auth1, auth2]);

        // Hash the authority IDs to create 32-byte commitments
        let hash1 = aura_core::hash::hash(&auth1.to_bytes());
        let hash2 = aura_core::hash::hash(&auth2.to_bytes());

        let binding = GuardianBinding::new(
            Hash32::new(hash1),
            Hash32::new(hash2),
            GuardianParameters::default(),
        );

        context
            .add_fact(RelationalFact::GuardianBinding(binding))
            .unwrap();

        assert_eq!(context.guardian_bindings().len(), 1);
    }
}
