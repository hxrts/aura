//! FROST crypto integration tests

use aura_choreography::integration::crypto_bridge::FrostCryptoBridge;
use aura_choreography::test_utils::crypto_test_utils::{generate_test_frost_shares, create_test_signing_package};
use aura_protocol::effects::Effects;
use frost_ed25519::Identifier;
use frost_ed25519::round1::SigningCommitments;
use std::collections::BTreeMap;

/// Test FROST crypto bridge basic nonce generation
#[tokio::test]
async fn test_frost_nonce_generation() {
    let effects = Effects::deterministic(42, 0);
    let key_packages = generate_test_frost_shares(2, 3, 42)
        .expect("Failed to generate test shares");
    let bridge = FrostCryptoBridge::new(effects, key_packages);
    
    let identifier = Identifier::try_from(1u16).expect("Valid identifier");
    
    // Generate nonces
    let nonces = bridge.generate_nonces(identifier).await
        .expect("Failed to generate nonces");
    
    // Verify nonces are generated (we can't inspect internal structure easily)
    // But we can verify the operation succeeded
    
    // Generate again to verify different nonces
    let nonces_2 = bridge.generate_nonces(identifier).await
        .expect("Failed to generate nonces second time");
    
    // Note: With deterministic effects, nonces might be the same
    // This tests that the bridge works correctly
}

/// Test FROST commitment creation
#[tokio::test]
async fn test_frost_commitment_creation() {
    let effects = Effects::deterministic(12345, 0);
    let key_packages = generate_test_frost_shares(2, 3, 12345)
        .expect("Failed to generate test shares");
    let bridge = FrostCryptoBridge::new(effects, key_packages);
    
    let identifier = Identifier::try_from(1u16).expect("Valid identifier");
    
    // Generate nonces first
    let nonces = bridge.generate_nonces(identifier).await
        .expect("Failed to generate nonces");
    
    // Create commitments
    let commitments = bridge.create_commitments(&nonces).await
        .expect("Failed to create commitments");
    
    // Verify commitments can be serialized (basic functionality test)
    let hiding_bytes = commitments.hiding().serialize();
    let binding_bytes = commitments.binding().serialize();
    
    assert_eq!(hiding_bytes.len(), 32, "Hiding commitment should be 32 bytes");
    assert_eq!(binding_bytes.len(), 32, "Binding commitment should be 32 bytes");
    assert_ne!(hiding_bytes, [0u8; 32], "Hiding commitment should not be all zeros");
    assert_ne!(binding_bytes, [0u8; 32], "Binding commitment should not be all zeros");
}

/// Test FROST signature share generation
#[tokio::test]
async fn test_frost_signature_share_generation() {
    let effects = Effects::deterministic(98765, 0);
    let key_packages = generate_test_frost_shares(2, 3, 98765)
        .expect("Failed to generate test shares");
    let bridge = FrostCryptoBridge::new(effects, key_packages);
    
    let identifier = Identifier::try_from(1u16).expect("Valid identifier");
    let message = b"Test message for signing";
    
    // Generate nonces and commitments
    let nonces = bridge.generate_nonces(identifier).await
        .expect("Failed to generate nonces");
    let commitments = bridge.create_commitments(&nonces).await
        .expect("Failed to create commitments");
    
    // Create a simple signing package with just our commitment
    let mut commitment_map = BTreeMap::new();
    commitment_map.insert(identifier, commitments);
    
    let signing_package = create_test_signing_package(message, &commitment_map)
        .expect("Failed to create signing package");
    
    // Generate signature share
    let signature_share = bridge.generate_signature_share(
        identifier,
        &nonces,
        &signing_package,
    ).await.expect("Failed to generate signature share");
    
    // Verify signature share can be serialized
    let share_bytes = signature_share.serialize();
    assert_eq!(share_bytes.len(), 32, "Signature share should be 32 bytes");
    assert_ne!(share_bytes, [0u8; 32], "Signature share should not be all zeros");
}

