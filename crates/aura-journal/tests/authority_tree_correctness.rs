//! Comprehensive correctness tests for authority-internal incremental tree updates.
#![allow(clippy::expect_used, clippy::uninlined_format_args)]

use aura_journal::commitment_integration::TreeStateConversion;
use aura_journal::commitment_tree::authority_state::AuthorityTreeState;
use aura_journal::LeafId;
use proptest::prelude::*;
use rand_chacha::rand_core::{RngCore, SeedableRng};
use rand_chacha::ChaCha20Rng;

fn key_bytes(seed: u64, index: u32) -> Vec<u8> {
    let mut rng = ChaCha20Rng::seed_from_u64(seed ^ u64::from(index));
    let mut key = vec![0u8; 32];
    rng.fill_bytes(&mut key);
    key
}

fn build_state(device_count: u32) -> AuthorityTreeState {
    let mut state = AuthorityTreeState::new();
    for i in 0..device_count {
        state.add_device(key_bytes(0xCAFE_BABE, i));
    }
    state
}

#[test]
fn primitive_mutations_preserve_invariants() {
    let mut state = AuthorityTreeState::new();
    let l0 = state.add_device(vec![1; 32]);
    let l1 = state.add_device(vec![2; 32]);

    assert_eq!(state.active_leaf_count(), 2);
    assert!(state.validate_topology_invariants().is_ok());

    state
        .update_leaf_public_key(l0, vec![3; 32])
        .expect("leaf key update should succeed");
    assert!(state.validate_topology_invariants().is_ok());

    state
        .update_threshold(2)
        .expect("threshold update should succeed");
    assert_eq!(state.get_threshold(), 2);
    assert!(state.validate_topology_invariants().is_ok());

    let epoch_before = state.epoch;
    state.rotate_epoch().expect("epoch rotation should succeed");
    assert!(state.epoch > epoch_before);
    assert!(state.validate_topology_invariants().is_ok());

    state
        .remove_device(l1.0)
        .expect("remove device should succeed");
    assert_eq!(state.active_leaf_count(), 1);
    assert!(state.validate_topology_invariants().is_ok());
}

#[test]
fn tree_commitment_root_fixtures() {
    let mut state = AuthorityTreeState::new();
    state.add_device(vec![0x11; 32]);
    state.add_device(vec![0x22; 32]);
    state
        .update_threshold(2)
        .expect("threshold update should succeed");
    let root_after_threshold = state.root_commitment;

    state.rotate_epoch().expect("epoch rotate should succeed");
    let root_after_epoch = state.root_commitment;

    state
        .update_leaf_public_key(LeafId(0), vec![0x33; 32])
        .expect("leaf update should succeed");
    let root_after_key_update = state.root_commitment;

    assert_eq!(
        root_after_threshold,
        [
            0x95, 0x12, 0x60, 0x26, 0xA6, 0xE9, 0x14, 0xDA, 0x0A, 0xA5, 0xBF, 0xD3, 0x1B, 0x4B,
            0x8F, 0x64, 0xAB, 0x95, 0xDF, 0x61, 0x48, 0xFD, 0x4D, 0xB0, 0x49, 0x47, 0xCC, 0xAF,
            0xB6, 0xCC, 0x0E, 0x57
        ]
    );
    assert_eq!(
        root_after_epoch,
        [
            0xAB, 0x7C, 0x96, 0x13, 0x63, 0xA0, 0x09, 0xA6, 0xCF, 0x5E, 0xA1, 0x1E, 0x9C, 0x23,
            0x01, 0x80, 0xC7, 0xCB, 0xD5, 0xA5, 0xA7, 0x52, 0x6E, 0x4E, 0x0D, 0x6F, 0xE0, 0xFE,
            0x89, 0x85, 0x03, 0x67
        ]
    );
    assert_eq!(
        root_after_key_update,
        [
            0x42, 0x2A, 0xAE, 0xDE, 0x62, 0xCE, 0x35, 0xE0, 0xC3, 0xE3, 0x08, 0xC6, 0x23, 0x35,
            0x21, 0xA6, 0x15, 0xE7, 0x16, 0xD3, 0x78, 0x99, 0xED, 0x43, 0xB4, 0x3A, 0x93, 0x39,
            0x76, 0xE5, 0xDD, 0x84
        ]
    );
}

