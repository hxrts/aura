//! Authorization isolation and policy evaluation contracts.
//!
//! These tests verify security-critical properties: cross-authority tokens
//! must be rejected, policy rules must deny unauthorized scopes, and
//! budget exhaustion must be enforced.

use super::common;
use aura_authorization::biscuit_evaluator::BiscuitAuthorizationBridge;
use aura_authorization::{BiscuitTokenManager, TokenAuthority};
use aura_core::types::scope::AuthorizationOp;

// ============================================================================
// Cross-authority isolation
// ============================================================================

/// A token issued by authority A must be rejected when evaluated by
/// authority B's bridge. The bridge verifies the token signature against
/// its root public key — a token signed by a different key fails
/// signature verification. If this passes, tokens are portable across
/// trust roots.
#[test]
fn cross_authority_token_rejected() {
    let authority_a = common::token_authority(1);
    let authority_b = common::token_authority(2);
    let recipient = common::authority_id(3);

    let token = authority_a
        .create_token(recipient, common::read_capability())
        .unwrap_or_else(|err| panic!("token creation failed: {err:?}"));

    // Evaluated by B's bridge (different root key)
    let bridge_b =
        BiscuitAuthorizationBridge::new(authority_b.root_public_key(), common::authority_id(2));

    let scope = common::context_scope(10);

    // Authorization must fail — token signed by A's key, verified against B's key
    let result = bridge_b.authorize(&token, AuthorizationOp::Read, &scope);
    assert!(
        result.is_err(),
        "token from authority A must be rejected by authority B's evaluator"
    );
}

// ============================================================================
// Policy evaluation correctness
// ============================================================================

/// A token without the required capability must be denied. The Datalog
/// policy "allow if capability(X)" must not match when the capability
/// fact is absent.
#[test]
fn token_without_capability_denied() {
    let keypair = biscuit_auth::KeyPair::new();
    let authority_id = common::authority_id(5);

    // Build a token with NO capabilities — only basic facts
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    builder
        .add_fact(biscuit_auth::macros::fact!("authority(\"some-authority\")"))
        .unwrap_or_else(|err| panic!("failed to add authority fact: {err:?}"));
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build capability-less token: {err:?}"));

    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), authority_id);
    let scope = common::context_scope(6);

    // Write requires capability("write") — token doesn't have it
    let result = bridge
        .authorize(&token, AuthorizationOp::Write, &scope)
        .unwrap_or_else(|err| panic!("evaluation should succeed even if denied: {err:?}"));
    assert!(
        !result.authorized,
        "token without write capability must be denied for write operations"
    );
}

/// A token with read capability must be denied for write operations —
/// capabilities are not hierarchical, they're explicitly enumerated.
#[test]
fn read_capability_does_not_imply_write() {
    let keypair = biscuit_auth::KeyPair::new();
    let authority_id = common::authority_id(7);

    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    builder
        .add_fact(biscuit_auth::macros::fact!("capability(\"read\")"))
        .unwrap_or_else(|err| panic!("failed to add read capability fact: {err:?}"));
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build read-capability token: {err:?}"));

    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), authority_id);
    let scope = common::context_scope(8);

    let read_result = bridge
        .authorize(&token, AuthorizationOp::Read, &scope)
        .unwrap_or_else(|err| panic!("read authorization evaluation failed: {err:?}"));
    assert!(read_result.authorized, "read capability should allow read");

    let write_result = bridge
        .authorize(&token, AuthorizationOp::Write, &scope)
        .unwrap_or_else(|err| panic!("write authorization evaluation failed: {err:?}"));
    assert!(
        !write_result.authorized,
        "read capability must NOT allow write"
    );
}

// ============================================================================
// Attenuation chain monotonicity
// ============================================================================

/// Attenuating a token twice must not restore capabilities lost in the
/// first attenuation. Attenuation is monotonically restrictive.
#[test]
fn double_attenuation_cannot_restore_capabilities() {
    let issuer = common::authority_id(20);
    let recipient = common::authority_id(21);

    let authority = TokenAuthority::new(issuer);
    let token = authority
        .create_token(recipient, common::read_write_capabilities())
        .unwrap_or_else(|err| panic!("failed to create base token: {err:?}"));

    // First attenuation: restrict to read only
    let manager = BiscuitTokenManager::new(recipient, token);
    let read_only = manager
        .attenuate_read("/")
        .unwrap_or_else(|err| panic!("failed to attenuate token to read-only: {err:?}"));

    // Verify write is blocked
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), recipient);
    let scope = common::context_scope(22);

    let write_check = bridge
        .authorize(&read_only, AuthorizationOp::Write, &scope)
        .unwrap_or_else(|err| panic!("write evaluation for read-only token failed: {err:?}"));
    assert!(!write_check.authorized, "read-only token must block write");

    // Second attenuation of the already-restricted token cannot restore write
    let manager2 = BiscuitTokenManager::new(recipient, read_only);
    let double_attenuated = manager2
        .attenuate_read("/")
        .unwrap_or_else(|err| panic!("failed to attenuate read-only token again: {err:?}"));

    let write_check2 = bridge
        .authorize(&double_attenuated, AuthorizationOp::Write, &scope)
        .unwrap_or_else(|err| {
            panic!("write evaluation for doubly attenuated token failed: {err:?}")
        });
    assert!(
        !write_check2.authorized,
        "double attenuation must not restore write capability"
    );
}
