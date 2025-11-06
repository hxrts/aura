//! Cryptographic testing utilities for choreographic protocols

// CryptoEffects not available in current middleware structure
use aura_protocol::effects::Effects;
use ed25519_dalek::{SigningKey, VerifyingKey};
use frost_ed25519::{Identifier, SigningPackage};
use frost_ed25519::keys::{KeyPackage, SecretShare};
use frost_ed25519::round1::{SigningCommitments, SigningNonces};
use std::collections::BTreeMap;

/// Generate test key shares for FROST threshold signing
pub fn generate_test_frost_shares(
    threshold: u16,
    max_signers: u16,
    seed: u64,
) -> Result<BTreeMap<Identifier, SecretShare>, Box<dyn std::error::Error>> {
    let effects = Effects::deterministic(seed, 0);
    let mut rng = effects.rng();
    
    // Generate random secret for dealer
    let _secret = frost_ed25519::SigningKey::new(&mut rng);
    
    // Create identifiers for participants
    let mut identifiers = Vec::new();
    for i in 1..=max_signers {
        identifiers.push(Identifier::try_from(i)?);
    }
    
    // Generate key packages via dealer
    let (key_packages, _public_key_package) = frost_ed25519::keys::generate_with_dealer(
        max_signers,
        threshold,
        frost_ed25519::keys::IdentifierList::Custom(&identifiers),
        &mut rng,
    )?;
    
    Ok(key_packages)
}

/// Generate test signing nonces for FROST participants
pub fn generate_test_signing_nonces(
    participants: &[Identifier],
    key_packages: &BTreeMap<Identifier, SecretShare>, 
    seed: u64,
) -> Result<BTreeMap<Identifier, SigningNonces>, Box<dyn std::error::Error>> {
    let effects = Effects::deterministic(seed, 0);
    let mut rng = effects.rng();
    let mut nonces = BTreeMap::new();
    
    for &participant in participants {
        if let Some(key_package) = key_packages.get(&participant) {
            let signing_share = key_package.signing_share();
            let (nonce, _commitments) = frost_ed25519::round1::commit(signing_share, &mut rng);
            nonces.insert(participant, nonce);
        }
    }
    
    Ok(nonces)
}

/// Create test signing package for FROST
pub fn create_test_signing_package(
    message: &[u8],
    commitments: &BTreeMap<Identifier, SigningCommitments>,
) -> Result<SigningPackage, Box<dyn std::error::Error>> {
    Ok(SigningPackage::new(commitments.clone(), message))
}

/// Generate test DKD context hash
pub fn generate_test_dkd_context(
    app_id: &str,
    context: &str,
    effects: &Effects,
) -> [u8; 32] {
    let mut data = Vec::new();
    data.extend_from_slice(app_id.as_bytes());
    data.extend_from_slice(context.as_bytes());
    effects.blake3_hash(&data)
}

/// Create deterministic Ed25519 key for testing
pub fn create_test_signing_key(seed: u64, index: usize) -> SigningKey {
    let effects = Effects::deterministic(seed, index as u64);
    let key_bytes = effects.random_bytes_array::<32>();
    SigningKey::from_bytes(&key_bytes)
}

/// Verify Ed25519 signature
pub fn verify_test_signature(
    public_key: &VerifyingKey,
    message: &[u8],
    signature: &ed25519_dalek::Signature,
) -> bool {
    use ed25519_dalek::Verifier;
    public_key.verify(message, signature).is_ok()
}

/// Hash multiple byte arrays for testing
pub fn hash_test_data(data: &[&[u8]], effects: &Effects) -> [u8; 32] {
    let mut combined = Vec::new();
    for d in data {
        combined.extend_from_slice(d);
    }
    effects.blake3_hash(&combined)
}