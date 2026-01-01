//! Tests for consensus share ordering semantics.
#![allow(missing_docs)]

use aura_consensus::core::state::{PathSelection, ShareData, ShareProposal};
use aura_consensus::core::transitions::{apply_share, start_consensus, TransitionResult};
use std::collections::BTreeSet;

fn base_state() -> aura_consensus::core::state::ConsensusState {
    let witnesses: BTreeSet<String> = ["A", "B", "C"].iter().map(|s| s.to_string()).collect();
    match start_consensus(
        "cid-1".to_string(),
        "op".to_string(),
        "prestate".to_string(),
        2,
        witnesses,
        "A".to_string(),
        PathSelection::FastPath,
    ) {
        TransitionResult::Ok(state) => state,
        TransitionResult::NotEnabled(reason) => panic!("start_consensus failed: {reason}"),
    }
}

fn proposal(witness: &str) -> ShareProposal {
    ShareProposal {
        witness: witness.to_string(),
        result_id: "rid-1".to_string(),
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
        assert_eq!(result_id, "rid-1");
    }
}
