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
//! - Context-scoped fact journal mirror (uses `aura-journal` fact model)
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
//! - **Context Journal**: CRDT fact journal for relational state (`aura-journal` facts + `Generic` extensibility)
//! - **RelationalFact**: Guardian bindings, recovery grants, peer metadata
//! - **ConsensusAdapter**: Aura Consensus coordination for agreement
//!
//! RelationalContext implementation for cross-authority coordination
//!
//! This crate implements the RelationalContext abstraction that manages
//! relationships between authorities without exposing internal structure.

// RelationalContext uses std::sync::RwLock for synchronous interior mutability.
// All methods are synchronous and never cross .await boundaries, so blocking
// locks are safe here. See clippy.toml for blocking lock policy.
#![allow(clippy::disallowed_types)]

#[cfg(test)]
use aura_core::relational::GuardianParameters;
use aura_core::threshold::{ConvergenceCert, ReversionFact, RotateFact};
use aura_core::{
    hash::hash,
    identifiers::{AuthorityId, ContextId},
    relational::{GuardianBinding, RecoveryGrant},
    time::{OrderTime, TimeStamp},
    Hash32, Result,
};
use aura_journal::fact::{
    DkgTranscriptCommit, Fact, FactContent, Journal, JournalNamespace, RelationalFact,
};
use aura_journal::DomainFact;
use std::collections::BTreeSet;
use std::sync::RwLock;

pub mod consensus_adapter;
pub mod facts;
pub mod guardian;
pub mod guardian_request;
pub mod guardian_service;

/// Operation category map (A/B/C) for protocol gating and review.
pub const OPERATION_CATEGORIES: &[(&str, &str)] = &[
    ("relational:contact-add", "C"),
    ("relational:contact-remove", "C"),
    ("relational:guardian-bind", "C"),
    ("relational:recovery-grant", "C"),
];

/// Lookup the operation category (A/B/C) for a given operation.
pub fn operation_category(operation: &str) -> Option<&'static str> {
    OPERATION_CATEGORIES
        .iter()
        .find(|(op, _)| *op == operation)
        .map(|(_, category)| *category)
}

// Export domain fact types
pub use facts::{
    ContactFact, ContactFactReducer, GuardianBindingDetailsFact, GuardianBindingDetailsFactReducer,
    ReadReceiptPolicy, RecoveryGrantDetailsFact, RecoveryGrantDetailsFactReducer,
    CONTACT_FACT_TYPE_ID, GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID,
    RECOVERY_GRANT_DETAILS_FACT_TYPE_ID,
};

// Export consensus functions from adapter
pub use consensus_adapter::{run_consensus, run_consensus_with_config, ConsensusConfig};
pub use guardian_request::{
    parse_guardian_request, GuardianRequestFact, GuardianRequestFactReducer,
    GuardianRequestPayload, GUARDIAN_REQUEST_FACT_TYPE_ID,
};
pub use guardian_service::GuardianService;

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
    /// Context-scoped fact journal (append-only CRDT).
    ///
    /// This is an in-memory mirror of the journal CRDT structure; production
    /// runtimes persist typed facts via Layer 6 (`aura-agent`) using
    /// `AuraEffectSystem::{commit_relational_facts, commit_generic_fact_bytes}`.
    journal: RwLock<Journal>,
}

impl RelationalContext {
    /// Create a new relational context
    pub fn new(participants: Vec<AuthorityId>) -> Self {
        let mut seed = Vec::new();
        for participant in &participants {
            seed.extend_from_slice(&participant.to_bytes());
        }
        let context_id = ContextId::new_from_entropy(hash(&seed));

        Self {
            context_id,
            participants,
            journal: RwLock::new(Journal::new(JournalNamespace::Context(context_id))),
        }
    }

    /// Create with a specific context ID
    pub fn with_id(context_id: ContextId, participants: Vec<AuthorityId>) -> Self {
        Self {
            context_id,
            participants,
            journal: RwLock::new(Journal::new(JournalNamespace::Context(context_id))),
        }
    }

