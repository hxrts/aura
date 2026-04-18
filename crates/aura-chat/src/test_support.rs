use crate::guards::GuardSnapshot;
use crate::types::ChatGroupId;
use aura_core::time::{PhysicalTime, TimeStamp};
use aura_core::types::identifiers::{AuthorityId, ChannelId, ContextId};
use aura_core::FlowCost;
use aura_guards::types::CapabilityId;
use uuid::Uuid;

pub(crate) fn test_context_id(seed: u8) -> ContextId {
    ContextId::new_from_entropy([seed; 32])
}

pub(crate) fn test_channel_id(seed: u8) -> ChannelId {
    ChannelId::from_bytes([seed; 32])
}

pub(crate) fn test_authority_id(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

pub(crate) fn test_group_id(seed: u8) -> ChatGroupId {
    ChatGroupId::from_uuid(Uuid::from_bytes([seed; 16]))
}

pub(crate) fn test_timestamp_ms(ts_ms: u64) -> TimeStamp {
    TimeStamp::PhysicalClock(PhysicalTime {
        ts_ms,
        uncertainty: None,
    })
}

pub(crate) fn test_guard_snapshot(
    authority_seed: u8,
    context_seed: u8,
    flow_budget_remaining: FlowCost,
    capabilities: Vec<CapabilityId>,
    now_ms: u64,
) -> GuardSnapshot {
    GuardSnapshot::new(
        test_authority_id(authority_seed),
        test_context_id(context_seed),
        flow_budget_remaining,
        capabilities,
        now_ms,
    )
}
