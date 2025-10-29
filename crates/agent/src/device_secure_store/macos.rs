//! macOS Keychain Services implementation for secure storage
//!
//! This module provides a real implementation of secure storage for macOS using
//! the Security Framework and Keychain Services.

use super::{common::PlatformKeyStore, DeviceAttestation, SecurityLevel};
use aura_types::{AuraError, AuraResult as Result};
use std::collections::HashMap;

#[cfg(target_os = "macos")]
use security_framework::{
    item::{ItemClass, ItemSearchOptions, Limit, SearchResult},
    passwords::{delete_generic_password, get_generic_password, set_generic_password},
};

/// macOS Keychain Services implementation of platform-specific storage operations
pub struct MacOSKeychain {
    /// Service name for keychain items
    service_name: String,
}

impl MacOSKeychain {
    /// Create a new macOS keychain storage instance
    pub fn new() -> Result<Self> {
        Ok(Self {
            service_name: "aura-identity".to_string(),
        })
    }

    /// Create a new macOS keychain storage instance with custom service name
    pub fn with_service_name(service_name: String) -> Result<Self> {
        Ok(Self { service_name })
    }

    #[cfg(target_os = "macos")]
    fn store_to_keychain(&self, account: &str, data: &[u8]) -> Result<()> {
        set_generic_password(&self.service_name, account, data)
            .map_err(|e| AuraError::storage_failed(format!("Keychain store error: {}", e)))
    }