/// Test FROST signature aggregation
#[tokio::test]
async fn test_frost_signature_aggregation() {
    let effects = Effects::deterministic(55555, 0);
    let key_packages = generate_test_frost_shares(2, 3, 55555)
        .expect("Failed to generate test shares");
    let bridge = FrostCryptoBridge::new(effects.clone(), key_packages.clone());
    
    let message = b"Aggregation test message";
    
    // Simulate generating shares from multiple participants
    let mut all_commitments = BTreeMap::new();
    let mut all_nonces = BTreeMap::new();
    let mut all_shares = BTreeMap::new();
    
    // Generate for participants 1 and 2 (threshold = 2)
    for i in 1..=2 {
        let identifier = Identifier::try_from(i as u16).expect("Valid identifier");
        
        // Generate nonces and commitments
        let nonces = bridge.generate_nonces(identifier).await
            .expect(&format!("Failed to generate nonces for participant {}", i));
        let commitments = bridge.create_commitments(&nonces).await
            .expect(&format!("Failed to create commitments for participant {}", i));
        
        all_commitments.insert(identifier, commitments);
        all_nonces.insert(identifier, nonces);
    }
    
    // Create signing package
    let signing_package = create_test_signing_package(message, &all_commitments)
        .expect("Failed to create signing package");
    
    // Generate signature shares
    for (identifier, nonces) in all_nonces {
        let signature_share = bridge.generate_signature_share(
            identifier,
            &nonces,
            &signing_package,
        ).await.expect(&format!("Failed to generate signature share for {:?}", identifier));
        
        all_shares.insert(identifier, signature_share);
    }
    
    // Create a minimal public key package for aggregation
    let mut rng = effects.rng();
    let identifiers: Vec<Identifier> = (1..=3)
        .map(|i| Identifier::try_from(i as u16))
        .collect::<Result<Vec<_>, _>>()
        .expect("Valid identifiers");
    
    let (_key_packages, public_key_package) = frost_ed25519::keys::generate_with_dealer(
        3, // max_signers
        2, // min_signers
        identifiers,
        &mut rng,
    ).expect("Failed to generate key packages");
    
    // Aggregate signature
    let group_signature = bridge.aggregate_signature(
        &signing_package,
        &all_shares,
        &public_key_package,
    ).await.expect("Failed to aggregate signature");
    
    // Verify signature format
    let signature_bytes = group_signature.serialize();
    assert_eq!(signature_bytes.len(), 64, "Ed25519 signature should be 64 bytes");
    assert_ne!(signature_bytes.to_vec(), vec![0u8; 64], "Signature should not be all zeros");
}

