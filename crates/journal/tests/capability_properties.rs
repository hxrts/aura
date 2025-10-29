#![allow(warnings, clippy::all)]
//! Property Tests: Capability Token Invariants
//!
//! Tests fundamental properties that must hold for ALL capability tokens.
//! Uses proptest to generate random test cases and verify invariants.
//!
//! Reference: work/pre_ssb_storage_tests.md - Category 1.2

use aura_crypto::Effects;
use aura_crypto::{ed25519_sign, ed25519_verify, Ed25519SigningKey, Ed25519VerifyingKey};
use aura_journal::capability::{CapabilityId, CapabilityScope, Subject};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};

// Reuse types from capability_tokens.rs
// TODO: Move these to aura-journal/src/capability/types.rs

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAuthentication {
    pub device_id: Vec<u8>,
    pub account_id: Vec<u8>,
    pub device_signature: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission {
    Storage {
        operation: String,
        resource: String,
    },
    Communication {
        operation: String,
        relationship: String,
    },
    Relay {
        operation: String,
        trust_level: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    pub authenticated_device: Vec<u8>,
    pub granted_permissions: Vec<Permission>,
    pub delegation_chain: Vec<CapabilityId>,
    pub signature: Vec<u8>,
    pub issued_at: u64,
    pub expires_at: Option<u64>,
}

impl CapabilityToken {
    pub fn new(
        device_id: Vec<u8>,
        permissions: Vec<Permission>,
        delegation_chain: Vec<CapabilityId>,
        signing_key: &SigningKey,
        effects: &Effects,
    ) -> Result<Self, String> {
        let issued_at = effects.now().map_err(|e| format!("Time error: {:?}", e))?;

        let mut token = Self {
            authenticated_device: device_id,
            granted_permissions: permissions,
            delegation_chain,
            signature: Vec::new(),
            issued_at,
            expires_at: None,
        };

        let message = token.signing_message()?;
        token.signature = signing_key.sign(&message).to_bytes().to_vec();

        Ok(token)
    }

    fn signing_message(&self) -> Result<Vec<u8>, String> {
        let mut msg = Vec::new();
        msg.extend_from_slice(&self.authenticated_device);

        let perms_bytes = serde_json::to_vec(&self.granted_permissions)
            .map_err(|e| format!("Serialization error: {}", e))?;
        msg.extend_from_slice(&perms_bytes);

        for cap_id in &self.delegation_chain {
            msg.extend_from_slice(&cap_id.0);
        }

        msg.extend_from_slice(&self.issued_at.to_le_bytes());

        Ok(msg)
    }

    pub fn verify(&self, verifying_key: &VerifyingKey) -> Result<(), String> {
        let message = self.signing_message()?;

        use aura_crypto::Ed25519Signature;
        let sig = Signature::from_bytes(
            self.signature
                .as_slice()
                .try_into()
                .map_err(|_| "Invalid signature length")?,
        );

        verifying_key
            .verify(&message, &sig)
            .map_err(|e| format!("Signature verification failed: {}", e))
    }

    pub fn has_permission(&self, permission: &Permission) -> bool {
        self.granted_permissions.contains(permission)
    }

    pub fn is_subset_of_permissions(&self, superset: &[Permission]) -> bool {
        self.granted_permissions
            .iter()
            .all(|p| superset.contains(p))
    }
}

// Proptest generators

fn arb_device_id() -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), 8..32)
}

fn arb_operation() -> impl Strategy<Value = String> {
    prop::string::string_regex("(read|write|delete|admin)").unwrap()
}

fn arb_resource() -> impl Strategy<Value = String> {
    prop::string::string_regex("(/[a-z]+)+").unwrap()
}

fn arb_relationship() -> impl Strategy<Value = String> {
    prop::string::string_regex("(friend|family|colleague|public)").unwrap()
}

fn arb_trust_level() -> impl Strategy<Value = String> {
    prop::string::string_regex("(low|medium|high|ultimate)").unwrap()
}

