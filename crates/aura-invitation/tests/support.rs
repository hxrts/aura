#![allow(missing_docs)]

use aura_core::types::identifiers::{AuthorityId, ContextId};
use aura_core::FlowCost;
use aura_invitation::{capabilities::InvitationCapability, guards::GuardSnapshot};

pub fn test_context(seed: u8) -> ContextId {
    ContextId::new_from_entropy([seed; 32])
}

pub fn snapshot_with_caps(
    authority_id: AuthorityId,
    context_id: ContextId,
    capabilities: &[InvitationCapability],
    flow_budget: u32,
    now_ms: u64,
) -> GuardSnapshot {
    GuardSnapshot::new(
        authority_id,
        context_id,
        FlowCost::new(flow_budget),
        capabilities
            .iter()
            .map(InvitationCapability::as_name)
            .collect(),
        0,
        now_ms,
    )
}