#[test]
fn incremental_matches_full_recompute_for_single_mutations() {
    let mut state = build_state(12);
    assert!(state.validate_topology_invariants().is_ok());

    state
        .update_leaf_public_key(LeafId(5), vec![9; 32])
        .expect("leaf update should succeed");
    assert_eq!(
        state.root_commitment,
        state.recompute_root_commitment_full()
    );

    state
        .update_threshold(3)
        .expect("threshold update should succeed");
    assert_eq!(
        state.root_commitment,
        state.recompute_root_commitment_full()
    );

    state.rotate_epoch().expect("epoch update should succeed");
    assert_eq!(
        state.root_commitment,
        state.recompute_root_commitment_full()
    );
}

#[test]
fn recompute_is_idempotent_without_mutation() {
    let state = build_state(16);
    let current = state.root_commitment;
    let full = state.recompute_root_commitment_full();
    assert_eq!(current, full);

    let full_again = state.recompute_root_commitment_full();
    assert_eq!(full, full_again);
}

#[test]
fn merkle_proofs_verify_for_all_active_leaves() {
    let state = build_state(21);
    for i in 0..21 {
        let leaf_id = LeafId(i);
        assert!(
            state.verify_merkle_proof(leaf_id),
            "proof should verify for leaf {}",
            i
        );
    }
}

#[test]
fn stale_proof_is_rejected_after_relevant_mutation() {
    let mut state = build_state(9);
    let target = LeafId(4);
    let stale = state
        .merkle_proof(target)
        .expect("proof should exist")
        .to_vec();

    // Structural change should invalidate stale proof paths.
    state.remove_device(8).expect("remove should succeed");

    assert!(state.verify_merkle_proof(target));
    assert!(
        !state.verify_merkle_proof_path(target, &stale),
        "old proof should be rejected after relevant mutation"
    );
}

#[test]
fn unaffected_leaf_proof_expectation_is_explicit() {
    let mut state = build_state(8);
    let untouched_leaf = LeafId(0);
    let before = state
        .merkle_proof(untouched_leaf)
        .expect("proof should exist")
        .to_vec();

    state
        .update_leaf_public_key(LeafId(7), vec![0xCD; 32])
        .expect("update should succeed");

    // In this fixed topology, updating Leaf(7) changes the sibling subtree hash
    // used by Leaf(0), so the previous proof is expected to become stale.
    assert!(
        !state.verify_merkle_proof_path(untouched_leaf, &before),
        "old untouched-leaf proof should be rejected in this scenario"
    );
    assert!(state.verify_merkle_proof(untouched_leaf));
}

#[test]
fn serialization_roundtrip_preserves_commitment_and_topology() {
    let mut state = build_state(13);
    state.update_threshold(4).expect("threshold update");
    state.rotate_epoch().expect("epoch update");

    let encoded = serde_json::to_vec(&state).expect("serialize state");
    let decoded: AuthorityTreeState = serde_json::from_slice(&encoded).expect("deserialize state");

    assert_eq!(state.root_commitment, decoded.root_commitment);
    assert_eq!(state.active_leaf_count(), decoded.active_leaf_count());
    assert_eq!(state.get_threshold(), decoded.get_threshold());
    assert!(decoded.validate_topology_invariants().is_ok());
    assert_eq!(
        decoded.root_commitment,
        decoded.recompute_root_commitment_full()
    );
}

#[test]
fn tree_state_summary_cross_check() {
    let mut state = build_state(7);
    state.update_threshold(3).expect("threshold update");

    let summary = state.to_tree_state_summary();
    assert_eq!(summary.epoch(), state.epoch);
    assert_eq!(summary.root_commitment().0, state.root_commitment);
    assert_eq!(summary.threshold(), state.get_threshold());
    assert_eq!(summary.device_count(), state.active_leaf_count() as u32);
}

