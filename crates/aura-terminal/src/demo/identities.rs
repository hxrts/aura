use aura_core::time::TimeStamp;
use aura_core::{AuthorityId, ContextId, DeviceId};
use serde::Serialize;

use crate::ids;

#[derive(Debug, Clone, Serialize)]
pub(super) struct GuardianAcceptance {
    pub guardian_id: AuthorityId,
    pub setup_id: String,
    pub accepted: bool,
    pub public_key: Vec<u8>,
    pub timestamp: TimeStamp,
}

pub(super) fn demo_authority_id(seed: u64, name: &str) -> AuthorityId {
    ids::authority_id(&format!("demo:{seed}:{name}:authority"))
}

pub(super) fn demo_device_id(seed: u64, name: &str) -> DeviceId {
    ids::device_id(&format!("demo:{seed}:{name}:device"))
}

pub(super) fn demo_context_id(seed: u64, name: &str) -> ContextId {
    ids::context_id(&format!("demo:{seed}:{name}:context"))
}
