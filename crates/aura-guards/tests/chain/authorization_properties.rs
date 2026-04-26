//! Property-based tests for authorization security contracts.
//!
//! These tests verify the key invariants of the capability authorization
//! system using proptest to exercise edge cases and boundary conditions.
#![allow(missing_docs)]

use super::support::test_authority;
use aura_authorization::VerifiedBiscuitToken;
use aura_core::types::scope::{AuthorityOp, ResourceScope};
use aura_core::CapabilityName;
use aura_guards::authorization::BiscuitAuthorizationBridge;
use aura_guards::CapabilityId;
use biscuit_auth::macros::*;
use proptest::prelude::*;

// ============================================================================
// Strategies
// ============================================================================

/// Strategy for valid capability name characters.
fn valid_cap_char() -> BoxedStrategy<char> {
    prop_oneof![
        Just('a'),
        Just('b'),
        Just('c'),
        Just('d'),
        Just('e'),
        Just('f'),
        Just('g'),
        Just('h'),
        Just('i'),
        Just('j'),
        Just('k'),
        Just('l'),
        Just('m'),
        Just('n'),
        Just('o'),
        Just('p'),
        Just('0'),
        Just('1'),
        Just('9'),
        Just('_'),
        Just('-'),
        Just(':'),
    ]
    .boxed()
}

/// Strategy for valid capability names (1-32 chars from the allowed set).
fn valid_cap_name_strategy() -> BoxedStrategy<String> {
    proptest::collection::vec(valid_cap_char(), 1..32)
        .prop_map(|chars| chars.into_iter().collect::<String>())
        .boxed()
}

/// Strategy for simple lowercase alpha names (for namespace/action pairs).
fn alpha_name(min: usize, max: usize) -> BoxedStrategy<String> {
    proptest::collection::vec(
        prop_oneof![
            Just('a'),
            Just('b'),
            Just('c'),
            Just('d'),
            Just('e'),
            Just('f'),
            Just('g'),
            Just('h'),
        ],
        min..max,
    )
    .prop_map(|chars| chars.into_iter().collect::<String>())
    .boxed()
}

/// Strategy for namespaced capability names (namespace:action format).
fn namespaced_cap_strategy() -> BoxedStrategy<String> {
    (alpha_name(1, 8), alpha_name(1, 8))
        .prop_map(|(ns, action)| format!("{ns}:{action}"))
        .boxed()
}

/// Strategy for characters outside the valid capability set.
fn invalid_cap_char() -> BoxedStrategy<char> {
    prop_oneof![
        Just('A'),
        Just('Z'),
        Just(' '),
        Just('"'),
        Just('\''),
        Just('\\'),
        Just(';'),
        Just('('),
        Just(')'),
        Just('\n'),
        Just('\0'),
    ]
    .boxed()
}

fn test_scope() -> ResourceScope {
    ResourceScope::Authority {
        authority_id: test_authority(42),
        operation: AuthorityOp::UpdateTree,
    }
}

fn bridge_with_caps(capabilities: &[&str]) -> (BiscuitAuthorizationBridge, VerifiedBiscuitToken) {
    let keypair = biscuit_auth::KeyPair::new();
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    let scope_authority = test_authority(42).to_string();
    builder
        .add_fact(fact!("scope_authority({scope_authority})"))
        .unwrap_or_else(|err| panic!("failed to add authority scope fact: {err:?}"));
    for cap in capabilities {
        let cap_str: &str = cap;
        builder
            .add_fact(fact!("capability({cap_str})"))
            .unwrap_or_else(|err| panic!("failed to add capability fact {cap_str}: {err:?}"));
    }
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), test_authority(42));
    let token = VerifiedBiscuitToken::from_token(&token, keypair.public())
        .unwrap_or_else(|err| panic!("failed to verify token: {err:?}"));
    (bridge, token)
}