    /// Append a fact to the context.
    pub fn add_fact(&self, fact: RelationalFact) -> Result<()> {
        if let RelationalFact::Generic { context_id, .. } = &fact {
            if *context_id != self.context_id {
                return Err(aura_core::AuraError::invalid(
                    "Generic relational fact has mismatched context_id",
                ));
            }
        }

        let mut journal = self
            .journal
            .write()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire journal lock"))?;

        let order = Self::derive_order(&fact)?;
        let typed = Fact::new(
            order.clone(),
            TimeStamp::OrderClock(order),
            FactContent::Relational(fact),
        );

        journal.add_fact(typed)
    }

    /// Append a domain fact to the context (stored as `RelationalFact::Generic`).
    pub fn add_domain_fact<F: DomainFact>(&self, fact: &F) -> Result<()> {
        self.add_fact(fact.to_generic())
    }

    /// Append an arbitrary Generic relational fact scoped to this context.
    ///
    /// Note: Prefer using `add_domain_fact` for typed facts. This method is for
    /// cases where you have a pre-constructed envelope.
    pub fn add_generic_fact_envelope(
        &self,
        envelope: aura_core::types::facts::FactEnvelope,
    ) -> Result<()> {
        self.add_fact(RelationalFact::Generic {
            context_id: self.context_id,
            envelope,
        })
    }

