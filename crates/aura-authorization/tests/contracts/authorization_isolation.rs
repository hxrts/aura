//! Authorization isolation and policy evaluation contracts.
//!
//! These tests verify security-critical properties: cross-authority tokens
//! must be rejected, policy rules must deny unauthorized scopes, and
//! budget exhaustion must be enforced.

use super::common;
use aura_authorization::biscuit_evaluator::{BiscuitAuthorizationBridge, VerifiedBiscuitToken};
use aura_authorization::{BiscuitTokenManager, TokenAuthority};
use aura_core::types::scope::{AuthorityOp, AuthorizationOp, ResourceScope, StoragePath};

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
    let result = VerifiedBiscuitToken::from_token(&token, authority_b.root_public_key()).and_then(
        |verified| {
            bridge_b.authorize_with_time(&verified, AuthorizationOp::Read, &scope, Some(1_000))
        },
    );
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
    let verified = common::verified_token(&token, keypair.public());
    let scope = common::context_scope(6);

    // Write requires capability("write") — token doesn't have it
    let result = bridge
        .authorize_with_time(&verified, AuthorizationOp::Write, &scope, Some(1_000))
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
    let scoped_context = common::context_id(8).to_string();
    builder
        .add_fact(biscuit_auth::macros::fact!(
            "scope_context({scoped_context})"
        ))
        .unwrap_or_else(|err| panic!("failed to add context scope fact: {err:?}"));
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build read-capability token: {err:?}"));

    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), authority_id);
    let verified = common::verified_token(&token, keypair.public());
    let scope = common::context_scope(8);

    let read_result = bridge
        .authorize_with_time(&verified, AuthorizationOp::Read, &scope, Some(1_000))
        .unwrap_or_else(|err| panic!("read authorization evaluation failed: {err:?}"));
    assert!(read_result.authorized, "read capability should allow read");

    let write_result = bridge
        .authorize_with_time(&verified, AuthorizationOp::Write, &scope, Some(1_000))
        .unwrap_or_else(|err| panic!("write authorization evaluation failed: {err:?}"));
    assert!(
        !write_result.authorized,
        "read capability must NOT allow write"
    );
}

