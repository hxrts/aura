//! Biscuit bridge contracts — token creation, fact extraction, and
//! basic authorization flow through the BiscuitAuthorizationBridge.

use super::common;
use aura_authorization::biscuit_evaluator::{BiscuitAuthorizationBridge, VerifiedBiscuitToken};
use aura_authorization::BiscuitError;
use aura_core::capability_name;
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
    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), common::authority_id(1));
    let verified = common::verified_token(&token, keypair.public());
    let scope = ResourceScope::Authority {
        authority_id: common::authority_id(70),
        operation: AuthorityOp::UpdateTree,
    };

    let result = bridge
        .authorize_with_time(&verified, AuthorizationOp::Read, &scope, Some(500))
        .unwrap_or_else(|err| panic!("bridge authorization failed unexpectedly: {err:?}"));
    assert!(result.authorized);
}

#[test]
fn verified_biscuit_token_rejects_wrong_root() {
    let issuer = biscuit_auth::KeyPair::new();
    let wrong_root = biscuit_auth::KeyPair::new();
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    builder
        .add_fact(fact!("capability(\"read\")"))
        .unwrap_or_else(|err| panic!("failed to add read capability fact: {err:?}"));
    let token = builder
        .build(&issuer)
        .unwrap_or_else(|err| panic!("failed to build read-capability token: {err:?}"));
    let token_bytes = token
        .to_vec()
        .unwrap_or_else(|err| panic!("failed to serialize token: {err:?}"));

    assert!(VerifiedBiscuitToken::from_bytes(&token_bytes, issuer.public()).is_ok());
    assert!(VerifiedBiscuitToken::from_bytes(&token_bytes, wrong_root.public()).is_err());
}

#[test]
fn attacker_issued_token_with_matching_facts_is_rejected_by_trusted_root() {
    let trusted_authority = common::token_authority(15);
    let attacker = biscuit_auth::KeyPair::new();
    let recipient = trusted_authority.authority_id().to_string();
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    builder
        .add_fact(fact!("authority({recipient})"))
        .unwrap_or_else(|err| panic!("failed to add authority fact: {err:?}"));
    builder
        .add_fact(fact!("capability(\"read\")"))
        .unwrap_or_else(|err| panic!("failed to add read capability fact: {err:?}"));
    let token = builder
        .build(&attacker)
        .unwrap_or_else(|err| panic!("failed to build attacker token: {err:?}"));

    assert!(
        VerifiedBiscuitToken::from_token(&token, trusted_authority.root_public_key()).is_err(),
        "trusted verifier must reject attacker-issued token even if token facts look correct"
    );
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
    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), common::authority_id(2));
    let verified = VerifiedBiscuitToken::from_token(&token, keypair.public())
        .unwrap_or_else(|err| panic!("failed to verify token for diagnostics: {err:?}"));

    let facts = bridge.extract_diagnostic_token_facts(&verified);
    assert!(!facts.is_empty());
    assert!(facts.iter().any(|f| f.contains("authority(")));
}

#[test]
fn diagnostic_token_facts_cannot_drive_capability_authorization() {
    let keypair = biscuit_auth::KeyPair::new();
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    builder
        .add_fact(fact!("authority(\"diagnostic-only\")"))
        .unwrap_or_else(|err| panic!("failed to add authority fact: {err:?}"));
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build diagnostic-only token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), common::authority_id(16));
    let verified = common::verified_token(&token, keypair.public());

    let mut diagnostic_facts = bridge.extract_diagnostic_token_facts(&verified);
    diagnostic_facts.push("capability(\"read\")".to_string());

    let result = bridge
        .authorize_with_time(
            &verified,
            AuthorizationOp::Read,
            &common::context_scope(16),
            Some(1_000),
        )
        .unwrap_or_else(|err| panic!("authorization evaluation failed unexpectedly: {err:?}"));

    assert!(
        !result.authorized,
        "invented diagnostic facts must not influence Biscuit capability decisions"
    );
}

