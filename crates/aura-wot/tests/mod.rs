//! Test modules for aura-wot capability system

use aura_core::identifiers::{AuthorityId, DeviceId};
use aura_core::scope::{AuthorityOp, ResourceScope};
use aura_wot::biscuit_authorization::BiscuitAuthorizationBridge;
use biscuit_auth::macros::*;

#[test]
fn biscuit_bridge_authorizes_basic_token() {
    let keypair = biscuit_auth::KeyPair::new();
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    // Add the required capability fact to make authorization succeed
    builder.add_fact(fact!("capability(\"read\")")).unwrap();
    let token = builder.build(&keypair).unwrap();
    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), DeviceId::new());
    let scope = ResourceScope::Authority {
        authority_id: AuthorityId::new(),
        operation: AuthorityOp::UpdateTree,
    };

    let result = bridge.authorize(&token, "read", &scope).unwrap();
    assert!(result.authorized);
}

#[test]
fn biscuit_bridge_extracts_token_facts() {
    let keypair = biscuit_auth::KeyPair::new();
    let builder = biscuit_auth::builder::BiscuitBuilder::new();
    let token = builder.build(&keypair).unwrap();
    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), DeviceId::new());

    let facts = bridge.extract_token_facts_from_blocks(&token);
    assert!(!facts.is_empty());
    assert!(facts.iter().any(|f| f.contains("device(")));
}
