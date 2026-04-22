use aura_authorization::{ContextOp, ResourceScope, TokenAuthority, VerifiedBiscuitToken};
use aura_core::{
    capability_name,
    types::identifiers::{AuthorityId, ContextId},
    CapabilityName,
};
use biscuit_auth::{Biscuit, PublicKey};

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

pub fn verified_token(token: &Biscuit, root_public_key: PublicKey) -> VerifiedBiscuitToken {
    VerifiedBiscuitToken::from_token(token, root_public_key)
        .unwrap_or_else(|err| panic!("failed to verify test token: {err:?}"))
}
