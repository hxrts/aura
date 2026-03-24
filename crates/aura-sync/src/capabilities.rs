#![allow(missing_docs)]

use aura_macros::capability_family;

#[capability_family(namespace = "sync")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncCapability {
    #[capability("request_digest")]
    RequestDigest,
    #[capability("request_ops")]
    RequestOps,
    #[capability("push_ops")]
    PushOps,
    #[capability("announce_op")]
    AnnounceOp,
    #[capability("push_op")]
    PushOp,
}

#[capability_family(namespace = "sync")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncEpochCapability {
    #[capability("epoch:propose_rotation")]
    ProposeRotation,
    #[capability("epoch:confirm_readiness")]
    ConfirmReadiness,
    #[capability("epoch:commit_rotation")]
    CommitRotation,
}

pub fn evaluation_candidates_for_sync_guard() -> &'static [SyncCapability] {
    SyncCapability::declared_names()
}

pub fn evaluation_candidates_for_epoch_rotation_protocol() -> &'static [SyncEpochCapability] {
    SyncEpochCapability::declared_names()
}
