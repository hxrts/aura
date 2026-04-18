use crate::capabilities::{AuthenticationCapability, GuardianAuthCapability};
use crate::guards::GuardSnapshot;
use aura_core::types::identifiers::AuthorityId;
use aura_core::{DeviceId, FlowCost};
use aura_signature::session::SessionScope;

pub(crate) fn authority(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

pub(crate) fn device(seed: u8) -> DeviceId {
    DeviceId::new_from_entropy([seed; 32])
}

pub(crate) fn context(seed: u8) -> aura_core::types::identifiers::ContextId {
    aura_core::types::identifiers::ContextId::new_from_entropy([seed; 32])
}

pub(crate) fn protocol_scope(protocol_type: &str) -> SessionScope {
    SessionScope::Protocol {
        protocol_type: protocol_type.to_string(),
    }
}

pub(crate) fn snapshot_with_capabilities(
    authority_seed: u8,
    capabilities: Vec<aura_guards::types::CapabilityId>,
) -> GuardSnapshot {
    GuardSnapshot::new(
        authority(authority_seed),
        None,
        None,
        FlowCost::new(100),
        capabilities,
        1,
        1000,
    )
}

pub(crate) fn standard_service_snapshot() -> GuardSnapshot {
    snapshot_with_capabilities(
        1,
        vec![
            AuthenticationCapability::Request.as_name(),
            AuthenticationCapability::SubmitProof.as_name(),
            AuthenticationCapability::CreateSession.as_name(),
            GuardianAuthCapability::RequestApproval.as_name(),
            GuardianAuthCapability::Verify.as_name(),
        ],
    )
}

pub(crate) fn standard_guard_snapshot() -> GuardSnapshot {
    snapshot_with_capabilities(
        1,
        vec![
            AuthenticationCapability::Request.as_name(),
            AuthenticationCapability::SubmitProof.as_name(),
            AuthenticationCapability::Verify.as_name(),
            AuthenticationCapability::CreateSession.as_name(),
        ],
    )
}
