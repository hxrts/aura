//! Agent Basic Example
//!
//! This example demonstrates basic agent functionality using only
//! the core types that are currently available and working.

use aura_agent::{DerivedIdentity, DeviceAttestation, Result, SecurityLevel};
use aura_types::{AccountId, DeviceId};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("=== Aura Agent: Basic Usage Example ===\n");

    // 1. Create device and account identifiers
    let device_id = DeviceId::new();
    let account_id = AccountId::new();

    println!("Created device: {}", device_id);
    println!("Created account: {}\n", account_id);

    // 2. Demonstrate DerivedIdentity creation
    println!("Creating derived identity...");
    let identity = DerivedIdentity {
        app_id: "my-app".to_string(),
        context: "user-session".to_string(),
        identity_key: vec![1, 2, 3, 4], // Placeholder key data
        proof: vec![5, 6, 7, 8],        // Placeholder proof data
    };
    println!("[OK] Derived identity for app: {}", identity.app_id);
    println!("     Context: {}", identity.context);
    println!("     Key length: {} bytes", identity.identity_key.len());
    println!("     Proof length: {} bytes\n", identity.proof.len());

    // 3. Demonstrate device attestation
    println!("Creating device attestation...");
    let attestation = DeviceAttestation {
        platform: "Test Platform".to_string(),
        device_id: device_id.to_string(),
        security_features: vec![
            "Hardware-backed keys".to_string(),
            "AES-256 encryption".to_string(),
            "Secure boot".to_string(),
        ],
        security_level: SecurityLevel::TEE,
        attestation_data: [
            ("api_level".to_string(), "30".to_string()),
            ("security_patch".to_string(), "2023-10".to_string()),
        ]
        .into_iter()
        .collect(),
    };

    println!("[OK] Device attestation created:");
    println!("     Platform: {}", attestation.platform);
    println!("     Security Level: {:?}", attestation.security_level);
    println!("     Features: {:?}", attestation.security_features);

    // 4. Show serialization capabilities
    println!("\nTesting serialization...");
    let identity_json =
        serde_json::to_string_pretty(&identity).expect("Failed to serialize identity");
    println!("[OK] Identity serialized to JSON:");
    println!("{}", identity_json);

    println!("\n=== Example completed successfully! ===");
    println!("\nThis example demonstrates:");
    println!("• Basic agent type usage");
    println!("• Device and account ID management");
    println!("• Identity derivation structures");
    println!("• Device attestation creation");
    println!("• JSON serialization support");

    Ok(())
}

/// Example of working with secure storage interface
#[allow(dead_code)]
async fn example_secure_storage_interface() {
    println!("\n=== Secure Storage Interface Example ===");

    // This demonstrates the SecureStorage trait interface
    // without requiring a working implementation
    println!("Secure storage operations available:");
    println!("• store_key_share(key_id, key_share)");
    println!("• load_key_share(key_id) -> Option<KeyShare>");
    println!("• delete_key_share(key_id)");
    println!("• list_key_shares() -> Vec<String>");
    println!("• store_secure_data(key, data)");
    println!("• load_secure_data(key) -> Option<Vec<u8>>");
    println!("• delete_secure_data(key)");
    println!("• get_device_attestation() -> DeviceAttestation");

    println!("\nSecurity levels supported:");
    println!("• Software: {:?}", SecurityLevel::Software);
    println!("• TEE: {:?}", SecurityLevel::TEE);
    println!("• StrongBox: {:?}", SecurityLevel::StrongBox);
}
