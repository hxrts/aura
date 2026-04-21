use aura_core::types::authority::TreeStateSummary;
use aura_core::{Epoch, Hash32};
use aura_journal::commitment_integration::TreeStateConversion;
use aura_journal::commitment_tree::authority_state::AuthorityTreeState;
use aura_journal::{FactAttestedOp, TreeOpKind};

#[test]
fn tree_state_conversion() {
    let mut auth_state = AuthorityTreeState::new();
    auth_state.epoch = Epoch::new(5);
    auth_state.root_commitment = [1; 32];

    let tree_state = auth_state.to_tree_state_summary();
    assert_eq!(tree_state.epoch(), Epoch::new(5));
    assert_eq!(tree_state.root_commitment().0, [1; 32]);
    assert_eq!(tree_state.threshold(), 1);
    assert_eq!(tree_state.device_count(), 0);
}

#[test]
fn attested_op_validation() {
    let parent_commitment = Hash32::new([2; 32]);
    let tree_state = TreeStateSummary::with_values(Epoch::new(10), parent_commitment, 2, 3);

    let valid_op = FactAttestedOp {
        tree_op: TreeOpKind::AddLeaf {
            public_key: vec![0; 32],
            role: aura_core::tree::LeafRole::Device,
        },
        parent_commitment,
        new_commitment: Hash32::new([3; 32]),
        witness_threshold: 2,
        signature: vec![],
    };

    assert!(valid_op.validate_against_parent(&tree_state));

    let invalid_op = FactAttestedOp {
        parent_commitment: Hash32::from_bytes(&[99; 32]),
        ..valid_op
    };
    assert!(!invalid_op.validate_against_parent(&tree_state));
}
