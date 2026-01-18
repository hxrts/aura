//! Example: How callers should integrate equivocation detection
//!
//! This test demonstrates the pattern for Layer 5/6 callers (relational consensus,
//! agent runtime) to integrate equivocation detection into their consensus flows.

use aura_consensus::{
    core::validation::EquivocationDetector,
    facts::ConsensusFact,
    types::ConsensusId,
    witness::WitnessTracker,
};
use aura_core::{
    frost::PartialSignature,
    identifiers::{AuthorityId, ContextId},
    Hash32,
};

/// Example: Relational consensus coordinator integrating equivocation detection
#[test]
fn test_caller_integration_pattern() {
    // Caller has context information
    let context_id = ContextId::new_from_entropy([1u8; 32]);
    let consensus_id = ConsensusId(Hash32::new([3u8; 32]));
    let prestate_hash = Hash32::new([4u8; 32]);

    // Create tracker with detection enabled
    let mut tracker = WitnessTracker::new();

    // Witness 1 submits first signature
    let witness1 = AuthorityId::new_from_entropy([10u8; 32]);
    let sig1 = PartialSignature {
        signer: 1,
        signature: vec![1u8; 64],
    };
    let result_id_1 = Hash32::new([5u8; 32]);

    tracker.record_signature_with_detection(
        context_id,
        witness1,
        sig1,
        consensus_id,
        prestate_hash,
        result_id_1,
        1000,
    );

    // Verify no equivocation yet
    assert_eq!(tracker.get_equivocation_proofs().len(), 0);
    assert_eq!(tracker.get_signatures().len(), 1);

    // Witness 1 tries to equivocate with different result_id
    let sig2 = PartialSignature {
        signer: 1,
        signature: vec![2u8; 64],
    };
    let result_id_2 = Hash32::new([6u8; 32]);

    tracker.record_signature_with_detection(
        context_id,
        witness1,
        sig2,
        consensus_id,
        prestate_hash,
        result_id_2,
        2000,
    );

    // Equivocation detected - signature rejected
    assert_eq!(tracker.get_equivocation_proofs().len(), 1);
    assert_eq!(tracker.get_signatures().len(), 1); // Still only first signature

    // Extract proofs for journal emission
    let proofs = tracker.get_equivocation_proofs();
    assert_eq!(proofs.len(), 1);

    // Caller would emit to journal:
    // for proof in proofs {
    //     let fact = proof.to_generic();
    //     journal_effects.add_fact(context_id, fact).await?;
    // }

    // Verify proof contents
    match &proofs[0] {
        ConsensusFact::EquivocationProof(proof) => {
            assert_eq!(proof.context_id, context_id);
            assert_eq!(proof.witness, witness1);
            assert_eq!(proof.consensus_id, consensus_id.0);
            assert_eq!(proof.prestate_hash, prestate_hash);
            assert_eq!(proof.first_result_id, result_id_1);
            assert_eq!(proof.second_result_id, result_id_2);
            assert_eq!(proof.timestamp.ts_ms, 2000);
        }
    }

    // After emitting, clear to prevent duplicate emission
    tracker.clear_equivocation_proofs();
    assert_eq!(tracker.get_equivocation_proofs().len(), 0);
}

/// Example: Multi-round consensus with accumulated proofs
#[test]
fn test_multi_round_accumulation() {
    let context_id = ContextId::new_from_entropy([1u8; 32]);
    let consensus_id_1 = ConsensusId(Hash32::new([10u8; 32]));
    let consensus_id_2 = ConsensusId(Hash32::new([20u8; 32]));
    let prestate_hash = Hash32::new([4u8; 32]);

    let mut tracker = WitnessTracker::new();

    let witness1 = AuthorityId::new_from_entropy([100u8; 32]);
    let witness2 = AuthorityId::new_from_entropy([200u8; 32]);

    // Round 1: witness1 equivocates
    tracker.record_signature_with_detection(
        context_id,
        witness1,
        PartialSignature {
            signer: 1,
            signature: vec![1u8; 64],
        },
        consensus_id_1,
        prestate_hash,
        Hash32::new([1u8; 32]),
        1000,
    );

    tracker.record_signature_with_detection(
        context_id,
        witness1,
        PartialSignature {
            signer: 1,
            signature: vec![2u8; 64],
        },
        consensus_id_1,
        prestate_hash,
        Hash32::new([2u8; 32]),
        2000,
    );

    assert_eq!(tracker.get_equivocation_proofs().len(), 1);

    // Round 2: witness2 equivocates in different consensus
    tracker.record_signature_with_detection(
        context_id,
        witness2,
        PartialSignature {
            signer: 2,
            signature: vec![10u8; 64],
        },
        consensus_id_2,
        prestate_hash,
        Hash32::new([3u8; 32]),
        3000,
    );

    tracker.record_signature_with_detection(
        context_id,
        witness2,
        PartialSignature {
            signer: 2,
            signature: vec![20u8; 64],
        },
        consensus_id_2,
        prestate_hash,
        Hash32::new([4u8; 32]),
        4000,
    );

    // Both equivocations accumulated
    assert_eq!(tracker.get_equivocation_proofs().len(), 2);

    // Caller emits both proofs in a batch
    let proofs = tracker.get_equivocation_proofs();
    assert_eq!(proofs.len(), 2);

    // Verify both proofs have correct context
    for proof_fact in proofs {
        match proof_fact {
            ConsensusFact::EquivocationProof(proof) => {
                assert_eq!(proof.context_id, context_id);
                assert!(proof.witness == witness1 || proof.witness == witness2);
            }
        }
    }
}

