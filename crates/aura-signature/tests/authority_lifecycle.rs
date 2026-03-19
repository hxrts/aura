//! Authority registry and signature verification integration tests.
//!
//! Exercises the public API: registry creation, authority status types,
//! and unknown authority verification.

use aura_core::types::identifiers::AuthorityId;
use aura_signature::{AuthorityRegistry, AuthorityStatus};

/// Fresh registry has no known authorities.
#[test]
fn empty_registry_has_no_authorities() {
    let registry = AuthorityRegistry::new();
    assert!(registry.known_authorities().is_empty());
}

/// Unknown authority verification returns error.
#[test]
fn unknown_authority_verification_fails() {
    let registry = AuthorityRegistry::new();
    let unknown = AuthorityId::new_from_entropy([99u8; 32]);

    let result = registry.verify_authority(unknown);
    assert!(result.is_err(), "Unknown authority must fail verification");
}

/// AuthorityStatus variants are distinct and serializable.
#[test]
fn authority_status_variants_are_distinct() {
    assert_ne!(AuthorityStatus::Active, AuthorityStatus::Suspended);
    assert_ne!(AuthorityStatus::Active, AuthorityStatus::Revoked);
    assert_ne!(AuthorityStatus::Suspended, AuthorityStatus::Revoked);

    // Round-trip through serde
    let json = serde_json::to_string(&AuthorityStatus::Suspended).unwrap();
    let restored: AuthorityStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(restored, AuthorityStatus::Suspended);
}
