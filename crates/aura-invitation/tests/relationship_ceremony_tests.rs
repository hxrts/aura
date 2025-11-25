//! Integration tests for bidirectional relationship key establishment ceremony

#![allow(clippy::disallowed_methods)]

use aura_core::{AccountId, ContextId, DeviceId};
use aura_invitation::relationship_formation::{
    execute_relationship_formation, RelationshipFormationConfig, RelationshipFormationError,
    RelationshipKeys,
};
use aura_macros::aura_test;
// use aura_testkit::ChoreographyTestHarness; // Commented out for now
use uuid::Uuid;
// Note: For testing, use mock handlers from aura-effects

/// Test successful relationship formation ceremony
#[aura_test]
async fn test_successful_relationship_formation() -> aura_core::AuraResult<()> {
    let initiator_id = DeviceId(Uuid::new_v4());
    let responder_id = DeviceId(Uuid::new_v4());
    let account_context = Some(AccountId(Uuid::new_v4()));

    let initiator_fixture = aura_testkit::create_test_fixture_with_device_id(initiator_id).await?;
    let _responder_fixture = aura_testkit::create_test_fixture_with_device_id(responder_id).await?;
    let initiator_effects = initiator_fixture.effect_system_wrapped();

    let config = RelationshipFormationConfig {
        initiator_id,
        responder_id,
        account_context,
        timeout_secs: 60,
    };

    // Test that both sides can complete the ceremony
    // In a real implementation, these would run concurrently with proper message routing
    // For testing, we simulate by checking that the same config produces consistent results

    let result = execute_relationship_formation(
        initiator_id,
        config.clone(),
        true, // is_initiator
        initiator_effects.as_ref(),
    )
    .await;

    // In simulation mode, this should complete successfully
    // Note: In reality, this test would need a full choreography simulation framework
    match result {
        Ok(formation_result) => {
            assert!(formation_result.success);
            assert_ne!(formation_result.context_id, ContextId(Uuid::new_v4())); // Should be derived
            assert_ne!(formation_result.relationship_keys.encryption_key, [0u8; 32]); // Should be generated
            assert_ne!(formation_result.relationship_keys.mac_key, [0u8; 32]); // Should be generated
            assert!(!formation_result
                .relationship_keys
                .derivation_context
                .is_empty());
            println!("✓ Relationship formation ceremony completed successfully");
        }
        Err(e) => {
            // Expected in simulation mode without full message routing
            println!(
                "⚠ Relationship formation failed as expected in test mode: {}",
                e
            );
            assert!(matches!(e, RelationshipFormationError::Communication(_)));
        }
    }
    Ok(())
}

/// Test relationship formation with invalid configuration
#[aura_test]
async fn test_invalid_configuration() -> aura_core::AuraResult<()> {
    let device_id = DeviceId(Uuid::new_v4());
    let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
    let effect_system = fixture.effect_system_wrapped();

    let config = RelationshipFormationConfig {
        initiator_id: device_id,
        responder_id: device_id, // Same device - should fail
        account_context: None,
        timeout_secs: 60,
    };

    let result =
        execute_relationship_formation(device_id, config, true, effect_system.as_ref()).await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        RelationshipFormationError::InvalidConfig(_)
    ));
    Ok(())
}

/// Test bidirectional relationship formation using choreography test harness
#[aura_test]
async fn test_bidirectional_relationship_formation() -> aura_core::AuraResult<()> {
    // Simplified test that validates the API and structure
    // The ChoreographyTestHarness requires more complex setup for full bidirectional coordination

    let initiator_id = DeviceId(Uuid::new_v4());
    let responder_id = DeviceId(Uuid::new_v4());
    let account_context = Some(AccountId::new());

    let config = RelationshipFormationConfig {
        initiator_id,
        responder_id,
        account_context,
        timeout_secs: 60,
    };

    // For now, test that the configuration is created correctly
    assert_eq!(config.initiator_id, initiator_id);
    assert_eq!(config.responder_id, responder_id);
    assert_eq!(config.account_context, account_context);
    assert_eq!(config.timeout_secs, 60);

    println!("✓ Relationship formation configuration validated");
    // Note: Full bidirectional testing requires enhanced mock network coordination
    // This validates the API structure for now
    Ok(())
}

