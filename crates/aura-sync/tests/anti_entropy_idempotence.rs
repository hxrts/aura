#![allow(missing_docs)]
use aura_core::tree::{AttestedOp, TreeOp, TreeOpKind};
use aura_sync::protocols::{AntiEntropyConfig, AntiEntropyProtocol};

fn make_attested_op(seed: u8) -> AttestedOp {
    AttestedOp {
        op: TreeOp {
            parent_epoch: seed as u64,
            parent_commitment: [seed; 32],
            op: TreeOpKind::RotateEpoch {
                affected: Vec::new(),
            },
            version: 1,
        },
        agg_sig: vec![seed; 64],
        signer_count: 1,
    }
}

#[test]
fn anti_entropy_merge_batch_is_idempotent() {
    let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

    let mut local_ops = Vec::new();
    let incoming = vec![make_attested_op(1), make_attested_op(2)];

    let first = protocol
        .merge_batch(&mut local_ops, incoming.clone())
        .unwrap_or_else(|err| panic!("first merge succeeds: {err}"));
    assert_eq!(first.applied, 2);
    assert_eq!(first.duplicates, 0);

    let second = protocol
        .merge_batch(&mut local_ops, incoming)
        .unwrap_or_else(|err| panic!("second merge succeeds: {err}"));
    assert_eq!(second.applied, 0);
    assert_eq!(second.duplicates, 2);
}
