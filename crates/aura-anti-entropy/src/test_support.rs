//! Shared test fixtures for anti-entropy tests.

use crate::BloomDigest;
use aura_core::types::identifiers::{ContextId, DeviceId};
use aura_core::{tree::AttestedOp, Epoch, Hash32, TreeOp, TreeOpKind};
use aura_journal::{LeafId, LeafNode, NodeIndex};
use std::collections::BTreeSet;

pub fn create_test_op(commitment: Hash32) -> AttestedOp {
    AttestedOp {
        op: TreeOp {
            parent_commitment: commitment.0,
            parent_epoch: Epoch::new(1),
            op: TreeOpKind::AddLeaf {
                leaf: LeafNode::new_device(
                    LeafId(1),
                    DeviceId::new_from_entropy([3u8; 32]),
                    vec![1, 2, 3],
                )
                .expect("valid leaf"),
                under: NodeIndex(0),
            },
            version: 1,
        },
        agg_sig: vec![],
        signer_count: 1,
    }
}

pub fn digest_from_hashes<I>(cids: I) -> BloomDigest
where
    I: IntoIterator<Item = Hash32>,
{
    BloomDigest {
        cids: cids.into_iter().collect::<BTreeSet<_>>(),
    }
}

pub fn test_context(seed: u8) -> ContextId {
    ContextId::new_from_entropy([seed; 32])
}

pub fn test_device(id: u128) -> DeviceId {
    DeviceId::from_uuid(uuid::Uuid::from_u128(id))
}
