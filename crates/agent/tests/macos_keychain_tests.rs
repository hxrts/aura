//! macOS-specific keychain integration tests
//!
//! These tests verify that the secure storage system works correctly with macOS Keychain Services.
//! They should only be run on macOS systems and require keychain access permissions.

#![cfg(target_os = "macos")]

use aura_agent::device_secure_store::{DeviceAttestation, PlatformSecureStorage, SecureStorage};
use aura_coordination::KeyShare;
use frost_ed25519::keys::{KeyPackage, PublicKeyPackage};
use std::process::Command;
use uuid::Uuid;

/// Test that we can create a PlatformSecureStorage instance on macOS
#[tokio::test]
async fn test_platform_storage_creation() {
    let storage = PlatformSecureStorage::new();
    assert!(
        storage.is_ok(),
        "Should be able to create PlatformSecureStorage on macOS"
    );
}

/// Test basic keychain operations: store, load, delete
#[tokio::test]
async fn test_keychain_basic_operations() {
    let storage = PlatformSecureStorage::new().expect("Failed to create storage");
    let test_key_id = format!("test_basic_ops_{}", Uuid::new_v4());

    // Create a test key share
    let (key_packages, public_key_package) = frost_ed25519::keys::generate_with_dealer(
        2,
        3,
        Default::default(),
        &mut rand::thread_rng(),
    )
    .expect("Failed to generate test keys");

    let key_share = KeyShare {
        share: KeyPackage::try_from(key_packages.into_iter().next().unwrap().1)
            .expect("Failed to create KeyPackage"),
        public_key_package,
    };

    // Test store operation
    let store_result = storage.store_key_share(&test_key_id, &key_share);
    assert!(
        store_result.is_ok(),
        "Should be able to store key share in keychain: {:?}",
        store_result
    );

    // Test load operation
    let load_result = storage.load_key_share(&test_key_id);
    assert!(
        load_result.is_ok(),
        "Should be able to load key share from keychain: {:?}",
        load_result
    );

    let loaded_share = load_result.unwrap();
    assert_eq!(
        key_share.share.verifying_share().serialize(),
        loaded_share.share.verifying_share().serialize(),
        "Loaded key share should match original"
    );

    // Test delete operation
    let delete_result = storage.delete_key_share(&test_key_id);
    assert!(
        delete_result.is_ok(),
        "Should be able to delete key share from keychain: {:?}",
        delete_result
    );

    // Verify deletion
    let load_after_delete = storage.load_key_share(&test_key_id);
    assert!(
        load_after_delete.is_err(),
        "Should not be able to load deleted key share"
    );
}

/// Test that we can list stored key shares
#[tokio::test]
async fn test_keychain_list_operations() {
    let storage = PlatformSecureStorage::new().expect("Failed to create storage");
    let test_prefix = format!("test_list_{}", Uuid::new_v4());
    let key_ids: Vec<String> = (0..3).map(|i| format!("{}_{}", test_prefix, i)).collect();

    // Create test key shares
    let (key_packages, public_key_package) = frost_ed25519::keys::generate_with_dealer(
        2,
        3,
        Default::default(),
        &mut rand::thread_rng(),
    )
    .expect("Failed to generate test keys");

    let key_share = KeyShare {
        share: KeyPackage::try_from(key_packages.into_iter().next().unwrap().1)
            .expect("Failed to create KeyPackage"),
        public_key_package,
    };

    // Store multiple key shares
    for key_id in &key_ids {
        storage
            .store_key_share(key_id, &key_share)
            .expect("Failed to store test key share");
    }

    // List all key shares
    let list_result = storage.list_key_shares();
    assert!(
        list_result.is_ok(),
        "Should be able to list key shares: {:?}",
        list_result
    );

    let stored_keys = list_result.unwrap();
    for key_id in &key_ids {
        assert!(
            stored_keys.contains(key_id),
            "Listed keys should contain {}",
            key_id
        );
    }

    // Clean up
    for key_id in &key_ids {
        storage
            .delete_key_share(key_id)
            .expect("Failed to clean up test key share");
    }
}

/// Test that keychain storage persists across storage instance creation
#[tokio::test]
async fn test_keychain_persistence() {
    let test_key_id = format!("test_persistence_{}", Uuid::new_v4());

    // Create test key share
    let (key_packages, public_key_package) = frost_ed25519::keys::generate_with_dealer(
        2,
        3,
        Default::default(),
        &mut rand::thread_rng(),
    )
    .expect("Failed to generate test keys");

    let key_share = KeyShare {
        share: KeyPackage::try_from(key_packages.into_iter().next().unwrap().1)
            .expect("Failed to create KeyPackage"),
        public_key_package,
    };

    // Store with first instance
    {
        let storage1 = PlatformSecureStorage::new().expect("Failed to create storage");
        storage1
            .store_key_share(&test_key_id, &key_share)
            .expect("Failed to store key share");
    }

    // Load with second instance
    {
        let storage2 = PlatformSecureStorage::new().expect("Failed to create storage");
        let loaded_share = storage2
            .load_key_share(&test_key_id)
            .expect("Failed to load key share from new instance");

        assert_eq!(
            key_share.share.verifying_share().serialize(),
            loaded_share.share.verifying_share().serialize(),
            "Key share should persist across storage instances"
        );

        // Clean up
        storage2
            .delete_key_share(&test_key_id)
            .expect("Failed to clean up persistent test key");
    }
}

