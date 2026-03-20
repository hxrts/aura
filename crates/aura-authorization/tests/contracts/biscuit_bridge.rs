//! Biscuit bridge contracts — token creation, fact extraction, and
//! basic authorization flow through the BiscuitAuthorizationBridge.

use aura_authorization::biscuit_authorization::BiscuitAuthorizationBridge;
use aura_core::types::identifiers::AuthorityId;
use aura_core::types::scope::{AuthorityOp, AuthorizationOp, ResourceScope};
use biscuit_auth::macros::*;

/// Token with read capability passes authorization — the happy path
/// through the full Biscuit Datalog evaluation pipeline.
#[test]
fn biscuit_bridge_authorizes_basic_token() {
    let keypair = biscuit_auth::KeyPair::new();
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    // Add the required capability fact to make authorization succeed
    builder
        .add_fact(fact!("capability(\"read\")"))
        .unwrap_or_else(|err| panic!("failed to add read capability fact: {err:?}"));
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build read-capability token: {err:?}"));
    let bridge =
        BiscuitAuthorizationBridge::new(keypair.public(), AuthorityId::new_from_entropy([1u8; 32]));
    let scope = ResourceScope::Authority {
        authority_id: AuthorityId::new_from_entropy([70u8; 32]),
        operation: AuthorityOp::UpdateTree,
    };

    let result = bridge
        .authorize(&token, AuthorizationOp::Read, &scope)
        .unwrap_or_else(|err| panic!("bridge authorization failed unexpectedly: {err:?}"));
    assert!(result.authorized);
}

/// Bridge extracts token facts from Biscuit blocks — needed for
/// Datalog policy evaluation to access issuer/authority metadata.
#[test]
fn biscuit_bridge_extracts_token_facts() {
    let keypair = biscuit_auth::KeyPair::new();
    let builder = biscuit_auth::builder::BiscuitBuilder::new();
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build empty biscuit token: {err:?}"));
    let bridge =
        BiscuitAuthorizationBridge::new(keypair.public(), AuthorityId::new_from_entropy([2u8; 32]));

    let facts = bridge.extract_token_facts_from_blocks(&token);
    assert!(!facts.is_empty());
    assert!(facts.iter().any(|f| f.contains("authority(")));
}
