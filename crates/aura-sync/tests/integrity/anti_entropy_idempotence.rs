//! Anti-entropy merge idempotence tests.
//!
//! Verifies that merging the same batch of attested operations twice produces
//! the same result. If non-idempotent, repeated syncs cause state drift.

#![allow(missing_docs)]
use aura_core::tree::{AttestedOp, TreeOp, TreeOpKind};
use aura_core::{ContextId, DeviceId, Epoch, Hash32};
use aura_guards::{
    DecodedIngress, IngressSource, IngressVerificationEvidence, VerifiedIngress,
    VerifiedIngressMetadata,
};
use aura_sync::protocols::{AntiEntropyConfig, AntiEntropyProtocol};

fn make_attested_op(seed: u8) -> AttestedOp {
    AttestedOp {
        op: TreeOp {
            parent_epoch: Epoch::new(seed as u64),
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

fn verified_ops(ops: Vec<AttestedOp>) -> VerifiedIngress<Vec<AttestedOp>> {
    let peer = DeviceId::new_from_entropy([7u8; 32]);
    let context = ContextId::new_from_entropy([8u8; 32]);
    let metadata = VerifiedIngressMetadata::new(
        IngressSource::Device(peer),
        context,
        None,
        Hash32::zero(),
        1,
    );
    let evidence = IngressVerificationEvidence::new(
        metadata.clone(),
        aura_guards::REQUIRED_INGRESS_VERIFICATION_CHECKS,
    )
    .unwrap();
    DecodedIngress::new(ops, metadata).verify(evidence).unwrap()
}

#[test]
fn anti_entropy_merge_batch_is_idempotent() {
    let protocol = AntiEntropyProtocol::new(AntiEntropyConfig::default());

    let mut local_ops = Vec::new();
    let incoming = vec![make_attested_op(1), make_attested_op(2)];

    let first = protocol
        .merge_batch(&mut local_ops, verified_ops(incoming.clone()))
        .unwrap_or_else(|err| panic!("first merge succeeds: {err}"));
    assert_eq!(first.applied, 2);
    assert_eq!(first.duplicates, 0);

    let second = protocol
        .merge_batch(&mut local_ops, verified_ops(incoming))
        .unwrap_or_else(|err| panic!("second merge succeeds: {err}"));
    assert_eq!(second.applied, 0);
    assert_eq!(second.duplicates, 2);
}