/// Example: Standalone detector for custom integration
#[test]
fn test_standalone_detector_usage() {
    let mut detector = EquivocationDetector::new();

    let context_id = ContextId::new_from_entropy([1u8; 32]);
    let witness = AuthorityId::new_from_entropy([2u8; 32]);
    let consensus_id = ConsensusId(Hash32::new([3u8; 32]));
    let prestate_hash = Hash32::new([4u8; 32]);

    // First share - no equivocation
    let proof1 = detector.check_share(
        context_id,
        witness,
        consensus_id,
        prestate_hash,
        Hash32::new([5u8; 32]),
        1000,
    );
    assert!(proof1.is_none());

    // Second share for same result - no equivocation (duplicate)
    let proof2 = detector.check_share(
        context_id,
        witness,
        consensus_id,
        prestate_hash,
        Hash32::new([5u8; 32]),
        1500,
    );
    assert!(proof2.is_none());

    // Third share for different result - equivocation!
    let proof3 = detector.check_share(
        context_id,
        witness,
        consensus_id,
        prestate_hash,
        Hash32::new([6u8; 32]),
        2000,
    );
    assert!(proof3.is_some());

    // Caller can process proof immediately
    if let Some(ConsensusFact::EquivocationProof(proof)) = proof3 {
        assert_eq!(proof.witness, witness);
        assert_eq!(proof.consensus_id, consensus_id.0);
        // Emit to journal, log security alert, etc.
    }
}

/// Example: Integration with consensus result
#[test]
fn test_consensus_result_equivocation_proofs() {
    use aura_consensus::types::{CommitFact, ConsensusResult};
    use aura_core::{
        frost::ThresholdSignature,
        time::{PhysicalTime, ProvenancedTime, TimeStamp},
    };

    let context_id = ContextId::new_from_entropy([1u8; 32]);

    // Simulate consensus with detected equivocation
    let mut tracker = WitnessTracker::new();

    let consensus_id = ConsensusId(Hash32::new([10u8; 32]));
    let prestate_hash = Hash32::new([20u8; 32]);

    // Honest witness
    let witness_honest = AuthorityId::new_from_entropy([1u8; 32]);
    tracker.record_signature_with_detection(
        context_id,
        witness_honest,
        PartialSignature {
            signer: 1,
            signature: vec![1u8; 64],
        },
        consensus_id,
        prestate_hash,
        Hash32::new([100u8; 32]),
        1000,
    );

    // Equivocating witness
    let witness_evil = AuthorityId::new_from_entropy([2u8; 32]);
    tracker.record_signature_with_detection(
        context_id,
        witness_evil,
        PartialSignature {
            signer: 2,
            signature: vec![2u8; 64],
        },
        consensus_id,
        prestate_hash,
        Hash32::new([100u8; 32]),
        2000,
    );

    tracker.record_signature_with_detection(
        context_id,
        witness_evil,
        PartialSignature {
            signer: 2,
            signature: vec![3u8; 64],
        },
        consensus_id,
        prestate_hash,
        Hash32::new([200u8; 32]), // Different result!
        2500,
    );

    // Extract accumulated proofs
    let equivocation_proofs = tracker.get_equivocation_proofs().to_vec();
    assert_eq!(equivocation_proofs.len(), 1);

    // Create consensus result with proofs
    let commit_fact = CommitFact::new(
        consensus_id,
        prestate_hash,
        Hash32::new([30u8; 32]),
        vec![1, 2, 3],
        ThresholdSignature {
            signature: vec![0u8; 64],
            signers: vec![1],
        },
        None,
        vec![witness_honest],
        2,
        false,
        ProvenancedTime {
            stamp: TimeStamp::PhysicalClock(PhysicalTime {
                ts_ms: 3000,
                uncertainty: None,
            }),
            proofs: vec![],
            origin: None,
        },
    );

    let result = ConsensusResult::Committed {
        commit: commit_fact,
        equivocation_proofs,
    };

    // Caller extracts and emits proofs
    let proofs = result.equivocation_proofs();
    assert_eq!(proofs.len(), 1);

    match &proofs[0] {
        ConsensusFact::EquivocationProof(proof) => {
            assert_eq!(proof.witness, witness_evil);
            // Emit to journal for accountability
        }
    }
}
