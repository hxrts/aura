use aura_authorization::{ContextOp, ResourceScope, TokenAuthority};
use aura_core::{
    capability_name,
    types::identifiers::{AuthorityId, ContextId},
    CapabilityName,
};

pub fn authority_id(seed: u8) -> AuthorityId {
    AuthorityId::new_from_entropy([seed; 32])
}

pub fn context_id(seed: u8) -> ContextId {
    ContextId::new_from_entropy([seed; 32])
}

pub fn token_authority(seed: u8) -> TokenAuthority {
    TokenAuthority::new(authority_id(seed))
}

pub fn read_capability() -> Vec<CapabilityName> {
    vec![capability_name!("read")]
}

pub fn read_write_capabilities() -> Vec<CapabilityName> {
    vec![capability_name!("read"), capability_name!("write")]
}

pub fn context_scope(seed: u8) -> ResourceScope {
    ResourceScope::Context {
        context_id: context_id(seed),
        operation: ContextOp::AddBinding,
    }
}