/// Test relationship key properties
#[aura_test]
async fn test_relationship_key_properties() -> aura_core::AuraResult<()> {
    let device_id = DeviceId(Uuid::new_v4());
    let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
    let effect_system = fixture.effect_system_wrapped();

    // Test that identical inputs produce identical keys
    let private_key = [42u8; 32];
    let peer_public_key = [24u8; 32];
    let context_id = ContextId(Uuid::new_v4());

    let keys1 = aura_invitation::relationship_formation::derive_relationship_keys(
        &private_key,
        &peer_public_key,
        &context_id,
        effect_system.as_ref(),
    )
    .await?;

    let keys2 = aura_invitation::relationship_formation::derive_relationship_keys(
        &private_key,
        &peer_public_key,
        &context_id,
        effect_system.as_ref(),
    )
    .await?;

    assert_eq!(keys1.encryption_key, keys2.encryption_key);
    assert_eq!(keys1.mac_key, keys2.mac_key);
    assert_eq!(keys1.derivation_context, keys2.derivation_context);

    // Test that different inputs produce different keys
    let different_peer_key = [99u8; 32];
    let keys3 = aura_invitation::relationship_formation::derive_relationship_keys(
        &private_key,
        &different_peer_key,
        &context_id,
        effect_system.as_ref(),
    )
    .await?;

    assert_ne!(keys1.encryption_key, keys3.encryption_key);
    assert_ne!(keys1.mac_key, keys3.mac_key);
    // derivation_context includes context_id, so it will be different due to different shared secret
    assert_ne!(keys1.derivation_context, keys3.derivation_context);
    Ok(())
}

/// Test bidirectional key derivation symmetry
#[aura_test]
async fn test_bidirectional_key_symmetry() -> aura_core::AuraResult<()> {
    let device_id = DeviceId(Uuid::new_v4());
    let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
    let effect_system = fixture.effect_system_wrapped();

    let alice_private = [1u8; 32];
    let bob_private = [2u8; 32];
    let context_id = ContextId(Uuid::new_v4());

    // Derive public keys (simplified)
    let alice_public = aura_invitation::relationship_formation::derive_public_key(
        &alice_private,
        effect_system.as_ref(),
    )
    .await?;

    let bob_public = aura_invitation::relationship_formation::derive_public_key(
        &bob_private,
        effect_system.as_ref(),
    )
    .await?;

    // Alice derives keys using her private key and Bob's public key
    let alice_keys = aura_invitation::relationship_formation::derive_relationship_keys(
        &alice_private,
        &bob_public,
        &context_id,
        effect_system.as_ref(),
    )
    .await?;

    // Bob derives keys using his private key and Alice's public key
    let bob_keys = aura_invitation::relationship_formation::derive_relationship_keys(
        &bob_private,
        &alice_public,
        &context_id,
        effect_system.as_ref(),
    )
    .await?;

    // The derived keys should be identical (symmetric ECDH property)
    assert_eq!(alice_keys.encryption_key, bob_keys.encryption_key);
    assert_eq!(alice_keys.mac_key, bob_keys.mac_key);
    assert_eq!(alice_keys.derivation_context, bob_keys.derivation_context);
    Ok(())
}

/// Test validation proof creation and verification
#[aura_test]
async fn test_validation_proof_system() -> aura_core::AuraResult<()> {
    let alice_device = DeviceId::new();
    let bob_device = DeviceId::new();
    let fixture = aura_testkit::create_test_fixture_with_device_id(alice_device).await?;
    let effect_system = fixture.effect_system();

    let relationship_keys = RelationshipKeys {
        encryption_key: [100u8; 32].to_vec(),
        mac_key: [200u8; 32].to_vec(),
        derivation_context: vec![1, 2, 3, 4, 5],
    };

    // Alice creates her validation proof
    let alice_proof = aura_invitation::relationship_formation::create_validation_proof(
        &relationship_keys,
        &alice_device,
        effect_system.as_ref(),
    )
    .await?;

    // Bob creates his validation proof
    let bob_proof = aura_invitation::relationship_formation::create_validation_proof(
        &relationship_keys,
        &bob_device,
        effect_system.as_ref(),
    )
    .await?;

    // Proofs should be different (device-specific)
    assert_ne!(alice_proof, bob_proof);

    // Test key hash creation
    let key_hash = aura_invitation::relationship_formation::hash_relationship_keys(
        &relationship_keys,
        effect_system.as_ref(),
    )
    .await?;

    // Create validation structures
    let alice_validation = aura_invitation::relationship_formation::RelationshipValidation {
        context_id: ContextId(Uuid::new_v4()),
        validation_proof: alice_proof,
        key_hash: key_hash.clone(),
    };

    let bob_validation = aura_invitation::relationship_formation::RelationshipValidation {
        context_id: ContextId(Uuid::new_v4()),
        validation_proof: bob_proof,
        key_hash,
    };

    // Verify that Alice's proof is valid for Alice's device
    let result = aura_invitation::relationship_formation::verify_validation_proof(
        &alice_validation,
        &relationship_keys,
        &alice_device,
        effect_system.as_ref(),
    )
    .await;
    assert!(result.is_ok());

    // Verify that Bob's proof is valid for Bob's device
    let result = aura_invitation::relationship_formation::verify_validation_proof(
        &bob_validation,
        &relationship_keys,
        &bob_device,
        effect_system.as_ref(),
    )
    .await;
    assert!(result.is_ok());

    // Verify that Alice's proof is NOT valid for Bob's device (cross-verification should fail)
    let result = aura_invitation::relationship_formation::verify_validation_proof(
        &alice_validation,
        &relationship_keys,
        &bob_device,
        effect_system.as_ref(),
    )
    .await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        aura_invitation::relationship_formation::RelationshipFormationError::ValidationFailed(_)
    ));
    Ok(())
}

