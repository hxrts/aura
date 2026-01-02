use aura_authorization::{
    BiscuitAuthorizationBridge, BiscuitTokenManager, ContextOp, ResourceScope, TokenAuthority,
};
use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::scope::AuthorizationOp;

#[test]
fn attenuated_read_token_blocks_write() {
    let issuer = AuthorityId::new_from_entropy([1u8; 32]);
    let recipient = AuthorityId::new_from_entropy([2u8; 32]);
    let context_id = ContextId::new_from_entropy([3u8; 32]);

    let authority = TokenAuthority::new(issuer);
    let token = authority
        .create_token(recipient)
        .expect("token creation should succeed");
    let manager = BiscuitTokenManager::new(recipient, token.clone());

    let attenuated = manager
        .attenuate_read("/context/")
        .expect("attenuation should succeed");

    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), recipient);
    let scope = ResourceScope::Context {
        context_id,
        operation: ContextOp::AddBinding,
    };

    let base_write = bridge
        .authorize(&token, AuthorizationOp::Write, &scope)
        .expect("base token should authorize");
    assert!(base_write.authorized);

    let read_result = bridge
        .authorize(&attenuated, AuthorizationOp::Read, &scope)
        .expect("attenuated token should authorize read");
    assert!(read_result.authorized);

    let write_result = bridge
        .authorize(&attenuated, AuthorizationOp::Write, &scope)
        .expect("attenuated token should evaluate");
    assert!(!write_result.authorized);
}