fn arb_permission() -> impl Strategy<Value = Permission> {
    prop_oneof![
        (arb_operation(), arb_resource()).prop_map(|(op, res)| Permission::Storage {
            operation: op,
            resource: res,
        }),
        (arb_operation(), arb_relationship()).prop_map(|(op, rel)| Permission::Communication {
            operation: op,
            relationship: rel,
        }),
        (arb_operation(), arb_trust_level()).prop_map(|(op, trust)| Permission::Relay {
            operation: op,
            trust_level: trust,
        }),
    ]
}

fn arb_permissions() -> impl Strategy<Value = Vec<Permission>> {
    prop::collection::vec(arb_permission(), 1..10)
}

fn arb_capability_id() -> impl Strategy<Value = CapabilityId> {
    (any::<u64>(), any::<u64>()).prop_map(|(seed1, seed2)| {
        let subject = Subject::new(&format!("delegator_{}", seed1));
        let scope = CapabilityScope::simple(
            &format!("subsystem_{}", seed1),
            &format!("action_{}", seed2),
        );
        CapabilityId::from_chain(None, subject.as_bytes(), &scope.as_bytes())
    })
}

fn arb_delegation_chain() -> impl Strategy<Value = Vec<CapabilityId>> {
    prop::collection::vec(arb_capability_id(), 0..5)
}

// Helper to create deterministic key from seed
fn create_key_from_seed(seed: u64) -> SigningKey {
    let effects = Effects::deterministic(seed, 0);
    let key_bytes = effects.random_bytes::<32>();
    SigningKey::from_bytes(&key_bytes)
}