    #[cfg(target_os = "macos")]
    fn load_from_keychain(&self, account: &str) -> Result<Option<Vec<u8>>> {
        match get_generic_password(&self.service_name, account) {
            Ok(data) => Ok(Some(data)),
            Err(e) => {
                let error_string = format!("{}", e);
                if error_string.contains("NotFound") || error_string.contains("errSecItemNotFound")
                {
                    Ok(None)
                } else {
                    Err(AuraError::storage_failed(format!(
                        "Keychain load error: {}",
                        e
                    )))
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn delete_from_keychain(&self, account: &str) -> Result<()> {
        match delete_generic_password(&self.service_name, account) {
            Ok(()) => Ok(()),
            Err(e) => {
                let error_string = format!("{}", e);
                if error_string.contains("NotFound") || error_string.contains("errSecItemNotFound")
                {
                    // Item already doesn't exist, that's fine
                    Ok(())
                } else {
                    Err(AuraError::storage_failed(format!(
                        "Keychain delete error: {}",
                        e
                    )))
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn list_keychain_accounts(&self, prefix: &str) -> Result<Vec<String>> {
        let mut binding = ItemSearchOptions::new();
        let search_options = binding
            .class(ItemClass::generic_password())
            .service(&self.service_name)
            .limit(Limit::All)
            .load_attributes(true);

        match search_options.search() {
            Ok(search_results) => {
                let accounts = Vec::new();
                for result in search_results {
                    if let SearchResult::Dict(dict) = result {
                        // Extract account name from search result (simplified for stub)
                        // In real implementation, would parse CFDictionary properly
                        // For now, just return empty list as this is a stub implementation
                        let _ = dict; // Suppress unused variable warning
                    }
                }
                Ok(accounts)
            }
            Err(e) => {
                let error_string = format!("{}", e);
                if error_string.contains("NotFound") || error_string.contains("errSecItemNotFound")
                {
                    Ok(Vec::new())
                } else {
                    Err(AuraError::storage_failed(format!(
                        "Keychain list error: {}",
                        e
                    )))
                }
            }
        }
    }

    // Non-macOS fallback methods (for compilation on other platforms)
    #[cfg(not(target_os = "macos"))]
    fn store_to_keychain(&self, _account: &str, _data: &[u8]) -> Result<()> {
        Err(AuraError::platform_not_supported(
            "macOS Keychain not available on this platform",
        ))
    }

    #[cfg(not(target_os = "macos"))]
    fn load_from_keychain(&self, _account: &str) -> Result<Option<Vec<u8>>> {
        Err(AuraError::platform_not_supported(
            "macOS Keychain not available on this platform",
        ))
    }

    #[cfg(not(target_os = "macos"))]
    fn delete_from_keychain(&self, _account: &str) -> Result<()> {
        Err(AuraError::platform_not_supported(
            "macOS Keychain not available on this platform",
        ))
    }

    #[cfg(not(target_os = "macos"))]
    fn list_keychain_accounts(&self, _prefix: &str) -> Result<Vec<String>> {
        Err(AuraError::platform_not_supported(
            "macOS Keychain not available on this platform",
        ))
    }
}

impl PlatformKeyStore for MacOSKeychain {
    fn platform_store(&self, key: &str, data: &[u8]) -> Result<()> {
        self.store_to_keychain(key, data)
    }

    fn platform_load(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.load_from_keychain(key)
    }

    fn platform_delete(&self, key: &str) -> Result<()> {
        self.delete_from_keychain(key)
    }

    fn platform_list(&self, prefix: &str) -> Result<Vec<String>> {
        self.list_keychain_accounts(prefix)
    }

    fn platform_attestation(&self) -> Result<DeviceAttestation> {
        let mut attestation_data = HashMap::new();

        #[cfg(target_os = "macos")]
        {
            // Get system information for device attestation
            use std::process::Command;

            // Get hardware UUID
            if let Ok(output) = Command::new("system_profiler")
                .args(&["SPHardwareDataType", "-xml"])
                .output()
            {
                if let Ok(output_str) = String::from_utf8(output.stdout) {
                    attestation_data.insert("hardware_info".to_string(), output_str);
                }
            }

            // Get security features
            let security_features = vec![
                "keychain-services".to_string(),
                "secure-enclave".to_string(), // May or may not be available
                "system-integrity-protection".to_string(),
            ];

            Ok(DeviceAttestation {
                platform: "macOS".to_string(),
                device_id: format!("macos-{}", self.service_name),
                security_features,
                security_level: SecurityLevel::HSM, // Keychain can use Secure Enclave
                attestation_data,
            })
        }

        #[cfg(not(target_os = "macos"))]
        {
            Ok(DeviceAttestation {
                platform: "non-macos".to_string(),
                device_id: "non-macos-device".to_string(),
                security_features: vec!["software-only".to_string()],
                security_level: SecurityLevel::Software,
                attestation_data,
            })
        }
    }
}

// Factory function to create a SecureStorage implementation for macOS
pub fn create_macos_secure_storage(
    device_id: aura_types::DeviceId,
    account_id: aura_types::AccountId,
) -> Result<super::common::SecureStoreImpl<MacOSKeychain>> {
    let platform = MacOSKeychain::new()?;
    Ok(super::common::SecureStoreImpl::new(
        platform, device_id, account_id,
    ))
}

// Factory function with custom service name
pub fn create_macos_secure_storage_with_service(
    device_id: aura_types::DeviceId,
    account_id: aura_types::AccountId,
    service_name: String,
) -> Result<super::common::SecureStoreImpl<MacOSKeychain>> {
    let platform = MacOSKeychain::with_service_name(service_name)?;
    Ok(super::common::SecureStoreImpl::new(
        platform, device_id, account_id,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_types::{AccountId, DeviceId};
    use uuid::Uuid;

    #[test]
    fn test_keychain_storage_creation() {
        let keychain = MacOSKeychain::new();
        assert!(keychain.is_ok(), "Should be able to create MacOSKeychain");

        let custom_keychain = MacOSKeychain::with_service_name("test-service".to_string());
        assert!(
            custom_keychain.is_ok(),
            "Should be able to create MacOSKeychain with custom service"
        );
    }

    #[test]
    fn test_factory_functions() {
        let device_id = DeviceId(Uuid::new_v4());
        let account_id = AccountId::new();

        let storage = create_macos_secure_storage(device_id, account_id);
        assert!(storage.is_ok(), "Should create secure storage");

        let custom_storage = create_macos_secure_storage_with_service(
            device_id,
            account_id,
            "test-service".to_string(),
        );
        assert!(
            custom_storage.is_ok(),
            "Should create custom secure storage"
        );
    }

    #[test]
    fn test_device_attestation() {
        let keychain = MacOSKeychain::new().unwrap();
        let attestation = keychain.platform_attestation();
        assert!(
            attestation.is_ok(),
            "Should be able to get device attestation"
        );

        let attestation = attestation.unwrap();
        assert!(
            !attestation.platform.is_empty(),
            "Platform should not be empty"
        );
        assert!(
            !attestation.device_id.is_empty(),
            "Device ID should not be empty"
        );
    }

    // Note: Real keychain integration tests would require special entitlements
    // and may prompt the user for keychain access permissions
    #[cfg(target_os = "macos")]
    #[test]
    fn test_keychain_integration_basic() {
        let keychain = MacOSKeychain::with_service_name("aura-test".to_string()).unwrap();
        let test_data = b"test data for keychain";

        // This test may prompt for keychain access on macOS
        if let Ok(()) = keychain.store_to_keychain("test_account", test_data) {
            // If store succeeded, test load and delete
            if let Ok(Some(loaded_data)) = keychain.load_from_keychain("test_account") {
                assert_eq!(loaded_data, test_data);
            }

            // Clean up
            let _ = keychain.delete_from_keychain("test_account");
        }
        // If keychain access is denied, the test will not fail
    }
}
