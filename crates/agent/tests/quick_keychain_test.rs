//! Quick macOS keychain validation test
//!
//! This is a minimal test to verify that the keychain system compiles and
//! basic functionality is available without requiring the full test suite.

#![cfg(target_os = "macos")]

use aura_agent::device_secure_store::{DeviceAttestation, PlatformSecureStorage};
use aura_types::{AccountId, DeviceId};
use uuid::Uuid;

/// Quick compilation and instantiation test
#[tokio::test]
async fn test_keychain_system_available() {
    // Test that we can create the secure storage system
    let device_id = DeviceId(Uuid::new_v4());
    let account_id = AccountId(Uuid::new_v4());
    let storage_result = PlatformSecureStorage::new(device_id, account_id);
    println!(
        "PlatformSecureStorage creation result: {:?}",
        storage_result
            .as_ref()
            .map(|_| "Success")
            .map_err(|e| e.to_string())
    );

    // This test passes if we can create the storage instance
    // Even if keychain access is denied, creation should succeed
    assert!(
        storage_result.is_ok(),
        "Should be able to create PlatformSecureStorage instance"
    );

    // Test that we can create device attestation
    let attestation_result = DeviceAttestation::new();
    println!(
        "DeviceAttestation creation result: {:?}",
        attestation_result
            .as_ref()
            .map(|_| "Success")
            .map_err(|e| e.to_string())
    );

    assert!(
        attestation_result.is_ok(),
        "Should be able to create DeviceAttestation instance"
    );
}

/// Test that the backend selection works correctly on macOS
#[test]
fn test_macos_backend_selection() {
    // This test verifies that the correct backend is selected at compile time
    // On macOS, we should get the Keychain backend

    // We can't easily test the internal backend selection without exposing internals,
    // but we can verify that creation works and assume the right backend is selected
    let device_id = DeviceId(Uuid::new_v4());
    let account_id = AccountId(Uuid::new_v4());
    let storage = PlatformSecureStorage::new(device_id, account_id);
    assert!(
        storage.is_ok(),
        "macOS should be able to create keychain backend"
    );
}

/// Test hardware UUID extraction capability
#[test]
fn test_hardware_uuid_extraction() {
    use std::process::Command;

    // Test that we can run system_profiler (required for hardware UUID extraction)
    let output = Command::new("system_profiler")
        .args(&["SPHardwareDataType", "-detailLevel", "basic"])
        .output();

    assert!(output.is_ok(), "Should be able to run system_profiler");

    let output = output.unwrap();
    assert!(
        output.status.success(),
        "system_profiler should execute successfully"
    );

    let output_str = String::from_utf8_lossy(&output.stdout);
    assert!(
        output_str.contains("Hardware UUID:"),
        "Output should contain Hardware UUID"
    );

    println!("[OK] Hardware UUID extraction capability verified");
}

/// Test SIP (System Integrity Protection) detection
#[test]
fn test_sip_detection() {
    use std::process::Command;

    // Test that we can check SIP status
    let output = Command::new("csrutil").arg("status").output();

    if let Ok(output) = output {
        let output_str = String::from_utf8_lossy(&output.stdout);
        println!("SIP Status: {}", output_str.trim());

        // We don't assert on the SIP state since it varies by system,
        // but we verify we can check it
        assert!(
            output_str.contains("System Integrity Protection"),
            "Output should mention System Integrity Protection"
        );

        println!("[OK] SIP detection capability verified");
    } else {
        println!("[WARNING] csrutil not available or accessible");
    }
}