#[test]
fn regression_edge_cases() {
    // Empty tree
    let empty = AuthorityTreeState::new();
    assert!(empty.validate_topology_invariants().is_ok());

    // Singleton tree
    let mut singleton = AuthorityTreeState::new();
    let leaf = singleton.add_device(vec![7; 32]);
    assert!(singleton.validate_topology_invariants().is_ok());
    assert!(singleton.verify_merkle_proof(leaf));

    // Odd leaves
    let odd = build_state(5);
    assert!(odd.validate_topology_invariants().is_ok());

    // Add/remove churn
    let mut churn = build_state(6);
    for _ in 0..20 {
        let next = churn.add_device(vec![0xEE; 32]);
        churn.remove_device(next.0).expect("churn remove");
        assert!(churn.validate_topology_invariants().is_ok());
        assert_eq!(
            churn.root_commitment,
            churn.recompute_root_commitment_full()
        );
    }
}

#[test]
fn non_structural_recompute_touches_subset_of_branches() {
    let mut state = build_state(128);
    let total_branches = state.branches.len() as u32;

    state
        .update_leaf_public_key(LeafId(64), vec![0x1A; 32])
        .expect("leaf update should succeed");

    let recomputed = state.last_recomputed_branch_count();
    assert!(recomputed > 0);
    assert!(
        recomputed < total_branches,
        "non-structural update should not recompute all branches: recomputed={} total={}",
        recomputed,
        total_branches
    );
}

#[test]
fn scaling_guard_non_structural_recompute_counts() {
    for n in [32_u32, 128, 512] {
        let mut state = build_state(n);
        let total_branches = state.branches.len() as u32;
        let target = LeafId(n / 2);

        state
            .update_leaf_public_key(target, key_bytes(0x1234_5678, n))
            .expect("leaf update should succeed");

        let recomputed = state.last_recomputed_branch_count();
        assert!(
            recomputed < total_branches,
            "expected incremental path recompute for n={} (recomputed {} of {})",
            n,
            recomputed,
            total_branches
        );
    }
}

#[test]
#[ignore = "large scaling guard; run manually when tuning tree performance"]
fn scaling_guard_non_structural_recompute_counts_large() {
    let n = 2048_u32;
    let mut state = build_state(n);
    let total_branches = state.branches.len() as u32;
    let target = LeafId(n / 2);

    state
        .update_leaf_public_key(target, key_bytes(0xDEAD_BEEF, n))
        .expect("leaf update should succeed");

    let recomputed = state.last_recomputed_branch_count();
    assert!(
        recomputed < total_branches,
        "expected incremental path recompute for n={} (recomputed {} of {})",
        n,
        recomputed,
        total_branches
    );
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Add,
    Remove,
    Update,
    Threshold,
    Rotate,
}

proptest! {
    #[test]
    fn random_sequences_preserve_invariants_and_commitment_parity(
        ops in prop::collection::vec((0u8..5u8, 0u16..2048u16, prop::array::uniform8(any::<u8>())), 1..200)
    ) {
        let mut state = build_state(4);

        for (step, (op_tag, arg, bytes)) in ops.into_iter().enumerate() {
            let op = match op_tag {
                0 => Op::Add,
                1 => Op::Remove,
                2 => Op::Update,
                3 => Op::Threshold,
                _ => Op::Rotate,
            };

            match op {
                Op::Add => {
                    state.add_device(bytes.to_vec());
                }
                Op::Remove => {
                    if state.active_leaf_count() > 0 {
                        let leaves: Vec<_> = state.get_external_leaves().keys().copied().collect();
                        let leaf = leaves[usize::from(arg) % leaves.len()];
                        let _ = state.remove_device(leaf.0);
                    }
                }
                Op::Update => {
                    if state.active_leaf_count() > 0 {
                        let leaves: Vec<_> = state.get_external_leaves().keys().copied().collect();
                        let leaf = leaves[usize::from(arg) % leaves.len()];
                        let _ = state.update_leaf_public_key(leaf, bytes.to_vec());
                    }
                }
                Op::Threshold => {
                    let count = state.active_leaf_count() as u16;
                    if count > 0 {
                        let threshold = (arg % count).max(1);
                        let _ = state.update_threshold(threshold);
                    }
                }
                Op::Rotate => {
                    let _ = state.rotate_epoch();
                }
            }

            prop_assert!(
                state.validate_topology_invariants().is_ok(),
                "topology invariant failed at step {}",
                step
            );
            prop_assert_eq!(
                state.root_commitment,
                state.recompute_root_commitment_full(),
                "incremental/full mismatch at step {}",
                step
            );
        }
    }
}