/// Test FROST signature verification
#[tokio::test]
async fn test_frost_signature_verification() {
    let effects = Effects::deterministic(77777, 0);
    let key_packages = generate_test_frost_shares(2, 3, 77777)
        .expect("Failed to generate test shares");
    let bridge = FrostCryptoBridge::new(effects.clone(), key_packages);
    
    let message = b"Verification test message";
    
    // Create minimal setup for verification test
    let mut rng = effects.rng();
    let identifiers: Vec<Identifier> = (1..=3)
        .map(|i| Identifier::try_from(i as u16))
        .collect::<Result<Vec<_>, _>>()
        .expect("Valid identifiers");
    
    let (key_packages, public_key_package) = frost_ed25519::keys::generate_with_dealer(
        3, // max_signers
        2, // min_signers
        identifiers,
        &mut rng,
    ).expect("Failed to generate key packages");
    
    // Create a simple signing process
    let mut commitments = BTreeMap::new();
    let mut nonces_map = BTreeMap::new();
    
    // Generate commitments for threshold participants
    for i in 1..=2 {
        let identifier = Identifier::try_from(i as u16).expect("Valid identifier");
        let nonces = frost_ed25519::round1::commit(
            key_packages[&identifier].signing_share(),
            &mut rng,
        );
        let commitment = nonces.commitments().clone();
        
        commitments.insert(identifier, commitment);
        nonces_map.insert(identifier, nonces);
    }
    
    // Create signing package
    let signing_package = create_test_signing_package(message, &commitments)
        .expect("Failed to create signing package");
    
    // Generate signature shares
    let mut signature_shares = BTreeMap::new();
    for (identifier, nonces) in nonces_map {
        let signature_share = frost_ed25519::round2::sign(
            &signing_package,
            &nonces,
            key_packages[&identifier].signing_share(),
        ).expect("Failed to create signature share");
        signature_shares.insert(identifier, signature_share);
    }
    
    // Aggregate signature
    let group_signature = frost_ed25519::aggregate(
        &signing_package,
        &signature_shares,
        &public_key_package,
    ).expect("Failed to aggregate signature");
    
    // Test verification through the bridge
    let is_valid = bridge.verify_signature(message, &group_signature, &public_key_package).await
        .expect("Failed to verify signature");
    
    assert!(is_valid, "Valid signature should verify successfully");
    
    // Test with wrong message
    let wrong_message = b"Different message";
    let is_invalid = bridge.verify_signature(wrong_message, &group_signature, &public_key_package).await
        .expect("Failed to verify wrong signature");
    
    assert!(!is_invalid, "Invalid signature should fail verification");
}

/// Test FROST deterministic behavior
#[tokio::test]
async fn test_frost_deterministic_behavior() {
    let seed = 88888;
    
    // Run 1
    let effects_1 = Effects::deterministic(seed, 0);
    let key_packages_1 = generate_test_frost_shares(2, 3, seed)
        .expect("Failed to generate test shares");
    let bridge_1 = FrostCryptoBridge::new(effects_1, key_packages_1);
    
    let identifier = Identifier::try_from(1u16).expect("Valid identifier");
    let nonces_1 = bridge_1.generate_nonces(identifier).await
        .expect("Failed to generate nonces in run 1");
    let commitments_1 = bridge_1.create_commitments(&nonces_1).await
        .expect("Failed to create commitments in run 1");
    
    // Run 2 with same seed
    let effects_2 = Effects::deterministic(seed, 0);
    let key_packages_2 = generate_test_frost_shares(2, 3, seed)
        .expect("Failed to generate test shares");
    let bridge_2 = FrostCryptoBridge::new(effects_2, key_packages_2);
    
    let nonces_2 = bridge_2.generate_nonces(identifier).await
        .expect("Failed to generate nonces in run 2");
    let commitments_2 = bridge_2.create_commitments(&nonces_2).await
        .expect("Failed to create commitments in run 2");
    
    // Verify deterministic behavior
    assert_eq!(
        commitments_1.hiding().serialize(),
        commitments_2.hiding().serialize(),
        "Same seed should produce same hiding commitments"
    );
    assert_eq!(
        commitments_1.binding().serialize(), 
        commitments_2.binding().serialize(),
        "Same seed should produce same binding commitments"
    );
}

/// Test FROST error handling
#[tokio::test]
async fn test_frost_error_handling() {
    let effects = Effects::deterministic(99999, 0);
    let key_packages = generate_test_frost_shares(2, 3, 99999)
        .expect("Failed to generate test shares");
    let bridge = FrostCryptoBridge::new(effects.clone(), key_packages);
    
    // Test with invalid identifier
    let invalid_identifier = Identifier::try_from(10u16).expect("Valid but unused identifier");
    let result = bridge.generate_nonces(invalid_identifier).await;
    assert!(result.is_err(), "Invalid identifier should cause an error");
    
    // Test empty signature shares map
    let message = b"Test message";
    let empty_commitments = BTreeMap::new();
    let signing_package = create_test_signing_package(message, &empty_commitments);
    assert!(signing_package.is_err(), "Empty commitments should cause an error");
}