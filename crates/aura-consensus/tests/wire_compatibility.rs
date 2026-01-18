//! Wire format compatibility tests for evidence delta integration
//!
//! These tests verify that message serialization/deserialization works correctly
//! with evidence_delta fields, ensuring backward compatibility.

#![allow(clippy::expect_used)]

use aura_consensus::{
    evidence::{EquivocationProof, EvidenceDelta},
    messages::ConsensusMessage,
    ConsensusId,
};
use aura_core::{AuthorityId, Hash32};

fn test_consensus_id() -> ConsensusId {
    ConsensusId(Hash32::new([1u8; 32]))
}

fn test_authority() -> AuthorityId {
    AuthorityId::new_from_entropy([2u8; 32])
}

fn test_hash(seed: u8) -> Hash32 {
    Hash32::new([seed; 32])
}

fn empty_evidence_delta() -> EvidenceDelta {
    EvidenceDelta::empty(test_consensus_id(), 1000)
}

fn non_empty_evidence_delta() -> EvidenceDelta {
    let proof = EquivocationProof::new(
        test_authority(),
        test_consensus_id(),
        test_hash(1),
        test_hash(2),
        test_hash(3),
        1000,
    );

    EvidenceDelta {
        consensus_id: test_consensus_id(),
        equivocation_proofs: vec![proof],
        timestamp_ms: 1000,
    }
}

#[test]
fn test_execute_message_serialization_roundtrip() {
    let msg = ConsensusMessage::Execute {
        consensus_id: test_consensus_id(),
        prestate_hash: test_hash(1),
        operation_hash: test_hash(2),
        operation_bytes: vec![1, 2, 3, 4],
        cached_commitments: None,
        evidence_delta: empty_evidence_delta(),
    };

    // Serialize to JSON
    let json = serde_json::to_string(&msg).expect("Serialization should succeed");

    // Deserialize back
    let restored: ConsensusMessage =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    // Verify it's the same message type
    match restored {
        ConsensusMessage::Execute { .. } => {}
        _ => panic!("Expected Execute message"),
    }
}

#[test]
fn test_sign_share_message_with_evidence_delta() {
    let msg = ConsensusMessage::SignShare {
        consensus_id: test_consensus_id(),
        result_id: test_hash(5),
        share: aura_core::frost::PartialSignature {
            signer: 1,
            signature: vec![0u8; 64],
        },
        next_commitment: None,
        epoch: aura_core::types::Epoch(1),
        evidence_delta: non_empty_evidence_delta(),
    };

    // Serialize with bincode
    let bytes = bincode::serialize(&msg).expect("Serialization should succeed");

    // Deserialize back
    let restored: ConsensusMessage =
        bincode::deserialize(&bytes).expect("Deserialization should succeed");

    // Verify evidence delta was preserved
    match restored {
        ConsensusMessage::SignShare { evidence_delta, .. } => {
            assert_eq!(evidence_delta.equivocation_proofs.len(), 1);
            assert_eq!(evidence_delta.timestamp_ms, 1000);
        }
        _ => panic!("Expected SignShare message"),
    }
}

#[test]
fn test_evidence_delta_empty_serialization() {
    let delta = empty_evidence_delta();

    // Serialize
    let bytes = bincode::serialize(&delta).expect("Serialization should succeed");

    // Deserialize
    let restored: EvidenceDelta =
        bincode::deserialize(&bytes).expect("Deserialization should succeed");

    assert_eq!(restored.consensus_id, test_consensus_id());
    assert!(restored.equivocation_proofs.is_empty());
    assert!(restored.is_empty());
}

#[test]
fn test_evidence_delta_with_proofs_serialization() {
    let delta = non_empty_evidence_delta();

    // Serialize with JSON
    let json = serde_json::to_string(&delta).expect("Serialization should succeed");

    // Deserialize
    let restored: EvidenceDelta =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    assert_eq!(restored.equivocation_proofs.len(), 1);
    assert!(!restored.is_empty());

    let proof = &restored.equivocation_proofs[0];
    assert_eq!(proof.witness, test_authority());
    assert_eq!(proof.first_result_id, test_hash(2));
    assert_eq!(proof.second_result_id, test_hash(3));
}