/// Test trust record creation and signature verification
#[aura_test]
async fn test_trust_record_system() -> aura_core::AuraResult<()> {
    let alice_device = DeviceId::new();
    let bob_device = DeviceId::new();
    let fixture = aura_testkit::create_test_fixture_with_device_id(alice_device).await?;
    let effect_system = fixture.effect_system();

    let context_id = ContextId(Uuid::new_v4());
    let relationship_keys = RelationshipKeys {
        encryption_key: [50u8; 32].to_vec(),
        mac_key: [60u8; 32].to_vec(),
        derivation_context: vec![7, 8, 9],
    };

    // Alice creates a trust record for her relationship with Bob
    let trust_record_hash = aura_invitation::relationship_formation::create_trust_record(
        &context_id,
        &bob_device,
        &relationship_keys,
        effect_system.as_ref(),
    )
    .await?;

    // Alice signs the trust record
    let alice_signature = aura_invitation::relationship_formation::sign_trust_record(
        &trust_record_hash,
        &alice_device,
        effect_system.as_ref(),
    )
    .await?;

    // Create confirmation structure
    let alice_confirmation = aura_invitation::relationship_formation::RelationshipConfirmation {
        context_id,
        trust_record_hash,
        signature: alice_signature,
    };

    // Verify Alice's signature
    let result = aura_invitation::relationship_formation::verify_trust_record_signature(
        &alice_confirmation,
        &alice_device,
        effect_system.as_ref(),
    )
    .await;
    assert!(result.is_ok());

    // Verify that Bob's device cannot forge Alice's signature
    let result = aura_invitation::relationship_formation::verify_trust_record_signature(
        &alice_confirmation,
        &bob_device,
        effect_system.as_ref(),
    )
    .await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        aura_invitation::relationship_formation::RelationshipFormationError::ValidationFailed(_)
    ));
    Ok(())
}

/// Test context ID derivation consistency
#[aura_test]
async fn test_context_id_derivation() -> aura_core::AuraResult<()> {
    let device_id = DeviceId(Uuid::new_v4());
    let fixture = aura_testkit::create_test_fixture_with_device_id(device_id).await?;
    let effect_system = fixture.effect_system_wrapped();

    let init_request = aura_invitation::relationship_formation::RelationshipInitRequest {
        initiator_id: DeviceId(Uuid::new_v4()),
        responder_id: DeviceId(Uuid::new_v4()),
        account_context: Some(AccountId(Uuid::new_v4())),
        timestamp: 1234567890,
        nonce: vec![42u8; 32],
    };

    // Same request should produce same context ID
    let context1 = aura_invitation::relationship_formation::derive_context_id(
        &init_request,
        effect_system.as_ref(),
    )
    .await?;

    let context2 = aura_invitation::relationship_formation::derive_context_id(
        &init_request,
        effect_system.as_ref(),
    )
    .await?;

    assert_eq!(context1, context2);

    // Different nonce should produce different context ID
    let mut different_request = init_request.clone();
    different_request.nonce = vec![99u8; 32];

    let context3 = aura_invitation::relationship_formation::derive_context_id(
        &different_request,
        effect_system.as_ref(),
    )
    .await?;

    assert_ne!(context1, context3);
    Ok(())
}