/// Namespaced capabilities with `:` remain valid Biscuit capability tokens.
#[test]
fn biscuit_bridge_accepts_namespaced_capability_tokens() {
    let authority = common::token_authority(3);
    let recipient = authority.authority_id();
    let token = authority
        .create_token(recipient, vec![capability_name!("invitation:decline")])
        .unwrap_or_else(|err| panic!("failed to build invitation-capability token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), recipient);
    let verified = common::verified_token(&token, authority.root_public_key());

    let allowed = bridge
        .has_capability_with_time(&verified, "invitation:decline", Some(1_000))
        .unwrap_or_else(|err| panic!("namespaced capability lookup failed unexpectedly: {err:?}"));
    assert!(allowed);
}

/// Default member tokens must authorize runtime guard-chain send operations.
#[test]
fn biscuit_bridge_default_member_token_carries_guard_chain_send_capabilities() {
    let authority = common::token_authority(4);
    let recipient = authority.authority_id();
    let token = authority
        .create_token(
            recipient,
            vec![
                capability_name!("flow_charge"),
                capability_name!("amp:send"),
                capability_name!("sync:request_digest"),
                capability_name!("sync:request_ops"),
                capability_name!("sync:push_ops"),
                capability_name!("sync:announce_op"),
                capability_name!("sync:push_op"),
                capability_name!("chat:message:send"),
            ],
        )
        .unwrap_or_else(|err| panic!("failed to build default member token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), recipient);
    let verified = common::verified_token(&token, authority.root_public_key());

    for capability in [
        "flow_charge",
        "amp:send",
        "sync:request_digest",
        "sync:request_ops",
        "sync:push_ops",
        "sync:announce_op",
        "sync:push_op",
        "chat:message:send",
    ] {
        let allowed = bridge
            .has_capability_with_time(&verified, capability, Some(1_000))
            .unwrap_or_else(|err| {
                panic!("default member token lookup failed for {capability}: {err:?}")
            });
        assert!(allowed, "default member token must allow {capability}");
    }
}

/// Omitting time must fail closed so callers cannot accidentally bypass token
/// expiry checks by falling back to epoch 0.
#[test]
fn biscuit_bridge_requires_explicit_time() {
    let keypair = biscuit_auth::KeyPair::new();
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    builder
        .add_fact(fact!("capability(\"read\")"))
        .unwrap_or_else(|err| panic!("failed to add read capability fact: {err:?}"));
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build read-capability token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), common::authority_id(5));
    let verified = common::verified_token(&token, keypair.public());

    let error = match bridge.authorize_with_time(
        &verified,
        AuthorizationOp::Read,
        &common::context_scope(11),
        None,
    ) {
        Ok(_) => panic!("authorization without time must fail closed"),
        Err(error) => error,
    };

    assert!(matches!(error, BiscuitError::TimeRequired));
}

/// Time-bound Biscuit checks must reject expired tokens once the evaluator is
/// given a real current-time value.
#[test]
fn biscuit_bridge_rejects_expired_token_with_explicit_time() {
    let keypair = biscuit_auth::KeyPair::new();
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    builder
        .add_fact(fact!("capability(\"read\")"))
        .unwrap_or_else(|err| panic!("failed to add read capability fact: {err:?}"));
    builder
        .add_check(check!("check if time($time), $time < 1000"))
        .unwrap_or_else(|err| panic!("failed to add expiry check: {err:?}"));
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build expiring token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), common::authority_id(6));
    let verified = common::verified_token(&token, keypair.public());
    let scope = common::context_scope(12);

    let before_expiry = bridge
        .authorize_with_time(&verified, AuthorizationOp::Read, &scope, Some(999))
        .unwrap_or_else(|err| panic!("pre-expiry authorization failed unexpectedly: {err:?}"));
    assert!(before_expiry.authorized);

    let after_expiry = bridge
        .authorize_with_time(&verified, AuthorizationOp::Read, &scope, Some(1_500))
        .unwrap_or_else(|err| panic!("post-expiry authorization failed unexpectedly: {err:?}"));
    assert!(!after_expiry.authorized);
}