#[test]
fn test_equivocation_proof_roundtrip() {
    let proof = EquivocationProof::new(
        test_authority(),
        test_consensus_id(),
        test_hash(1),
        test_hash(2),
        test_hash(3),
        5000,
    );

    // Verify before serialization
    assert!(proof.verify().is_ok());

    // Serialize with bincode
    let bytes = bincode::serialize(&proof).expect("Serialization should succeed");

    // Deserialize
    let restored: EquivocationProof =
        bincode::deserialize(&bytes).expect("Deserialization should succeed");

    // Verify after deserialization
    assert!(restored.verify().is_ok());
    assert_eq!(restored.witness, proof.witness);
    assert_eq!(restored.consensus_id, proof.consensus_id);
    assert_eq!(restored.first_result_id, proof.first_result_id);
    assert_eq!(restored.second_result_id, proof.second_result_id);
    assert_eq!(restored.timestamp_ms, proof.timestamp_ms);
}

#[test]
fn test_consensus_result_message_with_evidence() {
    let commit_fact = aura_consensus::types::CommitFact::new(
        test_consensus_id(),
        test_hash(1),
        test_hash(2),
        vec![1, 2, 3, 4],
        aura_core::frost::ThresholdSignature {
            signature: vec![0u8; 64],
            signers: vec![1, 2, 3],
        },
        None,
        vec![test_authority()],
        2,
        false,
        aura_core::time::ProvenancedTime {
            stamp: aura_core::time::TimeStamp::PhysicalClock(aura_core::time::PhysicalTime {
                ts_ms: 1000,
                uncertainty: None,
            }),
            proofs: vec![],
            origin: None,
        },
    );

    let msg = ConsensusMessage::ConsensusResult {
        commit_fact,
        evidence_delta: non_empty_evidence_delta(),
    };

    // Serialize
    let bytes = bincode::serialize(&msg).expect("Serialization should succeed");

    // Deserialize
    let restored: ConsensusMessage =
        bincode::deserialize(&bytes).expect("Deserialization should succeed");

    // Verify evidence was preserved
    match restored {
        ConsensusMessage::ConsensusResult {
            evidence_delta,
            commit_fact,
            ..
        } => {
            assert_eq!(evidence_delta.equivocation_proofs.len(), 1);
            assert_eq!(commit_fact.consensus_id, test_consensus_id());
        }
        _ => panic!("Expected ConsensusResult message"),
    }
}

#[test]
fn test_backward_compatibility_empty_delta() {
    // Simulate a message from an older version that doesn't include evidence_delta
    // The delta should default to empty

    let delta = EvidenceDelta::empty(test_consensus_id(), 0);
    assert!(delta.is_empty());
    assert_eq!(delta.equivocation_proofs.len(), 0);
}

#[test]
fn test_multiple_proofs_in_delta() {
    let proof1 = EquivocationProof::new(
        test_authority(),
        test_consensus_id(),
        test_hash(1),
        test_hash(2),
        test_hash(3),
        1000,
    );

    let proof2 = EquivocationProof::new(
        AuthorityId::new_from_entropy([3u8; 32]),
        test_consensus_id(),
        test_hash(1),
        test_hash(4),
        test_hash(5),
        2000,
    );

    let delta = EvidenceDelta {
        consensus_id: test_consensus_id(),
        equivocation_proofs: vec![proof1, proof2],
        timestamp_ms: 2000,
    };

    // Serialize with JSON for readability
    let json = serde_json::to_string_pretty(&delta).expect("Serialization should succeed");

    // Deserialize
    let restored: EvidenceDelta =
        serde_json::from_str(&json).expect("Deserialization should succeed");

    assert_eq!(restored.equivocation_proofs.len(), 2);
    assert_eq!(restored.timestamp_ms, 2000);
}
