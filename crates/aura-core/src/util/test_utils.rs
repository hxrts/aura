//! Test utilities for aura-core and Layer 2 crates
//!
//! **Architecture Note**: These utilities are internal to aura-core and DO NOT use aura-testkit
//! to avoid circular dependencies. aura-testkit depends on aura-core, so aura-core cannot depend
//! on aura-testkit.
//!
//! This module provides:
//! 1. Deterministic ID and key generation (always available)
//! 2. Mock effect implementations (available with `test-utils` feature)
//!
//! Layer 2 crates can use the mock effects by enabling the feature:
//! ```toml
//! [dev-dependencies]
//! aura-core = { path = "../aura-core", features = ["test-utils"] }
//! ```

#![allow(clippy::expect_used)] // Test utilities use expect for fixed-size slice conversions

use crate::crypto::ed25519::{Ed25519SigningKey, Ed25519VerifyingKey};
use crate::crypto::hash::hash;
use crate::types::identifiers::AuthorityId;
use crate::types::identifiers::DeviceId;
use crate::{AccountId, SessionId};
use uuid::Uuid;

/// Create a deterministic DeviceId from a seed
///
/// This produces the same DeviceId for the same seed, enabling reproducible tests.
///
/// # Example
/// ```
/// use aura_core::util::test_utils::test_device_id;
///
/// let device1 = test_device_id(42);
/// let device2 = test_device_id(42);
/// assert_eq!(device1, device2);
/// ```
pub fn test_device_id(seed: u64) -> DeviceId {
    let hash_input = format!("device-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let uuid_bytes: [u8; 16] = hash_bytes[..16]
        .try_into()
        .expect("Slice of exactly 16 bytes should convert to [u8; 16]");
    DeviceId(Uuid::from_bytes(uuid_bytes))
}

/// Create a deterministic AuthorityId from a seed
///
/// This produces the same AuthorityId for the same seed, enabling reproducible tests.
///
/// # Example
/// ```
/// use aura_core::util::test_utils::test_authority_id;
///
/// let authority1 = test_authority_id(42);
/// let authority2 = test_authority_id(42);
/// assert_eq!(authority1, authority2);
/// ```
pub fn test_authority_id(seed: u64) -> AuthorityId {
    let hash_input = format!("authority-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let uuid_bytes: [u8; 16] = hash_bytes[..16]
        .try_into()
        .expect("Slice of exactly 16 bytes should convert to [u8; 16]");
    AuthorityId(Uuid::from_bytes(uuid_bytes))
}

/// Create a deterministic AccountId from a seed
///
/// This produces the same AccountId for the same seed, enabling reproducible tests.
///
/// # Example
/// ```
/// use aura_core::util::test_utils::test_account_id;
///
/// let account1 = test_account_id(42);
/// let account2 = test_account_id(42);
/// assert_eq!(account1, account2);
/// ```
pub fn test_account_id(seed: u64) -> AccountId {
    let hash_input = format!("account-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let uuid_bytes: [u8; 16] = hash_bytes[..16]
        .try_into()
        .expect("Slice of exactly 16 bytes should convert to [u8; 16]");
    AccountId(Uuid::from_bytes(uuid_bytes))
}

/// Create a deterministic SessionId from a seed
///
/// This produces the same SessionId for the same seed, enabling reproducible tests.
///
/// # Example
/// ```
/// use aura_core::util::test_utils::test_session_id;
///
/// let session1 = test_session_id(42);
/// let session2 = test_session_id(42);
/// assert_eq!(session1, session2);
/// ```
pub fn test_session_id(seed: u64) -> SessionId {
    let hash_input = format!("session-{}", seed);
    let hash_bytes = hash(hash_input.as_bytes());
    let uuid_bytes: [u8; 16] = hash_bytes[..16]
        .try_into()
        .expect("Slice of exactly 16 bytes should convert to [u8; 16]");
    SessionId(Uuid::from_bytes(uuid_bytes))
}

/// Create a deterministic Ed25519 key pair from a seed
///
/// Returns `(Ed25519SigningKey, Ed25519VerifyingKey)` tuple for testing.
/// The same seed always produces the same key pair.
///
/// # Example
/// ```
/// use aura_core::util::test_utils::test_key_pair;
///
/// let (sk1, vk1) = test_key_pair(42);
/// let (sk2, vk2) = test_key_pair(42);
/// assert_eq!(vk1, vk2);
/// ```
pub fn test_key_pair(seed: u64) -> (Ed25519SigningKey, Ed25519VerifyingKey) {
    let mut key_bytes = [0u8; 32];
    key_bytes[..8].copy_from_slice(&seed.to_le_bytes());
    let signing_key = Ed25519SigningKey::from_bytes(&key_bytes);
    let verifying_key = signing_key
        .verifying_key()
        .expect("valid signing key should produce valid verifying key");
    (signing_key, verifying_key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_id_deterministic() {
        let id1 = test_device_id(42);
        let id2 = test_device_id(42);
        assert_eq!(id1, id2);

        let id3 = test_device_id(43);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_authority_id_deterministic() {
        let id1 = test_authority_id(42);
        let id2 = test_authority_id(42);
        assert_eq!(id1, id2);

        let id3 = test_authority_id(43);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_account_id_deterministic() {
        let id1 = test_account_id(42);
        let id2 = test_account_id(42);
        assert_eq!(id1, id2);

        let id3 = test_account_id(43);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_session_id_deterministic() {
        let id1 = test_session_id(42);
        let id2 = test_session_id(42);
        assert_eq!(id1, id2);

        let id3 = test_session_id(43);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_key_pair_deterministic() {
        let (sk1, vk1) = test_key_pair(42);
        let (sk2, vk2) = test_key_pair(42);
        assert_eq!(vk1, vk2);
        assert_eq!(sk1.to_bytes(), sk2.to_bytes());

        let (_, vk3) = test_key_pair(43);
        assert_ne!(vk1, vk3);
    }

    #[test]
    fn test_key_pair_can_sign_and_verify() {
        let (sk, vk) = test_key_pair(42);
        let message = b"test message";
        let signature = sk.sign(message).expect("signing should succeed");
        vk.verify(message, &signature)
            .expect("verification should succeed");
    }
}
