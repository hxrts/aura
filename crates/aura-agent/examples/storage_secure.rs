//! Storage Secure Example
//!
//! This example demonstrates the secure storage interfaces and
//! platform-specific implementations available in the agent crate.

use aura_agent::{DeviceAttestation, SecureStorage, SecurityLevel};
use aura_crypto::KeyShare;
use aura_types::{AuraResult as Result, DeviceId};
use std::collections::HashMap;

/// Mock secure storage implementation for demonstration
#[derive(Debug)]
#[allow(dead_code)]
struct MockSecureStorage {
    platform: String,
    security_level: SecurityLevel,
    storage: HashMap<String, Vec<u8>>,
}

impl MockSecureStorage {
    fn new(platform: &str, security_level: SecurityLevel) -> Self {
        Self {
            platform: platform.to_string(),
            security_level,
            storage: HashMap::new(),
        }
    }
}

impl SecureStorage for MockSecureStorage {
    fn store_key_share(&self, key_id: &str, _key_share: &KeyShare) -> Result<()> {
        println!("Storing key share: {}", key_id);
        // In real implementation, would serialize and encrypt the key_share
        Ok(())
    }

    fn load_key_share(&self, key_id: &str) -> Result<Option<KeyShare>> {
        println!("Loading key share: {}", key_id);
        // In real implementation, would decrypt and deserialize
        Ok(None) // Mock returns None
    }

    fn delete_key_share(&self, key_id: &str) -> Result<()> {
        println!("Deleting key share: {}", key_id);
        Ok(())
    }

    fn list_key_shares(&self) -> Result<Vec<String>> {
        println!("Listing key shares");
        Ok(vec!["mock_key_1".to_string(), "mock_key_2".to_string()])
    }

    fn store_secure_data(&self, key: &str, data: &[u8]) -> Result<()> {
        println!(
            "Storing {} bytes of secure data with key: {}",
            data.len(),
            key
        );
        Ok(())
    }

    fn load_secure_data(&self, key: &str) -> Result<Option<Vec<u8>>> {
        println!("Loading secure data with key: {}", key);
        Ok(Some(vec![1, 2, 3, 4])) // Mock data
    }

    fn delete_secure_data(&self, key: &str) -> Result<()> {
        println!("Deleting secure data with key: {}", key);
        Ok(())
    }

    fn get_device_attestation(&self) -> Result<DeviceAttestation> {
        Ok(DeviceAttestation {
            platform: self.platform.clone(),
            device_id: DeviceId::new().to_string(),
            security_features: match self.security_level {
                SecurityLevel::StrongBox => vec![
                    "StrongBox Secure Element".to_string(),
                    "Hardware-backed keys".to_string(),
                    "Tamper resistance".to_string(),
                ],
                SecurityLevel::HSM => vec![
                    "Hardware Security Module".to_string(),
                    "Hardware-backed keys".to_string(),
                    "Tamper resistance".to_string(),
                ],
                SecurityLevel::TEE => vec![
                    "Trusted Execution Environment".to_string(),
                    "Hardware-backed keys".to_string(),
                ],
                SecurityLevel::Software => vec![
                    "Software keystore".to_string(),
                    "AES-256 encryption".to_string(),
                ],
            },
            security_level: self.security_level.clone(),
            attestation_data: [
                ("mock".to_string(), "true".to_string()),
                ("version".to_string(), "1.0.0".to_string()),
            ]
            .into_iter()
            .collect(),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    println!("=== Secure Storage Demonstration ===\n");

    // Demonstrate different security levels
    let storage_configs = vec![
        ("Android StrongBox", SecurityLevel::StrongBox),
        ("iOS Secure Enclave", SecurityLevel::TEE),
        ("Software Fallback", SecurityLevel::Software),
    ];

    for (platform, security_level) in storage_configs {
        println!("Testing {} ({:?})", platform, security_level);

        let storage = MockSecureStorage::new(platform, security_level);
        demonstrate_storage_operations(&storage).await?;
        println!();
    }

    // Demonstrate platform-specific features
    println!("Platform-Specific Features:");
    println!("• Android: Keystore API, StrongBox, Biometric auth");
    println!("• iOS: Secure Enclave, Keychain Services, Touch/Face ID");
    println!("• macOS: Keychain Services, Secure Enclave (T2+ chips)");
    println!("• Linux: Secret Service API, TPM integration");

    println!("\n=== Example completed successfully! ===");

    Ok(())
}

async fn demonstrate_storage_operations(storage: &dyn SecureStorage) -> Result<()> {
    // Get device attestation
    let attestation = storage.get_device_attestation()?;
    println!("   Platform: {}", attestation.platform);
    println!("   Security Level: {:?}", attestation.security_level);
    println!("   Features: {:?}", attestation.security_features);

    // Demonstrate secure data operations
    storage.store_secure_data("user_preference", b"dark_mode_enabled")?;

    if let Some(data) = storage.load_secure_data("user_preference")? {
        println!("   Loaded {} bytes of secure data", data.len());
    }

    // Demonstrate key share operations
    let key_shares = storage.list_key_shares()?;
    println!("   Found {} key shares: {:?}", key_shares.len(), key_shares);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_storage_operations() {
        let storage = MockSecureStorage::new("Test Platform", SecurityLevel::Software);

        // Test attestation
        let attestation = storage.get_device_attestation().unwrap();
        assert_eq!(attestation.platform, "Test Platform");
        assert_eq!(attestation.security_level, SecurityLevel::Software);

        // Test secure data operations
        storage.store_secure_data("test_key", b"test_data").unwrap();
        let loaded = storage.load_secure_data("test_key").unwrap();
        assert!(loaded.is_some());

        // Test key share operations
        let key_shares = storage.list_key_shares().unwrap();
        assert_eq!(key_shares.len(), 2);
    }

    #[test]
    fn test_security_level_hierarchy() {
        // Test that security levels have expected properties
        let levels = vec![
            SecurityLevel::Software,
            SecurityLevel::TEE,
            SecurityLevel::StrongBox,
        ];

        for level in levels {
            let storage = MockSecureStorage::new("Test", level);
            let attestation = storage.get_device_attestation().unwrap();

            // All should have at least basic security features
            assert!(!attestation.security_features.is_empty());
            assert_eq!(attestation.security_level, level);
        }
    }
}