    /// Record a guardian binding as:
    /// - a protocol-level binding receipt (authority IDs + hash), and
    /// - a domain-level detail fact (full `GuardianBinding` payload).
    pub fn add_guardian_binding(
        &self,
        account_id: AuthorityId,
        guardian_id: AuthorityId,
        binding: GuardianBinding,
    ) -> Result<Hash32> {
        let details = crate::facts::GuardianBindingDetailsFact::new(
            self.context_id,
            account_id,
            guardian_id,
            binding,
        );
        let details_bytes = details.to_bytes();
        let binding_hash = Hash32::from_bytes(&hash(&details_bytes));

        self.add_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::GuardianBinding {
                account_id,
                guardian_id,
                binding_hash,
            },
        ))?;
        self.add_domain_fact(&details)?;

        Ok(binding_hash)
    }

    /// Record a recovery grant as a domain-level detail fact (full `RecoveryGrant` payload).
    pub fn add_recovery_grant(
        &self,
        account_id: AuthorityId,
        grant: RecoveryGrant,
    ) -> Result<Hash32> {
        let details =
            crate::facts::RecoveryGrantDetailsFact::new(self.context_id, account_id, grant);
        let bytes = details.to_bytes();
        let grant_hash = Hash32::from_bytes(&hash(&bytes));
        self.add_domain_fact(&details)?;
        Ok(grant_hash)
    }

    /// Append a convergence certificate (A2 soft-safe).
    pub fn add_convergence_cert(&self, cert: ConvergenceCert) -> Result<()> {
        self.add_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::ConvergenceCert(cert),
        ))
    }

    /// Append a reversion fact for a soft-safe operation.
    pub fn add_reversion_fact(&self, fact: ReversionFact) -> Result<()> {
        self.add_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::ReversionFact(fact),
        ))
    }

    /// Append a lifecycle rotation fact.
    pub fn add_rotate_fact(&self, fact: RotateFact) -> Result<()> {
        self.add_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::RotateFact(fact),
        ))
    }

    /// Append a consensus-finalized DKG transcript commit.
    pub fn add_dkg_transcript_commit(&self, commit: DkgTranscriptCommit) -> Result<()> {
        self.add_fact(RelationalFact::Protocol(
            aura_journal::ProtocolRelationalFact::DkgTranscriptCommit(commit),
        ))
    }

    /// Get all guardian bindings in this context
    pub fn guardian_bindings(&self) -> Vec<GuardianBinding> {
        self.get_facts()
            .into_iter()
            .filter_map(|fact| match fact {
                RelationalFact::Generic { envelope, .. }
                    if envelope.type_id.as_str()
                        == crate::facts::GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID =>
                {
                    crate::facts::GuardianBindingDetailsFact::from_envelope(&envelope)
                        .map(|f| f.binding)
                }
                _ => None,
            })
            .collect()
    }

    /// Get guardian binding for a specific authority
    pub fn get_guardian_binding(&self, authority_id: AuthorityId) -> Option<GuardianBinding> {
        self.get_facts()
            .into_iter()
            .filter_map(|fact| match fact {
                RelationalFact::Generic { envelope, .. }
                    if envelope.type_id.as_str()
                        == crate::facts::GUARDIAN_BINDING_DETAILS_FACT_TYPE_ID =>
                {
                    crate::facts::GuardianBindingDetailsFact::from_envelope(&envelope)
                }
                _ => None,
            })
            .find(|f| f.guardian_id == authority_id || f.account_id == authority_id)
            .map(|f| f.binding)
    }

    /// Get all recovery grants in this context
    pub fn recovery_grants(&self) -> Vec<RecoveryGrant> {
        self.get_facts()
            .into_iter()
            .filter_map(|fact| match fact {
                RelationalFact::Generic { envelope, .. }
                    if envelope.type_id.as_str()
                        == crate::facts::RECOVERY_GRANT_DETAILS_FACT_TYPE_ID =>
                {
                    crate::facts::RecoveryGrantDetailsFact::from_envelope(&envelope)
                        .map(|f| f.grant)
                }
                _ => None,
            })
            .collect()
    }

    /// Deterministic key material for this context.
    ///
    /// This is **not a secret**. It is derived from the `ContextId` and the ordered
    /// participant set, and is intended for domain separation / stable identifiers.
    /// If confidentiality is required, use explicit key agreement and secret storage.
    pub fn context_key_material(&self) -> [u8; 32] {
        let mut material = Vec::with_capacity(32 + self.participants.len() * 32);
        material.extend_from_slice(&self.context_id.to_bytes());

        // Hash participant IDs in stable order to avoid permutation differences.
        let mut ids = self.participants.clone();
        ids.sort();
        for id in ids {
            material.extend_from_slice(&id.to_bytes());
        }

        aura_core::hash::hash(&material)
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
    ) -> Result<aura_core::Prestate> {
        let context_commitment = self.journal_commitment()?;
        let prestate = aura_core::Prestate::new(authority_commitments, context_commitment)
            .map_err(|e| aura_core::AuraError::invalid(e.to_string()))?;
        Ok(prestate)
    }

    /// Get all facts in this context for iteration
    pub fn get_facts(&self) -> BTreeSet<RelationalFact> {
        let Ok(journal) = self.journal.read() else {
            return BTreeSet::new();
        };

        journal
            .facts
            .iter()
            .filter_map(|f| match &f.content {
                FactContent::Relational(rf) => Some(rf.clone()),
                _ => None,
            })
            .collect()
    }

    /// Return the envelopes for all `RelationalFact::Generic` entries with a given type_id.
    ///
    /// This helper avoids leaking `aura-journal`'s fact enum into higher-layer crates that only
    /// need to parse their own domain payloads.
    pub fn generic_fact_envelopes(
        &self,
        type_id: &str,
    ) -> Vec<aura_core::types::facts::FactEnvelope> {
        let Ok(journal) = self.journal.read() else {
            return Vec::new();
        };

        journal
            .facts
            .iter()
            .filter_map(|f| match &f.content {
                FactContent::Relational(RelationalFact::Generic { envelope, .. })
                    if envelope.type_id.as_str() == type_id =>
                {
                    Some(envelope.clone())
                }
                _ => None,
            })
            .collect()
    }

    /// Compute the journal commitment hash
    pub fn journal_commitment(&self) -> Result<Hash32> {
        use aura_core::hash;
        let journal = self
            .journal
            .read()
            .map_err(|_| aura_core::AuraError::internal("Failed to acquire journal lock"))?;

        let mut hasher = hash::hasher();
        hasher.update(b"RELATIONAL_CONTEXT_FACTS");
        hasher.update(self.context_id.as_bytes());

        for fact in journal.facts.iter() {
            let bytes = aura_core::util::serialization::to_vec(fact)
                .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;
            hasher.update(&bytes);
        }

        Ok(Hash32(hasher.finalize()))
    }
}

impl RelationalContext {
    fn derive_order(fact: &RelationalFact) -> Result<OrderTime> {
        let bytes = aura_core::util::serialization::to_vec(fact)
            .map_err(|e| aura_core::AuraError::serialization(e.to_string()))?;
        Ok(OrderTime(hash(&bytes)))
    }
}

// Note: This crate stores context facts using `aura-journal`'s `RelationalFact`
// model (protocol-level variants + `Generic` extensibility).

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

        context.add_guardian_binding(auth1, auth2, binding).unwrap();

        assert_eq!(context.guardian_bindings().len(), 1);
    }
}