#[test]
fn authority_scoped_token_cannot_authorize_another_authority() {
    let issuer = common::authority_id(40);
    let authority = TokenAuthority::new(issuer);
    let scoped_authority = common::authority_id(41);
    let other_authority = common::authority_id(42);
    let token = authority
        .create_token(scoped_authority, common::read_capability())
        .unwrap_or_else(|err| panic!("failed to create authority-scoped token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), scoped_authority);
    let verified = common::verified_token(&token, authority.root_public_key());

    let allowed_scope = ResourceScope::Authority {
        authority_id: scoped_authority,
        operation: AuthorityOp::UpdateTree,
    };
    let denied_scope = ResourceScope::Authority {
        authority_id: other_authority,
        operation: AuthorityOp::UpdateTree,
    };

    assert!(
        bridge
            .authorize_with_time(
                &verified,
                AuthorizationOp::Read,
                &allowed_scope,
                Some(1_000)
            )
            .unwrap()
            .authorized
    );
    assert!(
        !bridge
            .authorize_with_time(&verified, AuthorizationOp::Read, &denied_scope, Some(1_000))
            .unwrap()
            .authorized,
        "authority-scoped token must not authorize another authority"
    );
}

#[test]
fn context_scoped_token_cannot_authorize_another_context() {
    let issuer = common::authority_id(43);
    let recipient = common::authority_id(44);
    let authority = TokenAuthority::new(issuer);
    let allowed_context = common::context_id(45);
    let token = authority
        .create_context_token(recipient, allowed_context, common::read_capability())
        .unwrap_or_else(|err| panic!("failed to create context-scoped token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), recipient);
    let verified = common::verified_token(&token, authority.root_public_key());

    assert!(
        bridge
            .authorize_with_time(
                &verified,
                AuthorizationOp::Read,
                &common::context_scope(45),
                Some(1_000)
            )
            .unwrap()
            .authorized
    );
    assert!(
        !bridge
            .authorize_with_time(
                &verified,
                AuthorizationOp::Read,
                &common::context_scope(46),
                Some(1_000)
            )
            .unwrap()
            .authorized,
        "context-scoped token must not authorize another context"
    );
}

#[test]
fn authority_context_root_token_authorizes_only_recipient_authority_contexts() {
    let issuer = common::authority_id(53);
    let recipient = common::authority_id(54);
    let other_authority = common::authority_id(55);
    let authority = TokenAuthority::new(issuer);
    let token = authority
        .create_token(recipient, common::read_capability())
        .unwrap_or_else(|err| panic!("failed to create authority-context root token: {err:?}"));
    let verified = common::verified_token(&token, authority.root_public_key());
    let recipient_bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), recipient);
    let other_bridge =
        BiscuitAuthorizationBridge::new(authority.root_public_key(), other_authority);

    assert!(
        recipient_bridge
            .authorize_with_time(
                &verified,
                AuthorizationOp::Read,
                &common::context_scope(56),
                Some(1_000),
            )
            .unwrap()
            .authorized,
        "authority-context root token should authorize recipient contexts"
    );
    assert!(
        !other_bridge
            .authorize_with_time(
                &verified,
                AuthorizationOp::Read,
                &common::context_scope(56),
                Some(1_000),
            )
            .unwrap()
            .authorized,
        "authority-context root token must not authorize another authority's context evaluator"
    );
}

#[test]
fn storage_scoped_token_cannot_authorize_another_path_or_authority() {
    let issuer = common::authority_id(47);
    let recipient = common::authority_id(48);
    let other_authority = common::authority_id(49);
    let authority = TokenAuthority::new(issuer);
    let allowed_path = StoragePath::parse("vault/channel-a").unwrap();
    let denied_path = StoragePath::parse("vault/channel-b").unwrap();
    let token = authority
        .create_storage_token(recipient, &allowed_path, common::read_capability())
        .unwrap_or_else(|err| panic!("failed to create storage-scoped token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(authority.root_public_key(), recipient);
    let verified = common::verified_token(&token, authority.root_public_key());
    let allowed_scope = ResourceScope::Storage {
        authority_id: recipient,
        path: allowed_path,
    };
    let denied_path_scope = ResourceScope::Storage {
        authority_id: recipient,
        path: denied_path,
    };
    let denied_authority_scope = ResourceScope::Storage {
        authority_id: other_authority,
        path: StoragePath::parse("vault/channel-a").unwrap(),
    };

    assert!(
        bridge
            .authorize_with_time(
                &verified,
                AuthorizationOp::Read,
                &allowed_scope,
                Some(1_000)
            )
            .unwrap()
            .authorized
    );
    assert!(
        !bridge
            .authorize_with_time(
                &verified,
                AuthorizationOp::Read,
                &denied_path_scope,
                Some(1_000)
            )
            .unwrap()
            .authorized,
        "storage-scoped token must not authorize another path"
    );
    assert!(
        !bridge
            .authorize_with_time(
                &verified,
                AuthorizationOp::Read,
                &denied_authority_scope,
                Some(1_000)
            )
            .unwrap()
            .authorized,
        "storage-scoped token must not authorize another authority"
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
    let read_only_verified = common::verified_token(&read_only, authority.root_public_key());

    let write_check = bridge
        .authorize_with_time(
            &read_only_verified,
            AuthorizationOp::Write,
            &scope,
            Some(1_000),
        )
        .unwrap_or_else(|err| panic!("write evaluation for read-only token failed: {err:?}"));
    assert!(!write_check.authorized, "read-only token must block write");

    // Second attenuation of the already-restricted token cannot restore write
    let manager2 = BiscuitTokenManager::new(recipient, read_only);
    let double_attenuated = manager2
        .attenuate_read("/")
        .unwrap_or_else(|err| panic!("failed to attenuate read-only token again: {err:?}"));
    let double_attenuated_verified =
        common::verified_token(&double_attenuated, authority.root_public_key());

    let write_check2 = bridge
        .authorize_with_time(
            &double_attenuated_verified,
            AuthorizationOp::Write,
            &scope,
            Some(1_000),
        )
        .unwrap_or_else(|err| {
            panic!("write evaluation for doubly attenuated token failed: {err:?}")
        });
    assert!(
        !write_check2.authorized,
        "double attenuation must not restore write capability"
    );
}

/// Rotating the authority root key for the same authority id must invalidate
/// tokens signed under the previous epoch/key material.
#[test]
fn authority_epoch_rotation_revokes_previous_tokens() {
    let authority_id = common::authority_id(30);
    let recipient = common::authority_id(31);

    let previous_epoch = TokenAuthority::new(authority_id);
    let token = previous_epoch
        .create_token(recipient, common::read_capability())
        .unwrap_or_else(|err| panic!("failed to create pre-rotation token: {err:?}"));

    let rotated_epoch = TokenAuthority::new(authority_id);
    let rotated_bridge =
        BiscuitAuthorizationBridge::new(rotated_epoch.root_public_key(), authority_id);

    let result = VerifiedBiscuitToken::from_token(&token, rotated_epoch.root_public_key())
        .and_then(|verified| {
            rotated_bridge.authorize_with_time(
                &verified,
                AuthorizationOp::Read,
                &common::context_scope(32),
                Some(1_000),
            )
        });
    assert!(
        result.is_err(),
        "rotated authority key must reject tokens issued before the epoch change"
    );
}
