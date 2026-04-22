//! Token attenuation contracts — attenuated tokens must restrict scope,
//! never widen it. If attenuation is not monotone, delegated tokens can
//! gain capabilities the issuer didn't grant.

use super::common;
use aura_authorization::{BiscuitAuthorizationBridge, BiscuitTokenManager, TokenAuthority};
use aura_core::types::scope::AuthorizationOp;

/// Read-attenuated token must block write operations — the core monotonicity
/// property. If this fails, delegation can escalate privileges.
#[test]
fn attenuated_read_token_blocks_write() {
    let issuer = common::authority_id(1);
    let recipient = common::authority_id(2);
    let authority = TokenAuthority::new(issuer);
    let token = authority
        .create_token(recipient, common::read_write_capabilities())
        .unwrap_or_else(|err| panic!("token creation should succeed: {err:?}"));
    let manager = BiscuitTokenManager::new(recipient, token.clone());

    let attenuated = manager
        .attenuate_read("/context/")
        .unwrap_or_else(|err| panic!("attenuation should succeed: {err:?}"));

    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), recipient);
    let scope = common::context_scope(3);
    let token_verified = common::verified_token(&token, authority.root_public_key());
    let attenuated_verified = common::verified_token(&attenuated, authority.root_public_key());

    let base_write = bridge
        .authorize_with_time(&token_verified, AuthorizationOp::Write, &scope, Some(1_000))
        .unwrap_or_else(|err| panic!("base token authorization should evaluate: {err:?}"));
    assert!(base_write.authorized);

    let read_result = bridge
        .authorize_with_time(
            &attenuated_verified,
            AuthorizationOp::Read,
            &scope,
            Some(1_000),
        )
        .unwrap_or_else(|err| panic!("attenuated read authorization should evaluate: {err:?}"));
    assert!(read_result.authorized);

    let write_result = bridge
        .authorize_with_time(
            &attenuated_verified,
            AuthorizationOp::Write,
            &scope,
            Some(1_000),
        )
        .unwrap_or_else(|err| panic!("attenuated write authorization should evaluate: {err:?}"));
    assert!(!write_result.authorized);
}
