//! Consensus domain facts for evidence and accountability
//!
//! This module defines domain-specific facts emitted by the consensus protocol,
//! including equivocation proofs and consensus results.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::time::PhysicalTime;
use aura_core::Hash32;
use aura_journal::extensibility::{DomainFact, FactEnvelope, FactReducer};
use aura_journal::reduction::{RelationalBinding, RelationalBindingType};
use serde::{Deserialize, Serialize};

/// Type ID for consensus facts
pub const CONSENSUS_FACT_TYPE_ID: &str = "consensus";

/// Schema version for consensus facts
pub const CONSENSUS_FACT_SCHEMA_VERSION: u16 = 1;

/// Domain facts emitted by consensus protocol
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsensusFact {
    /// Cryptographic proof of equivocation by a witness
    EquivocationProof(EquivocationProof),
}

/// Cryptographic evidence that a witness signed conflicting result IDs
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EquivocationProof {
    /// Context this consensus belongs to
    pub context_id: ContextId,
    /// Authority that equivocated
    pub witness: AuthorityId,
    /// Consensus instance identifier
    pub consensus_id: Hash32,
    /// Prestate hash for this consensus round
    pub prestate_hash: Hash32,
    /// First result ID witnessed
    pub first_result_id: Hash32,
    /// Second conflicting result ID witnessed
    pub second_result_id: Hash32,
    /// Timestamp when equivocation was detected
    pub timestamp: PhysicalTime,
}

impl EquivocationProof {
    /// Convert to evidence module's EquivocationProof for message propagation
    pub fn to_evidence_proof(&self) -> crate::evidence::EquivocationProof {
        crate::evidence::EquivocationProof {
            witness: self.witness,
            consensus_id: crate::ConsensusId(self.consensus_id),
            prestate_hash: self.prestate_hash,
            first_result_id: self.first_result_id,
            second_result_id: self.second_result_id,
            timestamp_ms: self.timestamp.ts_ms,
        }
    }
}

impl DomainFact for ConsensusFact {
    fn type_id(&self) -> &'static str {
        CONSENSUS_FACT_TYPE_ID
    }

    fn schema_version(&self) -> u16 {
        CONSENSUS_FACT_SCHEMA_VERSION
    }

    fn context_id(&self) -> ContextId {
        match self {
            ConsensusFact::EquivocationProof(proof) => proof.context_id,
        }
    }

    fn to_envelope(&self) -> FactEnvelope {
        // SAFETY: ConsensusFact serialization should be deterministic.
        // We use expect here because if serialization fails, it's a critical bug.
        #[allow(clippy::expect_used)]
        let payload = aura_core::util::serialization::to_vec(self)
            .expect("ConsensusFact serialization should not fail");

        FactEnvelope {
            type_id: aura_core::types::facts::FactTypeId::from(self.type_id()),
            schema_version: self.schema_version(),
            encoding: aura_core::types::facts::FactEncoding::DagCbor,
            payload,
        }
    }

    fn from_envelope(envelope: &FactEnvelope) -> Option<Self>
    where
        Self: Sized,
    {
        if envelope.type_id.as_str() != CONSENSUS_FACT_TYPE_ID {
            return None;
        }
        aura_core::util::serialization::from_slice(&envelope.payload).ok()
    }
}

/// Reducer for consensus facts
pub struct ConsensusFactReducer;

impl FactReducer for ConsensusFactReducer {
    fn handles_type(&self) -> &'static str {
        CONSENSUS_FACT_TYPE_ID
    }

    fn reduce_envelope(
        &self,
        context_id: ContextId,
        envelope: &FactEnvelope,
    ) -> Option<RelationalBinding> {
        if envelope.type_id.as_str() != CONSENSUS_FACT_TYPE_ID {
            return None;
        }

        let fact = ConsensusFact::from_envelope(envelope)?;

        match fact {
            ConsensusFact::EquivocationProof(proof) => {
                // Equivocation proofs are accountability records
                // Store as generic binding for audit trail
                // Key structure: witness + consensus_id + both result IDs
                let mut key_data = Vec::new();
                key_data.extend_from_slice(&proof.witness.to_bytes());
                key_data.extend_from_slice(proof.consensus_id.as_bytes());
                key_data.extend_from_slice(proof.first_result_id.as_bytes());
                key_data.extend_from_slice(proof.second_result_id.as_bytes());

                Some(RelationalBinding {
                    binding_type: RelationalBindingType::Generic("equivocation-proof".to_string()),
                    context_id,
                    data: key_data,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::time::PhysicalTime;

    #[test]
    fn equivocation_proof_envelope_roundtrip() {
        let proof = EquivocationProof {
            context_id: ContextId::new_from_entropy([1u8; 32]),
            witness: AuthorityId::new_from_entropy([2u8; 32]),
            consensus_id: Hash32::new([3u8; 32]),
            prestate_hash: Hash32::new([4u8; 32]),
            first_result_id: Hash32::new([5u8; 32]),
            second_result_id: Hash32::new([6u8; 32]),
            timestamp: PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            },
        };

        let fact = ConsensusFact::EquivocationProof(proof);
        let envelope = fact.to_envelope();

        assert_eq!(envelope.type_id.as_str(), CONSENSUS_FACT_TYPE_ID);
        assert_eq!(envelope.schema_version, CONSENSUS_FACT_SCHEMA_VERSION);

        let restored = ConsensusFact::from_envelope(&envelope);
        assert_eq!(restored, Some(fact));
    }

    #[test]
    fn consensus_fact_to_generic() {
        let proof = EquivocationProof {
            context_id: ContextId::new_from_entropy([1u8; 32]),
            witness: AuthorityId::new_from_entropy([2u8; 32]),
            consensus_id: Hash32::new([3u8; 32]),
            prestate_hash: Hash32::new([4u8; 32]),
            first_result_id: Hash32::new([5u8; 32]),
            second_result_id: Hash32::new([6u8; 32]),
            timestamp: PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            },
        };

        let fact = ConsensusFact::EquivocationProof(proof);
        let generic = fact.to_generic();

        if let aura_journal::fact::RelationalFact::Generic { envelope, .. } = generic {
            assert_eq!(envelope.type_id.as_str(), CONSENSUS_FACT_TYPE_ID);
            let restored = ConsensusFact::from_envelope(&envelope);
            assert!(restored.is_some());
        } else {
            panic!("Expected Generic variant");
        }
    }

    #[test]
    fn consensus_fact_reducer_handles_equivocation_proof() {
        let proof = EquivocationProof {
            context_id: ContextId::new_from_entropy([1u8; 32]),
            witness: AuthorityId::new_from_entropy([2u8; 32]),
            consensus_id: Hash32::new([3u8; 32]),
            prestate_hash: Hash32::new([4u8; 32]),
            first_result_id: Hash32::new([5u8; 32]),
            second_result_id: Hash32::new([6u8; 32]),
            timestamp: PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            },
        };

        let fact = ConsensusFact::EquivocationProof(proof.clone());
        let envelope = fact.to_envelope();

        let reducer = ConsensusFactReducer;
        let binding = reducer.reduce_envelope(proof.context_id, &envelope);

        assert!(binding.is_some());
        let binding = binding.unwrap();
        assert_eq!(
            binding.binding_type,
            RelationalBindingType::Generic("equivocation-proof".to_string())
        );
        assert_eq!(binding.context_id, proof.context_id);

        // Verify key structure contains witness + consensus_id + both result IDs
        // AuthorityId (16 bytes) + 3 × Hash32 (96 bytes) = 112 bytes
        assert_eq!(binding.data.len(), 16 + 32 + 32 + 32);
    }
}