fn bridge_with_caps_owned(
    capabilities: &[String],
) -> (
    biscuit_auth::KeyPair,
    BiscuitAuthorizationBridge,
    VerifiedBiscuitToken,
) {
    let keypair = biscuit_auth::KeyPair::new();
    let mut builder = biscuit_auth::builder::BiscuitBuilder::new();
    let scope_authority = test_authority(42).to_string();
    builder
        .add_fact(fact!("scope_authority({scope_authority})"))
        .unwrap_or_else(|err| panic!("failed to add authority scope fact: {err:?}"));
    for cap in capabilities {
        let cap_str: &str = cap.as_str();
        builder
            .add_fact(fact!("capability({cap_str})"))
            .unwrap_or_else(|err| panic!("failed to add capability fact {cap_str}: {err:?}"));
    }
    let token = builder
        .build(&keypair)
        .unwrap_or_else(|err| panic!("failed to build token: {err:?}"));
    let bridge = BiscuitAuthorizationBridge::new(keypair.public(), test_authority(42));
    let token = VerifiedBiscuitToken::from_token(&token, keypair.public())
        .unwrap_or_else(|err| panic!("failed to verify token: {err:?}"));
    (keypair, bridge, token)
}

// ============================================================================
// Contract 1: Capability name validation charset
//
// Security property: only the sanctioned character set reaches Datalog.
// ============================================================================

proptest! {
    /// Any string built from valid characters passes validation.
    #[test]
    fn valid_capability_names_always_pass(name in valid_cap_name_strategy()) {
        prop_assert!(
            CapabilityName::parse(&name).is_ok(),
            "valid capability name rejected: {:?}",
            name
        );
    }

    /// A string containing any invalid character is rejected.
    #[test]
    fn invalid_chars_always_rejected(
        prefix in alpha_name(0, 4),
        bad_char in invalid_cap_char(),
        suffix in alpha_name(0, 4),
    ) {
        let name: String = format!("{prefix}{bad_char}{suffix}");
        prop_assert!(
            CapabilityName::parse(&name).is_err(),
            "invalid capability name accepted: {:?}",
            name
        );
    }
}

/// Empty strings are always rejected.
#[test]
fn empty_capability_always_rejected() {
    assert!(CapabilityName::parse("").is_err());
}

// ============================================================================
// Contract 2: CapabilityId validation
//
// Security property: invalid or mixed-case names are rejected rather than
// normalized into a different capability.
// ============================================================================

proptest! {
    /// Mixed-case capability names are rejected.
    #[test]
    fn capability_id_rejects_mixed_case(
        a in alpha_name(1, 8),
        b in alpha_name(1, 8),
    ) {
        let mixed = format!("{}:{}", a.to_uppercase(), b);
        prop_assert!(
            CapabilityId::try_from(mixed.clone()).is_err(),
            "CapabilityId({mixed:?}) should be rejected",
        );
    }

    /// Uppercase capability names are rejected.
    #[test]
    fn capability_id_rejects_uppercase(name in alpha_name(1, 16)) {
        let upper = name.to_uppercase();
        prop_assert!(
            CapabilityId::try_from(upper.clone()).is_err(),
            "CapabilityId should reject uppercase input, got: {upper:?}",
        );
    }
}

// ============================================================================
// Contract 3: Namespaced authorization isolation
//
// Security property: a token with capability X:Y authorizes only X:Y,
// not X:Z or Y:X.
// ============================================================================

proptest! {
    /// A token with capability X:Y authorizes operation X:Y.
    #[test]
    fn token_authorizes_matching_capability(cap in namespaced_cap_strategy()) {
        let cap_ref = cap.as_str();
        let (bridge, token) = bridge_with_caps(&[cap_ref]);
        let result = bridge.has_capability(&token, cap_ref, 1000);
        prop_assert!(
            result.unwrap_or_else(|err| panic!("check failed: {err:?}")),
            "token with {:?} should authorize {:?}",
            cap,
            cap
        );
    }

    /// A token with capability ns_a:action does NOT authorize ns_b:action
    /// (different namespace).
    #[test]
    fn token_denies_different_namespace(
        ns_a in alpha_name(1, 4),
        ns_b in alpha_name(5, 8),
        action in alpha_name(1, 4),
    ) {
        let cap_granted = format!("{ns_a}:{action}");
        let cap_checked = format!("{ns_b}:{action}");

        let (bridge, token) = bridge_with_caps(&[&cap_granted]);
        let result = bridge.has_capability(&token, &cap_checked, 1000);
        prop_assert!(
            !result.unwrap_or_else(|err| panic!("check failed: {err:?}")),
            "token with {:?} should NOT authorize {:?}",
            cap_granted,
            cap_checked
        );
    }

    /// A token with capability ns:action_a does NOT authorize ns:action_b.
    #[test]
    fn token_denies_different_action_same_namespace(
        ns in alpha_name(1, 4),
        action_a in alpha_name(1, 4),
        action_b in alpha_name(5, 8),
    ) {
        let cap_granted = format!("{ns}:{action_a}");
        let cap_checked = format!("{ns}:{action_b}");

        let (bridge, token) = bridge_with_caps(&[&cap_granted]);
        let result = bridge.has_capability(&token, &cap_checked, 1000);
        prop_assert!(
            !result.unwrap_or_else(|err| panic!("check failed: {err:?}")),
            "token with {:?} should NOT authorize {:?}",
            cap_granted,
            cap_checked
        );
    }
}