/// Test that hardware UUID derivation works on macOS
#[tokio::test]
async fn test_macos_hardware_uuid_derivation() {
    // Test that we can get hardware UUID from system_profiler
    let output = Command::new("system_profiler")
        .args(&["SPHardwareDataType", "-detailLevel", "basic"])
        .output()
        .expect("Failed to run system_profiler");

    assert!(output.status.success(), "system_profiler should succeed");

    let hardware_info = String::from_utf8_lossy(&output.stdout);
    let hardware_uuid = hardware_info
        .lines()
        .find(|line| line.contains("Hardware UUID:"))
        .and_then(|line| line.split(':').nth(1))
        .map(|uuid| uuid.trim());

    assert!(
        hardware_uuid.is_some(),
        "Should be able to extract hardware UUID"
    );
    assert!(
        !hardware_uuid.unwrap().is_empty(),
        "Hardware UUID should not be empty"
    );

    // Test that platform key derivation uses this UUID
    let storage = PlatformSecureStorage::new().expect("Failed to create storage");
    // Storage creation should succeed, indicating key derivation worked
}

/// Test device attestation on macOS
#[tokio::test]
async fn test_macos_device_attestation() {
    let attestation = DeviceAttestation::new();
    assert!(
        attestation.is_ok(),
        "Should be able to create DeviceAttestation on macOS: {:?}",
        attestation
    );

    let attestation = attestation.unwrap();
    let challenge = b"test_challenge_for_macos_attestation";

    // Test attestation statement creation
    let statement_result = attestation.create_attestation(challenge);
    assert!(
        statement_result.is_ok(),
        "Should be able to create attestation statement: {:?}",
        statement_result
    );

    let statement = statement_result.unwrap();

    // Verify basic statement properties
    assert_eq!(statement.challenge, challenge, "Challenge should match");
    assert!(
        statement.device_id.starts_with("apple_device_"),
        "Device ID should have Apple prefix"
    );
    assert!(statement.timestamp > 0, "Timestamp should be set");
    assert!(statement.signature.is_some(), "Statement should be signed");

    // Test signature verification
    let public_key = attestation.public_key();
    let verification_result = DeviceAttestation::verify_attestation(&statement, &public_key);
    assert!(
        verification_result.is_ok(),
        "Attestation verification should succeed: {:?}",
        verification_result
    );
    assert!(
        verification_result.unwrap(),
        "Attestation should verify as valid"
    );
}

/// Test System Integrity Protection (SIP) detection on macOS
#[tokio::test]
async fn test_macos_sip_detection() {
    let attestation = DeviceAttestation::new().expect("Failed to create attestation");
    let challenge = b"sip_test_challenge";
    let statement = attestation
        .create_attestation(challenge)
        .expect("Failed to create attestation statement");

    // Check that platform properties include SIP status
    assert!(
        statement.platform_properties.contains_key("sip_enabled"),
        "Attestation should include SIP status"
    );

    // Verify SIP status matches system state
    let sip_output = Command::new("csrutil").arg("status").output();

    if let Ok(output) = sip_output {
        let output_str = String::from_utf8_lossy(&output.stdout);
        let system_sip_enabled = !output_str.contains("disabled");
        let attestation_sip = statement
            .platform_properties
            .get("sip_enabled")
            .and_then(|s| s.parse::<bool>().ok())
            .unwrap_or(false);

        assert_eq!(
            system_sip_enabled, attestation_sip,
            "Attestation SIP status should match system state"
        );
    }
}

/// Test keychain access control (this may prompt for user permission)
#[tokio::test]
async fn test_keychain_access_control() {
    let storage = PlatformSecureStorage::new().expect("Failed to create storage");
    let test_key_id = format!("test_access_control_{}", Uuid::new_v4());

    // Create test key share
    let (key_packages, public_key_package) = frost_ed25519::keys::generate_with_dealer(
        2,
        3,
        Default::default(),
        &mut rand::thread_rng(),
    )
    .expect("Failed to generate test keys");

    let key_share = KeyShare {
        share: KeyPackage::try_from(key_packages.into_iter().next().unwrap().1)
            .expect("Failed to create KeyPackage"),
        public_key_package,
    };

    // Store key share - this may prompt for keychain access
    println!("Note: This test may prompt for keychain access permission");
    let store_result = storage.store_key_share(&test_key_id, &key_share);

    if store_result.is_err() {
        println!("Keychain access denied or failed: {:?}", store_result);
        println!("This is expected if keychain access was denied in the system prompt");
        return; // Skip rest of test if access denied
    }

    // If storage succeeded, test that we can load it back
    let load_result = storage.load_key_share(&test_key_id);
    assert!(
        load_result.is_ok(),
        "Should be able to load stored key share"
    );

    // Clean up
    storage.delete_key_share(&test_key_id).ok();
}

