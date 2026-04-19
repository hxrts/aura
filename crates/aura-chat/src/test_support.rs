use crate::guards::GuardSnapshot;
use crate::types::ChatGroupId;
use aura_core::time::TimeStamp;
use aura_core::FlowCost;
use aura_guards::types::CapabilityId;
use aura_testkit::test_builders;
use uuid::Uuid;

pub(crate) fn test_context_id(seed: u8) -> aura_core::types::identifiers::ContextId {
    test_builders::context_id(seed)
}

pub(crate) fn test_channel_id(seed: u8) -> aura_core::types::identifiers::ChannelId {
    test_builders::channel_id(seed)
}

pub(crate) fn test_authority_id(seed: u8) -> aura_core::types::identifiers::AuthorityId {
    test_builders::authority_id(seed)
}

pub(crate) fn test_group_id(seed: u8) -> ChatGroupId {
    ChatGroupId::from_uuid(Uuid::from_bytes([seed; 16]))
}

pub(crate) fn test_timestamp_ms(ts_ms: u64) -> TimeStamp {
    test_builders::timestamp_ms(ts_ms)
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