// ============================================================================
// Contract 4: No capability escalation through authorize()
//
// Security property: a token without capability X cannot pass authorize(X).
// ============================================================================

proptest! {
    /// A token without a specific namespaced capability is denied
    /// when authorize() is called with that operation.
    #[test]
    fn authorize_denies_missing_namespaced_capability(cap in namespaced_cap_strategy()) {
        // Token with only "read" — no namespaced capabilities
        let (bridge, token) = bridge_with_caps(&["read"]);
        let result = bridge
            .authorize(&token, &cap, &test_scope(), 1000)
            .unwrap_or_else(|err| panic!("authorize failed: {err:?}"));
        prop_assert!(
            !result.authorized,
            "token without {:?} should be denied by authorize()",
            cap
        );
    }

    /// A token with a namespaced capability passes authorize() for
    /// that exact operation.
    #[test]
    fn authorize_passes_matching_namespaced_capability(cap in namespaced_cap_strategy()) {
        let cap_ref = cap.as_str();
        let (bridge, token) = bridge_with_caps(&[cap_ref]);
        let result = bridge
            .authorize(&token, &cap, &test_scope(), 1000)
            .unwrap_or_else(|err| panic!("authorize failed: {err:?}"));
        prop_assert!(
            result.authorized,
            "token with {:?} should be authorized by authorize()",
            cap
        );
    }
}

// ============================================================================
// Contract 5: Validate-then-evaluate consistency
//
// Security property: any name that passes validation can be evaluated
// without internal error. No validated input crashes Datalog.
// ============================================================================

proptest! {
    /// Any capability name that passes validation can be used in Datalog
    /// evaluation without causing an internal error.
    #[test]
    fn validated_names_evaluate_without_error(name in valid_cap_name_strategy()) {
        if CapabilityName::parse(&name).is_ok() {
            let (bridge, token) = bridge_with_caps(&[]);
            let result = bridge.has_capability(&token, &name, 1000);
            prop_assert!(
                result.is_ok(),
                "validated name {:?} caused evaluation error: {:?}",
                name,
                result.err()
            );
        }
    }
}

// ============================================================================
// Contract 6: Capability attenuation monotonicity
//
// Security property: attenuating a token (adding restriction blocks)
// can only reduce, never expand, its authorized capability set.
// ============================================================================

proptest! {
    /// A token with capability A but NOT capability B cannot authorize
    /// operation B through authorize() — even if B is a valid operation.
    /// This is the fundamental non-escalation property.
    #[test]
    fn attenuation_via_reduced_token(
        cap_a in namespaced_cap_strategy(),
        cap_b in namespaced_cap_strategy(),
    ) {
        prop_assume!(cap_a != cap_b);

        // Build a token with ONLY cap_a
        let (_, bridge, token) = bridge_with_caps_owned(std::slice::from_ref(&cap_a));

        // authorize() with cap_a passes
        let result_a = bridge
            .authorize(&token, &cap_a, &test_scope(), 1000)
            .unwrap();
        prop_assert!(result_a.authorized, "token with {:?} should authorize {:?}", cap_a, cap_a);

        // authorize() with cap_b (not in token) is denied
        let result_b = bridge
            .authorize(&token, &cap_b, &test_scope(), 1000)
            .unwrap();
        prop_assert!(
            !result_b.authorized,
            "token with only {:?} should NOT authorize {:?}",
            cap_a,
            cap_b
        );
    }
}
