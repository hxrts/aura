//! Deterministic structural benchmark input; no signature or live-owner proof.

use aura_core::{
    hash::hash,
    tree::{AttestedOp, LeafId, LeafNode, NodeIndex, TreeOp, TreeOpKind},
    DeviceId,
};
use aura_journal::commitment_tree::{reduce, TreeState};
use uuid::Uuid;

/// Original deterministic device identity used by these fixtures.
pub fn test_device_id(seed: u64) -> DeviceId {
    let digest = hash(format!("device-{seed}").as_bytes());
    let bytes: [u8; 16] = digest[..16]
        .try_into()
        .unwrap_or_else(|error| panic!("UUID-sized hash prefix: {error}"));
    DeviceId(Uuid::from_bytes(bytes))
}

/// Derive every parent from successful canonical reduction of its full prefix.
/// This deliberately expensive generator runs outside measured large setup.
pub fn connected_tree_history(device_count: u32, rotations: usize) -> (Vec<AttestedOp>, TreeState) {
    let mut history = Vec::new();
    let mut state = TreeState::new();
    for index in 0..device_count as usize + rotations {
        let kind = if index < device_count as usize {
            TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(
                    LeafId(index as u32),
                    test_device_id(index as u64),
                    vec![0u8; 32],
                )
                .unwrap_or_else(|error| panic!("valid fixture leaf: {error}")),
                under: NodeIndex(0),
            }
        } else {
            TreeOpKind::RotateEpoch {
                affected: vec![NodeIndex(0)],
            }
        };
        history.push(AttestedOp {
            op: TreeOp {
                parent_epoch: state.current_epoch(),
                parent_commitment: state.current_commitment(),
                op: kind,
                version: 1,
            },
            agg_sig: vec![0u8; 64],
            signer_count: 3,
        });
        state = reduce(&history)
            .unwrap_or_else(|error| panic!("connected fixture operation {index}: {error}"));
    }
    assert_eq!(state.list_leaf_ids().len(), device_count as usize);
    assert_eq!(u64::from(state.current_epoch()), rotations as u64);
    assert_eq!(history.len(), device_count as usize + rotations);
    (history, state)
}
