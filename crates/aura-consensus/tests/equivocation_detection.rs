//! Integration tests for equivocation detection
//!
//! These tests verify that the equivocation detection system correctly
//! identifies and generates proofs when witnesses submit conflicting signatures.

use aura_consensus::{
    core::validation::EquivocationDetector,
    facts::{ConsensusFact, EquivocationProof},
    types::ConsensusId,
    witness::WitnessTracker,
};
use aura_core::{
    frost::PartialSignature,
    identifiers::{AuthorityId, ContextId},
    Hash32,
};

#[test]
fn test_witness_tracker_detects_equivocation() {
    let mut tracker = WitnessTracker::new();

    let context_id = ContextId::new_from_entropy([1u8; 32]);
    let witness = AuthorityId::new_from_entropy([2u8; 32]);
    let consensus_id = ConsensusId(Hash32::new([3u8; 32]));
    let prestate_hash = Hash32::new([4u8; 32]);
    let result_id_1 = Hash32::new([5u8; 32]);
    let result_id_2 = Hash32::new([6u8; 32]);

    let sig1 = PartialSignature {
        signer: 1,
        signature: vec![1u8; 64],
    };

    let sig2 = PartialSignature {
        signer: 1,
        signature: vec![2u8; 64],
    };

    // First signature for result_id_1 - should be accepted
    tracker.record_signature_with_detection(
        context_id,
        witness,
        sig1.clone(),
        consensus_id,
        prestate_hash,
        result_id_1,
        1000,
    );

    // Verify signature was added
    assert_eq!(tracker.get_signatures().len(), 1);
    assert_eq!(tracker.get_equivocation_proofs().len(), 0);

    // Second signature for result_id_2 - should trigger equivocation detection
    tracker.record_signature_with_detection(
        context_id,
        witness,
        sig2,
        consensus_id,
        prestate_hash,
        result_id_2,
        2000,
    );

    // Verify equivocation was detected
    assert_eq!(tracker.get_signatures().len(), 1); // Still only first signature
    assert_eq!(tracker.get_equivocation_proofs().len(), 1); // Proof generated

    // Verify proof contents
    let proofs = tracker.get_equivocation_proofs();
    match &proofs[0] {
        ConsensusFact::EquivocationProof(proof) => {
            assert_eq!(proof.context_id, context_id);
            assert_eq!(proof.witness, witness);
            assert_eq!(proof.consensus_id, consensus_id.0);
            assert_eq!(proof.prestate_hash, prestate_hash);
            assert_eq!(proof.first_result_id, result_id_1);
            assert_eq!(proof.second_result_id, result_id_2);
            assert_eq!(proof.timestamp.ts_ms, 2000);
        }
    }
}

#[test]
fn test_witness_tracker_allows_duplicate_same_result() {
    let mut tracker = WitnessTracker::new();

    let context_id = ContextId::new_from_entropy([1u8; 32]);
    let witness = AuthorityId::new_from_entropy([2u8; 32]);
    let consensus_id = ConsensusId(Hash32::new([3u8; 32]));
    let prestate_hash = Hash32::new([4u8; 32]);
    let result_id = Hash32::new([5u8; 32]);

    let sig1 = PartialSignature {
        signer: 1,
        signature: vec![1u8; 64],
    };

    let sig2 = PartialSignature {
        signer: 1,
        signature: vec![2u8; 64], // Different signature bytes
    };

    // First signature
    tracker.record_signature_with_detection(
        context_id,
        witness,
        sig1,
        consensus_id,
        prestate_hash,
        result_id,
        1000,
    );

    // Second signature for SAME result_id - should not trigger equivocation
    tracker.record_signature_with_detection(
        context_id,
        witness,
        sig2,
        consensus_id,
        prestate_hash,
        result_id, // Same result_id
        2000,
    );

    // Verify no equivocation detected
    assert_eq!(tracker.get_equivocation_proofs().len(), 0);
    // First signature was already added, second is duplicate (same witness)
    assert_eq!(tracker.get_signatures().len(), 1);
}

#[test]
fn test_witness_tracker_tracks_multiple_witnesses() {
    let mut tracker = WitnessTracker::new();

    let context_id = ContextId::new_from_entropy([1u8; 32]);
    let witness1 = AuthorityId::new_from_entropy([2u8; 32]);
    let witness2 = AuthorityId::new_from_entropy([3u8; 32]);
    let consensus_id = ConsensusId(Hash32::new([4u8; 32]));
    let prestate_hash = Hash32::new([5u8; 32]);
    let result_id_1 = Hash32::new([6u8; 32]);
    let result_id_2 = Hash32::new([7u8; 32]);

    let sig1 = PartialSignature {
        signer: 1,
        signature: vec![1u8; 64],
    };

    let sig2 = PartialSignature {
        signer: 2,
        signature: vec![2u8; 64],
    };

    let sig3 = PartialSignature {
        signer: 1,
        signature: vec![3u8; 64],
    };

    // Witness 1 votes for result_id_1
    tracker.record_signature_with_detection(
        context_id,
        witness1,
        sig1,
        consensus_id,
        prestate_hash,
        result_id_1,
        1000,
    );

    // Witness 2 votes for result_id_2 (different witness, no equivocation)
    tracker.record_signature_with_detection(
        context_id,
        witness2,
        sig2,
        consensus_id,
        prestate_hash,
        result_id_2,
        2000,
    );

    // No equivocation yet - different witnesses voting for different results is OK
    assert_eq!(tracker.get_equivocation_proofs().len(), 0);
    assert_eq!(tracker.get_signatures().len(), 2);

    // Witness 1 tries to vote for result_id_2 - THIS is equivocation
    tracker.record_signature_with_detection(
        context_id,
        witness1, // Same witness as first vote
        sig3,
        consensus_id,
        prestate_hash,
        result_id_2, // Different result_id than first vote
        3000,
    );

    // Now we have equivocation from witness1
    assert_eq!(tracker.get_equivocation_proofs().len(), 1);
    assert_eq!(tracker.get_signatures().len(), 2); // Still just 2 valid signatures

    match &tracker.get_equivocation_proofs()[0] {
        ConsensusFact::EquivocationProof(proof) => {
            assert_eq!(proof.witness, witness1);
            assert_eq!(proof.first_result_id, result_id_1);
            assert_eq!(proof.second_result_id, result_id_2);
        }
    }
}

