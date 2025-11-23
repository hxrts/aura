//! Relational facts for cross-authority coordination
//!
//! This module defines the facts that can be stored in relational contexts
//! to coordinate relationships between authorities.

use serde::{Deserialize, Serialize};

/// Facts that can be stored in relational contexts
///
/// These facts represent cross-authority relationships and operations
/// that require coordination between multiple authorities.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RelationalFact {
    /// Guardian binding between authorities
    GuardianBinding(super::guardian::GuardianBinding),
    /// Recovery grant approval
    RecoveryGrant(super::recovery::RecoveryGrant),
    /// Generic binding for extensibility
    Generic(GenericBinding),
}

/// Generic binding for application-specific relationships
///
/// This type allows for extensible relational facts without modifying
/// the core RelationalFact enum. Applications can define their own
/// binding schemas and store them as generic bindings.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GenericBinding {
    /// Type of binding (application-defined schema identifier)
    pub binding_type: String,
    /// Serialized binding data (application-defined format)
    pub binding_data: Vec<u8>,
    /// Optional consensus proof if binding required agreement
    pub consensus_proof: Option<super::consensus::ConsensusProof>,
}

impl GenericBinding {
    /// Create a new generic binding
    pub fn new(binding_type: String, binding_data: Vec<u8>) -> Self {
        Self {
            binding_type,
            binding_data,
            consensus_proof: None,
        }
    }

    /// Create a generic binding with consensus proof
    pub fn with_consensus_proof(
        binding_type: String,
        binding_data: Vec<u8>,
        consensus_proof: super::consensus::ConsensusProof,
    ) -> Self {
        Self {
            binding_type,
            binding_data,
            consensus_proof: Some(consensus_proof),
        }
    }

    /// Check if this binding has consensus proof
    pub fn has_consensus_proof(&self) -> bool {
        self.consensus_proof.is_some()
    }

    /// Get the binding type
    pub fn binding_type(&self) -> &str {
        &self.binding_type
    }

    /// Get the binding data
    pub fn binding_data(&self) -> &[u8] {
        &self.binding_data
    }

    /// Get the consensus proof, if any
    pub fn consensus_proof(&self) -> Option<&super::consensus::ConsensusProof> {
        self.consensus_proof.as_ref()
    }
}
