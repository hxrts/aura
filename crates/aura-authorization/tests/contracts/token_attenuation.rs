//! Token attenuation contracts — attenuated tokens must restrict scope,
//! never widen it. If attenuation is not monotone, delegated tokens can
//! gain capabilities the issuer didn't grant.

use aura_authorization::{
    BiscuitAuthorizationBridge, BiscuitTokenManager, ContextOp, ResourceScope, TokenAuthority,
};
use aura_core::types::scope::AuthorizationOp;
use aura_core::{
    capability_name,
    types::identifiers::{AuthorityId, ContextId},
};

/// Read-attenuated token must block write operations — the core monotonicity
/// property. If this fails, delegation can escalate privileges.
#[test]
fn attenuated_read_token_blocks_write() {
    let issuer = AuthorityId::new_from_entropy([1u8; 32]);
    let recipient = AuthorityId::new_from_entropy([2u8; 32]);
    let context_id = ContextId::new_from_entropy([3u8; 32]);

    let authority = TokenAuthority::new(issuer);
    let token = authority
        .create_token(
            recipient,
            vec![capability_name!("read"), capability_name!("write")],
        )
        .unwrap_or_else(|err| panic!("token creation should succeed: {err:?}"));
    let manager = BiscuitTokenManager::new(recipient, token.clone());

    let attenuated = manager
        .attenuate_read("/context/")
        .unwrap_or_else(|err| panic!("attenuation should succeed: {err:?}"));

    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), recipient);
    let scope = ResourceScope::Context {
        context_id,
        operation: ContextOp::AddBinding,
    };

    let base_write = bridge
        .authorize(&token, AuthorizationOp::Write, &scope)
        .unwrap_or_else(|err| panic!("base token authorization should evaluate: {err:?}"));
    assert!(base_write.authorized);

    let read_result = bridge
        .authorize(&attenuated, AuthorizationOp::Read, &scope)
        .unwrap_or_else(|err| panic!("attenuated read authorization should evaluate: {err:?}"));
    assert!(read_result.authorized);

    let write_result = bridge
        .authorize(&attenuated, AuthorizationOp::Write, &scope)
        .unwrap_or_else(|err| panic!("attenuated write authorization should evaluate: {err:?}"));
    assert!(!write_result.authorized);
}