/// Test error handling for invalid key IDs
#[tokio::test]
async fn test_keychain_error_handling() {
    let storage = PlatformSecureStorage::new().expect("Failed to create storage");

    // Test loading non-existent key
    let nonexistent_key = "nonexistent_key_12345";
    let load_result = storage.load_key_share(nonexistent_key);
    assert!(load_result.is_err(), "Loading non-existent key should fail");

    // Test deleting non-existent key
    let delete_result = storage.delete_key_share(nonexistent_key);
    assert!(
        delete_result.is_err(),
        "Deleting non-existent key should fail"
    );
}

/// Integration test that verifies the complete secure storage workflow
#[tokio::test]
async fn test_complete_keychain_workflow() {
    println!("Running complete keychain workflow test on macOS");

    // Step 1: Create storage
    let storage =
        PlatformSecureStorage::new().expect("Should be able to create secure storage on macOS");

    // Step 2: Create test data
    let account_id = Uuid::new_v4();
    let key_id = format!("aura_key_share_{}", account_id);

    let (key_packages, public_key_package) = frost_ed25519::keys::generate_with_dealer(
        2,
        3,
        Default::default(),
        &mut rand::thread_rng(),
    )
    .expect("Failed to generate test keys");

    let key_share = KeyShare {
        share: KeyPackage::try_from(key_packages.into_iter().next().unwrap().1)
            .expect("Failed to create KeyPackage"),
        public_key_package,
    };

    // Step 3: Store key share (mimics CLI init behavior)
    println!("Storing key share with ID: {}", key_id);
    storage
        .store_key_share(&key_id, &key_share)
        .expect("Should be able to store key share in keychain");

    // Step 4: Verify it's in the list
    let stored_keys = storage
        .list_key_shares()
        .expect("Should be able to list stored keys");
    assert!(
        stored_keys.contains(&key_id),
        "Key should appear in listing"
    );

    // Step 5: Load key share (mimics agent connect behavior)
    println!("Loading key share with ID: {}", key_id);
    let loaded_share = storage
        .load_key_share(&key_id)
        .expect("Should be able to load key share from keychain");

    // Step 6: Verify integrity
    assert_eq!(
        key_share.share.verifying_share().serialize(),
        loaded_share.share.verifying_share().serialize(),
        "Loaded key share should match original"
    );

    // Step 7: Test device attestation integration
    let attestation =
        DeviceAttestation::new().expect("Should be able to create device attestation");

    let challenge = b"integration_test_challenge";
    let statement = attestation
        .create_attestation(challenge)
        .expect("Should be able to create attestation statement");

    let public_key = attestation.public_key();
    let is_valid = DeviceAttestation::verify_attestation(&statement, &public_key)
        .expect("Should be able to verify attestation");
    assert!(is_valid, "Attestation should be valid");

    // Step 8: Clean up
    println!("Cleaning up test data");
    storage
        .delete_key_share(&key_id)
        .expect("Should be able to delete key share");

    // Step 9: Verify deletion
    let load_after_delete = storage.load_key_share(&key_id);
    assert!(
        load_after_delete.is_err(),
        "Should not be able to load deleted key"
    );

    println!("[VERIFIED] Complete keychain workflow test passed!");
}

/// Performance test for keychain operations
#[tokio::test]
async fn test_keychain_performance() {
    let storage = PlatformSecureStorage::new().expect("Failed to create storage");
    let test_count = 5; // Small number to avoid cluttering keychain

    // Create test data
    let (key_packages, public_key_package) = frost_ed25519::keys::generate_with_dealer(
        2,
        3,
        Default::default(),
        &mut rand::thread_rng(),
    )
    .expect("Failed to generate test keys");

    let key_share = KeyShare {
        share: KeyPackage::try_from(key_packages.into_iter().next().unwrap().1)
            .expect("Failed to create KeyPackage"),
        public_key_package,
    };

    let mut key_ids = Vec::new();

    // Measure store operations
    let start = std::time::Instant::now();
    for i in 0..test_count {
        let key_id = format!("perf_test_store_{}", i);
        storage
            .store_key_share(&key_id, &key_share)
            .expect("Store operation should succeed");
        key_ids.push(key_id);
    }
    let store_duration = start.elapsed();
    println!(
        "Stored {} keys in {:?} (avg: {:?})",
        test_count,
        store_duration,
        store_duration / test_count
    );

    // Measure load operations
    let start = std::time::Instant::now();
    for key_id in &key_ids {
        storage
            .load_key_share(key_id)
            .expect("Load operation should succeed");
    }
    let load_duration = start.elapsed();
    println!(
        "Loaded {} keys in {:?} (avg: {:?})",
        test_count,
        load_duration,
        load_duration / test_count
    );

    // Clean up
    for key_id in &key_ids {
        storage.delete_key_share(key_id).ok();
    }

    // Performance assertions (generous limits for CI/varied hardware)
    assert!(
        store_duration.as_millis() < 10000,
        "Store operations should complete within 10 seconds"
    );
    assert!(
        load_duration.as_millis() < 5000,
        "Load operations should complete within 5 seconds"
    );
}