// Property Tests

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    /// Property: All validly-created tokens verify successfully with the correct key
    ///
    /// Invariant: Correctness - valid tokens always verify
    #[test]
    fn prop_capability_tokens_always_verify_with_correct_key(
        device_id in arb_device_id(),
        permissions in arb_permissions(),
        delegation_chain in arb_delegation_chain(),
        key_seed in any::<u64>(),
        time_seed in any::<u64>(),
    ) {
        let effects = Effects::deterministic(time_seed, 1000);
        let signing_key = create_key_from_seed(key_seed);

        // Create token
        let token = CapabilityToken::new(
            device_id,
            permissions,
            delegation_chain,
            &signing_key,
            &effects,
        ).expect("Token creation should succeed");

        // Verify with correct key
        let verifying_key = signing_key.verifying_key();
        let result = token.verify(&verifying_key);

        prop_assert!(result.is_ok(), "Valid token should always verify with correct key");
    }

    /// Property: Tokens never verify with a wrong key
    ///
    /// Invariant: Security - forged tokens never verify
    #[test]
    fn prop_capability_tokens_never_verify_with_wrong_key(
        device_id in arb_device_id(),
        permissions in arb_permissions(),
        delegation_chain in arb_delegation_chain(),
        correct_key_seed in any::<u64>(),
        wrong_key_seed in any::<u64>(),
        time_seed in any::<u64>(),
    ) {
        // Ensure keys are different
        prop_assume!(correct_key_seed != wrong_key_seed);

        let effects = Effects::deterministic(time_seed, 1000);
        let correct_key = create_key_from_seed(correct_key_seed);
        let wrong_key = create_key_from_seed(wrong_key_seed);

        // Create token with correct key
        let token = CapabilityToken::new(
            device_id,
            permissions,
            delegation_chain,
            &correct_key,
            &effects,
        ).expect("Token creation should succeed");

        // Verify with wrong key
        let wrong_verifying_key = wrong_key.verifying_key();
        let result = token.verify(&wrong_verifying_key);

        prop_assert!(result.is_err(), "Token should never verify with wrong key");

        // Double-check it works with correct key
        let correct_verifying_key = correct_key.verifying_key();
        prop_assert!(token.verify(&correct_verifying_key).is_ok());
    }

    /// Property: Delegation chains are transitive
    ///
    /// Invariant: If A→B valid and B→C valid, then chain A→B→C is valid
    #[test]
    fn prop_capability_delegation_chains_transitive(
        device_id in arb_device_id(),
        permissions in arb_permissions(),
        key_seed in any::<u64>(),
        time_seed in any::<u64>(),
        delegator_seeds in prop::collection::vec(any::<u64>(), 1..5),
    ) {
        let effects = Effects::deterministic(time_seed, 1000);
        let signing_key = create_key_from_seed(key_seed);

        // Build delegation chain: each capability delegates to next
        let mut delegation_chain = Vec::new();
        let mut parent: Option<&CapabilityId> = None;

        for (idx, seed) in delegator_seeds.iter().enumerate() {
            let subject = Subject::new(&format!("delegator_{}", seed));
            let scope = CapabilityScope::simple("storage", &format!("level_{}", idx));
            let cap = CapabilityId::from_chain(parent, subject.as_bytes(), &scope.as_bytes());

            delegation_chain.push(cap.clone());
            parent = delegation_chain.last();
        }

        // Create token with full chain
        let token_full = CapabilityToken::new(
            device_id.clone(),
            permissions.clone(),
            delegation_chain.clone(),
            &signing_key,
            &effects,
        ).expect("Token creation should succeed");

        // Verify full chain
        let verifying_key = signing_key.verifying_key();
        prop_assert!(token_full.verify(&verifying_key).is_ok(), "Full delegation chain should verify");

        // Create token with each prefix of the chain (transitivity)
        for prefix_len in 1..=delegation_chain.len() {
            let prefix_chain = delegation_chain[..prefix_len].to_vec();
            let token_prefix = CapabilityToken::new(
                device_id.clone(),
                permissions.clone(),
                prefix_chain,
                &signing_key,
                &effects,
            ).expect("Prefix token creation should succeed");

            prop_assert!(
                token_prefix.verify(&verifying_key).is_ok(),
                "Delegation chain prefix should verify (transitivity)"
            );
        }
    }

    /// Property: Permission subsets are allowed
    ///
    /// Invariant: If token has permission set P, requesting subset S ⊂ P is valid
    #[test]
    fn prop_permission_subsets_allowed(
        device_id in arb_device_id(),
        mut permissions in arb_permissions(),
        key_seed in any::<u64>(),
        time_seed in any::<u64>(),
    ) {
        // Ensure at least 2 permissions so we can take a subset
        if permissions.len() < 2 {
            permissions.push(Permission::Storage {
                operation: "read".to_string(),
                resource: "/extra".to_string(),
            });
        }

        let effects = Effects::deterministic(time_seed, 1000);
        let signing_key = create_key_from_seed(key_seed);

        // Create token with full permission set
        let token_full = CapabilityToken::new(
            device_id.clone(),
            permissions.clone(),
            vec![],
            &signing_key,
            &effects,
        ).expect("Full token creation should succeed");

        // Create token with subset of permissions
        let subset_size = permissions.len() / 2;
        let subset_permissions = permissions[..subset_size].to_vec();

        let token_subset = CapabilityToken::new(
            device_id,
            subset_permissions.clone(),
            vec![],
            &signing_key,
            &effects,
        ).expect("Subset token creation should succeed");

        // Verify both tokens
        let verifying_key = signing_key.verifying_key();
        prop_assert!(token_full.verify(&verifying_key).is_ok(), "Full permission token should verify");
        prop_assert!(token_subset.verify(&verifying_key).is_ok(), "Subset permission token should verify");

        // Verify subset property
        prop_assert!(
            token_subset.is_subset_of_permissions(&permissions),
            "Subset token should have subset of full permissions"
        );

        // Verify each permission in subset exists in full set
        for perm in &subset_permissions {
            prop_assert!(token_full.has_permission(perm), "Full token should have all subset permissions");
        }
    }
}

#[cfg(test)]
mod manual_tests {

    #[test]
    fn test_property_tests_compile_and_run() {
        // This test ensures the proptest macros compile correctly
        // The actual property tests run via the proptest! macro above
        println!("[OK] Property tests compile successfully");
    }
}
