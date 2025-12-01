//! # Aura Relational - Layer 5: Feature/Protocol Implementation
//!
//! This crate implements relational contexts for cross-authority coordination
//! and management of guardian bindings, recovery grants, and peer relationships
//! in the Aura threshold identity platform.
//!
//! ## Purpose
//!
//! Layer 5 feature crate providing relational context management for:
//! - Cross-authority relationship coordination without exposing internal structure
//! - Guardian binding management and lifecycle
//! - Recovery grant storage and consensus coordination
//! - Relational fact journals for distributed state agreement
//! - Consensus coordination through Aura Consensus protocol
//!
//! ## Architecture Constraints
//!
//! This crate depends on:
//! - **Layer 1** (aura-core): Core types, effects, errors
//! - **Layer 2** (aura-journal): CRDT semantics and fact storage
//! - **Layer 3** (aura-effects): Effect handler implementations
//! - **Layer 4** (aura-protocol): Orchestration and Aura Consensus
//!
//! ## What Belongs Here
//!
//! - RelationalContext abstraction and lifecycle management
//! - RelationalJournal for fact-based state in relational contexts
//! - Guardian binding management and query operations
//! - Recovery grant management and retrieval
//! - Consensus adapter for running Aura Consensus on relational state
//! - Context-scoped metadata and participant tracking
//!
//! ## What Does NOT Belong Here
//!
//! - Effect handler implementations (belong in aura-effects)
//! - Low-level consensus algorithms (belong in aura-protocol)
//! - Guardian protocol coordination (belong in aura-recovery)
//! - Recovery protocol coordination (belong in aura-recovery)
//! - Invitation choreographies (belong in aura-invitation)
//!
//! ## Design Principles
//!
//! - Relational contexts are stateless coordinators for distributed agreement
//! - All state is fact-based and stored in journals; no mutable local state
//! - Participant list is explicit and immutable per context
//! - Facts form a semilattice: adding facts is monotonic, idempotent
//! - Consensus ensures multi-authority agreement on fact sets
//! - Contexts provide metadata privacy: no exposure of internal device structure
//!
//! ## Key Components
//!
//! - **RelationalContext**: Multi-authority coordination unit
//! - **RelationalJournal**: CRDT-based fact storage for relational state
//! - **RelationalFact**: Guardian bindings, recovery grants, peer metadata
//! - **ConsensusAdapter**: Aura Consensus coordination for agreement
//!
//! RelationalContext implementation for cross-authority coordination
//!
//! This crate implements the RelationalContext abstraction that manages
//! relationships between authorities without exposing internal structure.

#[cfg(test)]
use aura_core::relational::GuardianParameters;
use aura_core::{
    hash::hash,
    identifiers::{AuthorityId, ContextId},
    relational::{GuardianBinding, RecoveryGrant, RelationalFact},
    Hash32, Result,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::Mutex;

pub mod consensus_adapter;
pub mod guardian;

// Export consensus functions from adapter
pub use consensus_adapter::{run_consensus, run_consensus_with_config, ConsensusConfig};

/// RelationalContext manages cross-authority relationships
///
/// Contexts provide a way for multiple authorities to coordinate
/// without revealing their internal device structure or identity.
#[derive(Debug)]
pub struct RelationalContext {
    /// Unique identifier for this context
    pub context_id: ContextId,
    /// Authorities participating in this context
    pub participants: Vec<AuthorityId>,
    /// Journal storing relational facts (wrapped in Mutex for thread-safe mutation)
    journal: Mutex<RelationalJournal>,
}

impl RelationalContext {
    /// Create a new relational context
    pub fn new(participants: Vec<AuthorityId>) -> Self {
        let mut seed = Vec::new();
        for participant in &participants {
            seed.extend_from_slice(&participant.to_bytes());
        }
        let context_id = ContextId::new_from_entropy(hash(&seed));
        let journal = RelationalJournal::new(context_id);

        Self {
            context_id,
            participants,
            journal: Mutex::new(journal),
        }
    }

    /// Create with a specific context ID
    pub fn with_id(context_id: ContextId, participants: Vec<AuthorityId>) -> Self {
        let journal = RelationalJournal::new(context_id);

        Self {
            context_id,
            participants,
            journal: Mutex::new(journal),
        }
    }

    /// Add a relational fact to the context
    pub fn add_fact(&self, fact: RelationalFact) -> Result<()> {
        let mut journal = self
            .journal
            .lock()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire journal lock"))?;
        journal.add_fact(fact)
    }

    /// Get all guardian bindings in this context
    pub fn guardian_bindings(&self) -> Vec<GuardianBinding> {
        if let Ok(journal) = self.journal.lock() {
            journal.guardian_bindings().into_iter().cloned().collect()
        } else {
            vec![]
        }
    }

    /// Get guardian binding for a specific authority
    pub fn get_guardian_binding(&self, authority_id: AuthorityId) -> Option<GuardianBinding> {
        self.guardian_bindings().into_iter().find(|b| {
            // Compare authority IDs directly instead of converting to Hash32
            // since account_commitment should be derived from the authority ID
            b.account_commitment == Hash32::from_bytes(&authority_id.to_bytes())
        })
    }

    /// Get all recovery grants in this context
    pub fn recovery_grants(&self) -> Vec<RecoveryGrant> {
        if let Ok(journal) = self.journal.lock() {
            journal.recovery_grants().into_iter().cloned().collect()
        } else {
            vec![]
        }
    }

    /// Get shared secret for this context
    /// Returns the deterministic shared secret used for context-specific operations
    pub fn shared_secret(&self) -> Option<[u8; 32]> {
        // Derive a context secret from the context ID and all participant IDs.
        // This remains deterministic (for reproducibility/tests) while tying the
        // secret to the exact participant set.
        let mut material = Vec::with_capacity(32 + self.participants.len() * 16);
        material.extend_from_slice(&self.context_id.to_bytes());

        // Hash participant IDs in stable order to avoid permutation differences.
        let mut ids = self.participants.clone();
        ids.sort();
        for id in ids {
            material.extend_from_slice(&id.to_bytes());
        }

        Some(aura_core::hash::hash(&material))
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
        let journal_commitment = if let Ok(journal) = self.journal.lock() {
            journal.compute_commitment()
        } else {
            Hash32([0u8; 32]) // fallback to zero hash if lock fails
        };
        aura_core::Prestate::new(authority_commitments, journal_commitment)
    }

    /// Get all facts in this context for iteration
    pub fn get_facts(&self) -> BTreeSet<RelationalFact> {
        if let Ok(journal) = self.journal.lock() {
            journal.facts.clone()
        } else {
            BTreeSet::new()
        }
    }

    /// Compute the journal commitment hash
    pub fn journal_commitment(&self) -> Hash32 {
        if let Ok(journal) = self.journal.lock() {
            journal.compute_commitment()
        } else {
            Hash32([0u8; 32]) // fallback to zero hash if lock fails
        }
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
        let auth1 = AuthorityId::new_from_entropy([60u8; 32]);
        let auth2 = AuthorityId::new_from_entropy([61u8; 32]);

        let context = RelationalContext::new(vec![auth1, auth2]);

        assert_eq!(context.participants.len(), 2);
        assert!(context.has_participant(&auth1));
        assert!(context.has_participant(&auth2));
    }

    #[test]
    fn test_add_guardian_binding() {
        let auth1 = AuthorityId::new_from_entropy([62u8; 32]);
        let auth2 = AuthorityId::new_from_entropy([63u8; 32]);

        let context = RelationalContext::new(vec![auth1, auth2]);

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