#[test]
fn test_witness_tracker_clear_proofs() {
    let mut tracker = WitnessTracker::new();

    let context_id = ContextId::new_from_entropy([1u8; 32]);
    let witness = AuthorityId::new_from_entropy([2u8; 32]);
    let consensus_id = ConsensusId(Hash32::new([3u8; 32]));
    let prestate_hash = Hash32::new([4u8; 32]);

    // Generate an equivocation proof
    tracker.record_signature_with_detection(
        context_id,
        witness,
        PartialSignature {
            signer: 1,
            signature: vec![1u8; 64],
        },
        consensus_id,
        prestate_hash,
        Hash32::new([5u8; 32]),
        1000,
    );

    tracker.record_signature_with_detection(
        context_id,
        witness,
        PartialSignature {
            signer: 1,
            signature: vec![2u8; 64],
        },
        consensus_id,
        prestate_hash,
        Hash32::new([6u8; 32]),
        2000,
    );

    assert_eq!(tracker.get_equivocation_proofs().len(), 1);

    // Clear proofs
    tracker.clear_equivocation_proofs();

    assert_eq!(tracker.get_equivocation_proofs().len(), 0);
}

#[test]
fn test_equivocation_proof_serialization() {
    let proof = EquivocationProof {
        context_id: ContextId::new_from_entropy([1u8; 32]),
        witness: AuthorityId::new_from_entropy([2u8; 32]),
        consensus_id: Hash32::new([3u8; 32]),
        prestate_hash: Hash32::new([4u8; 32]),
        first_result_id: Hash32::new([5u8; 32]),
        second_result_id: Hash32::new([6u8; 32]),
        timestamp: aura_core::time::PhysicalTime {
            ts_ms: 1000,
            uncertainty: None,
        },
    };

    let fact = ConsensusFact::EquivocationProof(proof.clone());

    // Test that we can convert to generic for journal emission
    use aura_journal::extensibility::DomainFact;
    let generic = fact.to_generic();

    // Verify it can be deserialized
    if let aura_journal::fact::RelationalFact::Generic { envelope, .. } = generic {
        assert_eq!(envelope.type_id.as_str(), "consensus");
        let restored = ConsensusFact::from_envelope(&envelope);
        assert!(restored.is_some());

        if let Some(ConsensusFact::EquivocationProof(restored_proof)) = restored {
            assert_eq!(restored_proof.witness, proof.witness);
            assert_eq!(restored_proof.consensus_id, proof.consensus_id);
            assert_eq!(restored_proof.first_result_id, proof.first_result_id);
            assert_eq!(restored_proof.second_result_id, proof.second_result_id);
        } else {
            panic!("Failed to deserialize proof");
        }
    } else {
        panic!("Expected Generic variant");
    }
}

#[test]
fn test_detector_independence_across_consensus_instances() {
    let mut detector = EquivocationDetector::new();

    let context_id = ContextId::new_from_entropy([1u8; 32]);
    let witness = AuthorityId::new_from_entropy([2u8; 32]);
    let consensus_id_1 = ConsensusId(Hash32::new([3u8; 32]));
    let consensus_id_2 = ConsensusId(Hash32::new([4u8; 32]));
    let prestate_hash = Hash32::new([5u8; 32]);
    let result_id_1 = Hash32::new([6u8; 32]);
    let result_id_2 = Hash32::new([7u8; 32]);

    // Witness votes for result_id_1 in consensus_id_1
    let proof1 = detector.check_share(
        context_id,
        witness,
        consensus_id_1,
        prestate_hash,
        result_id_1,
        1000,
    );
    assert!(proof1.is_none()); // First vote, no equivocation

    // Same witness votes for result_id_2 in DIFFERENT consensus_id_2
    // This is NOT equivocation - different consensus instances are independent
    let proof2 = detector.check_share(
        context_id,
        witness,
        consensus_id_2,
        prestate_hash,
        result_id_2,
        2000,
    );
    assert!(proof2.is_none()); // Different consensus, no equivocation

    // But if witness votes for result_id_2 in consensus_id_1, that IS equivocation
    let proof3 = detector.check_share(
        context_id,
        witness,
        consensus_id_1, // Same consensus as first vote
        prestate_hash,
        result_id_2, // Different result_id
        3000,
    );
    assert!(proof3.is_some()); // Equivocation detected!

    if let Some(ConsensusFact::EquivocationProof(proof)) = proof3 {
        assert_eq!(proof.consensus_id, consensus_id_1.0);
        assert_eq!(proof.first_result_id, result_id_1);
        assert_eq!(proof.second_result_id, result_id_2);
    } else {
        panic!("Expected equivocation proof");
    }
}
