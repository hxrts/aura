//! Test Key Utilities
//!
//! Factory functions for creating test keys and key shares.
//! Consolidates FROST key generation and device key patterns.

use aura_crypto::Effects;
use ed25519_dalek::{SigningKey, VerifyingKey};
use std::collections::BTreeMap;

// Re-export commonly used FROST types
pub use frost_ed25519 as frost;

/// Create a test signing key with deterministic effects
/// 
/// Standard pattern for creating signing keys in tests.
/// 
/// # Arguments
/// * `effects` - Effects instance for deterministic key generation
pub fn test_signing_key(effects: &Effects) -> SigningKey {
    let key_bytes = effects.random_bytes::<32>();
    SigningKey::from_bytes(&key_bytes)
}

/// Create a test signing key from a specific seed
/// 
/// For tests that need a predictable key.
/// 
/// # Arguments
/// * `seed` - Specific seed for key generation
pub fn test_signing_key_from_seed(seed: u64) -> SigningKey {
    let effects = Effects::deterministic(seed, 1000);
    test_signing_key(&effects)
}

/// Create a deterministic key pair
/// 
/// Returns both signing and verifying keys.
/// 
/// # Arguments
/// * `effects` - Effects instance for deterministic generation
pub fn test_key_pair(effects: &Effects) -> (SigningKey, VerifyingKey) {
    let signing_key = test_signing_key(effects);
    let verifying_key = signing_key.verifying_key();
    (signing_key, verifying_key)
}

/// Generate FROST key shares for testing
/// 
/// This consolidates the key share generation pattern found in multiple test files.
/// Creates threshold FROST key shares with deterministic generation.
/// 
/// # Arguments
/// * `threshold` - M-of-N threshold value
/// * `total` - Total number of participants  
/// * `effects` - Effects instance for deterministic generation
/// 
/// # Returns
/// * Tuple of (key packages by identifier, public key package)
pub fn test_frost_key_shares(
    threshold: u16,
    total: u16,
    effects: &Effects,
) -> (BTreeMap<frost::Identifier, frost::keys::KeyPackage>, frost::keys::PublicKeyPackage) {
    let mut rng = effects.rng();
    
    let (shares, pubkey_package) = frost::keys::generate_with_dealer(
        total,
        threshold,
        frost::keys::IdentifierList::Default,
        &mut rng,
    ).unwrap(); // Key generation should succeed
    
    // Convert SecretShares to KeyPackages and then to BTreeMap
    let key_packages: BTreeMap<frost::Identifier, frost::keys::KeyPackage> = shares
        .into_iter()
        .map(|(id, secret_share)| {
            let key_package = frost::keys::KeyPackage::try_from(secret_share)
                .unwrap(); // KeyPackage creation should succeed
            (id, key_package)
        })
        .collect();
    
    (key_packages, pubkey_package)
}

/// Generate FROST key shares with default identifiers
/// 
/// Creates key shares using FROST's default identifier assignment.
/// 
/// # Arguments
/// * `threshold` - M-of-N threshold value
/// * `total` - Total number of participants
/// * `effects` - Effects instance for deterministic generation
pub fn test_frost_key_shares_default(
    threshold: u16,
    total: u16,
    effects: &Effects,
) -> (BTreeMap<frost::Identifier, frost::keys::KeyPackage>, frost::keys::PublicKeyPackage) {
    let mut rng = effects.rng();
    
    let (shares, pubkey_package) = frost::keys::generate_with_dealer(
        total,
        threshold,
        frost::keys::IdentifierList::Default,
        &mut rng,
    ).unwrap(); // Key generation should succeed
    
    // Convert SecretShares to KeyPackages - this matches existing pattern
    let key_packages: BTreeMap<frost::Identifier, frost::keys::KeyPackage> = shares
        .into_iter()
        .map(|(id, secret_share)| {
            let key_package = frost::keys::KeyPackage::try_from(secret_share)
                .unwrap(); // KeyPackage creation should succeed
            (id, key_package)
        })
        .collect();
    
    (key_packages, pubkey_package)
}

/// Create a single FROST key package for testing
/// 
/// For tests that only need one participant's key package.
/// Gets the first participant from the generated key shares.
/// 
/// # Arguments
/// * `threshold` - M-of-N threshold value
/// * `total` - Total number of participants
/// * `effects` - Effects instance for deterministic generation
pub fn test_frost_single_key_package(
    threshold: u16,
    total: u16,
    effects: &Effects,
) -> (frost::keys::KeyPackage, frost::keys::PublicKeyPackage) {
    let (key_packages, pubkey_package) = test_frost_key_shares(threshold, total, effects);
    
    let key_package = key_packages
        .values()
        .next()
        .unwrap() // Should have at least one key package
        .clone();
    
    (key_package, pubkey_package)
}

/// Create device key for capability testing
/// 
/// This matches the create_test_device_key pattern found in capability tests.
/// 
/// # Arguments
/// * `effects` - Effects instance for deterministic generation
pub fn test_device_key(effects: &Effects) -> SigningKey {
    test_signing_key(effects)
}