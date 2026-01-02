//! Tests for consensus share ordering semantics.
#![allow(missing_docs)]

use aura_consensus::core::state::{ConsensusThreshold, PathSelection, ShareData, ShareProposal};
use aura_consensus::core::transitions::{apply_share, start_consensus, TransitionResult};
use aura_consensus::types::ConsensusId;
use aura_core::{AuthorityId, Hash32, OperationId};
use std::collections::BTreeSet;

fn threshold(value: u16) -> ConsensusThreshold {
    ConsensusThreshold::new(value).expect("threshold")
}

fn test_authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

fn test_hash(label: &str) -> Hash32 {
    Hash32::from_bytes(label.as_bytes())
}

fn test_consensus_id(label: &str) -> ConsensusId {
    ConsensusId(Hash32::from_bytes(label.as_bytes()))
}

fn test_operation(seed: u8) -> OperationId {
    OperationId::new_from_entropy([seed; 32])
}

fn base_state() -> aura_consensus::core::state::ConsensusState {
    let witnesses: BTreeSet<AuthorityId> = [b'A', b'B', b'C']
        .iter()
        .map(|&b| test_authority(b))
        .collect();
    match start_consensus(
        test_consensus_id("cid-1"),
        test_operation(1),
        test_hash("prestate"),
        threshold(2),
        witnesses,
        test_authority(b'A'),
        PathSelection::FastPath,
    ) {
        TransitionResult::Ok(state) => state,
        TransitionResult::NotEnabled(reason) => panic!("start_consensus failed: {reason}"),
    }
}

fn proposal(witness: &str) -> ShareProposal {
    ShareProposal {
        witness: test_authority(witness.as_bytes()[0]),
        result_id: test_hash("rid-1"),
        share: ShareData {
            share_value: format!("share-{witness}"),
            nonce_binding: "nonce".to_string(),
            data_binding: "binding".to_string(),
        },
    }
}

fn apply_sequence(
    mut state: aura_consensus::core::state::ConsensusState,
    proposals: &[ShareProposal],
) -> aura_consensus::core::state::ConsensusState {
    for proposal in proposals {
        if let TransitionResult::Ok(next) = apply_share(&state, proposal.clone()) {
            state = next;
        }
    }
    state
}

#[test]
fn consensus_share_ordering_is_commutative_for_same_result() {
    let proposals = [proposal("A"), proposal("B"), proposal("C")];

    let permutations = vec![
        vec![0, 1, 2],
        vec![0, 2, 1],
        vec![1, 0, 2],
        vec![1, 2, 0],
        vec![2, 0, 1],
        vec![2, 1, 0],
    ];

    let mut commit_results = Vec::new();

    for order in permutations {
        let state = base_state();
        let ordered: Vec<ShareProposal> = order.iter().map(|&i| proposals[i].clone()).collect();
        let final_state = apply_sequence(state, &ordered);
        let commit = final_state
            .commit_fact
            .unwrap_or_else(|| panic!("consensus should commit"));
        commit_results.push(commit.result_id);
    }

    for result_id in commit_results.iter() {
        assert_eq!(result_id, &test_hash("rid-1"));
    }
}
