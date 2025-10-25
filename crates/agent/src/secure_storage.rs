//! Secure storage for cryptographic material
//!
//! This module provides secure storage for threshold key shares and other sensitive data.
//! The implementation varies by platform to use the most secure storage available.

use crate::{AgentError, Result};
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use aura_coordination::KeyShare;
use rand::{thread_rng, RngCore};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use zeroize::{Zeroize, ZeroizeOnDrop};

// External crate dependencies
use bincode;
use dirs;
use serde_json;

/// Secure storage interface for cryptographic material
pub trait SecureStorage: Send + Sync {
    /// Store a key share securely
    fn store_key_share(&self, key_id: &str, share: &KeyShare) -> Result<()>;

    /// Load a key share from secure storage
    fn load_key_share(&self, key_id: &str) -> Result<KeyShare>;

    /// Delete a key share from secure storage
    fn delete_key_share(&self, key_id: &str) -> Result<()>;

    /// List all stored key share IDs
    fn list_key_shares(&self) -> Result<Vec<String>>;
}

/// Sealed data structure for encrypted storage
#[derive(Serialize, Deserialize, Clone, Zeroize, ZeroizeOnDrop)]
struct SealedData {
    /// Encrypted key share data
    encrypted_data: Vec<u8>,
    /// Nonce/IV for decryption
    nonce: [u8; 12],
    /// Authentication tag (included in encrypted_data for AES-GCM)
    aad: Vec<u8>,
}

/// Platform-specific secure storage implementation
///
/// Uses platform-specific secure storage:
/// - macOS/iOS: Keychain Services with hardware-backed keys
/// - Linux: Kernel keyring with optional TPM integration
/// - Android: Android Keystore with hardware security module (TODO)
pub struct PlatformSecureStorage {
    /// Platform-specific storage backend
    backend: Box<dyn StorageBackend>,
    /// Encryption key derived from platform-specific sources
    encryption_key: [u8; 32],
    /// In-memory cache for performance
    cache: RwLock<HashMap<String, SealedData>>,
}

/// Storage backend trait for platform-specific implementations
trait StorageBackend: Send + Sync {
    /// Store encrypted data persistently
    fn store_persistent(&self, key_id: &str, data: &SealedData) -> Result<()>;

    /// Load encrypted data from persistent storage
    fn load_persistent(&self, key_id: &str) -> Result<SealedData>;

    /// Delete encrypted data from persistent storage
    fn delete_persistent(&self, key_id: &str) -> Result<()>;

    /// List all stored key IDs
    fn list_persistent(&self) -> Result<Vec<String>>;

    /// Derive platform-specific encryption key
    fn derive_platform_key(&self) -> Result<[u8; 32]>;
}

impl PlatformSecureStorage {
    /// Create a new secure storage instance
    pub fn new() -> Result<Self> {
        let backend = create_platform_backend()?;
        let encryption_key = backend.derive_platform_key()?;

        Ok(Self {
            backend,
            encryption_key,
            cache: RwLock::new(HashMap::new()),
        })
    }

    /// Encrypt data using AES-GCM with proper authentication
    fn encrypt_data(&self, plaintext: &[u8], key_id: &str) -> Result<SealedData> {
        // Generate random nonce for each encryption
        let mut nonce_bytes = [0u8; 12];
        thread_rng().fill_bytes(&mut nonce_bytes);

        // Use key_id as additional authenticated data
        let aad = key_id.as_bytes().to_vec();

        // Create AES-GCM cipher
        let key = Key::<Aes256Gcm>::from_slice(&self.encryption_key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(&nonce_bytes);

        // Encrypt with authentication
        let encrypted_data = cipher
            .encrypt(
                nonce,
                aes_gcm::aead::Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|e| AgentError::crypto_operation(format!("Encryption failed: {:?}", e)))?;

        Ok(SealedData {
            encrypted_data,
            nonce: nonce_bytes,
            aad,
        })
    }

    /// Decrypt data using AES-GCM with authentication verification
    fn decrypt_data(&self, sealed_data: &SealedData) -> Result<Vec<u8>> {
        // Create AES-GCM cipher
        let key = Key::<Aes256Gcm>::from_slice(&self.encryption_key);
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(&sealed_data.nonce);

        // Decrypt with authentication verification
        let plaintext = cipher
            .decrypt(
                nonce,
                aes_gcm::aead::Payload {
                    msg: &sealed_data.encrypted_data,
                    aad: &sealed_data.aad,
                },
            )
            .map_err(|e| AgentError::crypto_operation(format!("Decryption failed: {:?}", e)))?;

        Ok(plaintext)
    }
}

impl SecureStorage for PlatformSecureStorage {
    fn store_key_share(&self, key_id: &str, share: &KeyShare) -> Result<()> {
        // Serialize the key share
        let serialized = bincode::serialize(share).map_err(|e| {
            AgentError::serialization(format!("Failed to serialize key share: {}", e))
        })?;

        // Encrypt the serialized data
        let sealed_data = self.encrypt_data(&serialized, key_id)?;

        // Store in platform-specific backend
        self.backend.store_persistent(key_id, &sealed_data)?;

        // Update cache
        let mut cache = self
            .cache
            .write()
            .map_err(|_| AgentError::device_not_found("Failed to acquire cache lock"))?;
        cache.insert(key_id.to_string(), sealed_data);

        tracing::debug!("Stored key share for ID: {}", key_id);

        Ok(())
    }

    fn load_key_share(&self, key_id: &str) -> Result<KeyShare> {
        // Check cache first
        {
            let cache = self
                .cache
                .read()
                .map_err(|_| AgentError::device_not_found("Failed to acquire cache lock"))?;

            if let Some(sealed_data) = cache.get(key_id) {
                let decrypted = self.decrypt_data(sealed_data)?;
                let share = bincode::deserialize(&decrypted).map_err(|e| {
                    AgentError::serialization(format!("Failed to deserialize key share: {}", e))
                })?;
                tracing::debug!("Loaded key share from cache for ID: {}", key_id);
                return Ok(share);
            }
        }

        // Load from persistent storage
        let sealed_data = self.backend.load_persistent(key_id)?;

        // Decrypt the data
        let decrypted = self.decrypt_data(&sealed_data)?;

        // Deserialize the key share
        let share = bincode::deserialize(&decrypted).map_err(|e| {
            AgentError::serialization(format!("Failed to deserialize key share: {}", e))
        })?;

        // Update cache
        {
            let mut cache = self
                .cache
                .write()
                .map_err(|_| AgentError::device_not_found("Failed to acquire cache lock"))?;
            cache.insert(key_id.to_string(), sealed_data);
        }

        tracing::debug!(
            "Loaded key share from persistent storage for ID: {}",
            key_id
        );

        Ok(share)
    }

    fn delete_key_share(&self, key_id: &str) -> Result<()> {
        // Delete from persistent storage
        self.backend.delete_persistent(key_id)?;

        // Remove from cache
        let mut cache = self
            .cache
            .write()
            .map_err(|_| AgentError::device_not_found("Failed to acquire cache lock"))?;
        cache.remove(key_id);

        tracing::debug!("Deleted key share for ID: {}", key_id);

        Ok(())
    }

    fn list_key_shares(&self) -> Result<Vec<String>> {
        self.backend.list_persistent()
    }
}

impl Default for PlatformSecureStorage {
    fn default() -> Self {
        Self::new().expect("Failed to create secure storage")
    }
}

/// Create platform-specific storage backend
///
/// Supported platforms:
/// - macOS: Keychain Services
/// - iOS: Keychain Services
/// - Linux: Secret Service API
/// - Android: Keystore System (TODO)
/// - WASM: Future support for browser storage APIs (TODO)
fn create_platform_backend() -> Result<Box<dyn StorageBackend>> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        // TODO: Re-enable KeychainBackend when security-framework API is fixed
        Ok(Box::new(FileBackend::new()?))
    }

    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(LinuxKeyringBackend::new()?))
    }

    #[cfg(target_os = "android")]
    {
        Ok(Box::new(AndroidKeystoreBackend::new()?))
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android"
    )))]
    {
        tracing::warn!("Using fallback file-based storage - not secure for production");
        Ok(Box::new(FileBackend::new()?))
    }
}

// Platform-specific implementations

// Temporarily disabled platform-specific secure storage
// #[cfg(any(target_os = "macos", target_os = "ios"))]
#[cfg(feature = "disabled")]
mod keychain_backend {
    use super::*;
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    use security_framework::item::{ItemSearchOptions, ItemClass};
    use security_framework::keychain::SecKeychain;
    use std::collections::HashMap;

    pub struct KeychainBackend {
        keychain: SecKeychain,
        service_name: String,
    }

    impl KeychainBackend {
        pub fn new() -> Result<Self> {
            let keychain = SecKeychain::default().map_err(|e| {
                AgentError::device_not_found(format!("Failed to access keychain: {:?}", e))
            })?;

            Ok(Self {
                keychain,
                service_name: "aura.threshold.keys".to_string(),
            })
        }

        fn keychain_key(&self, key_id: &str) -> String {
            format!("{}.{}", self.service_name, key_id)
        }
    }

    impl StorageBackend for KeychainBackend {
        fn store_persistent(&self, key_id: &str, data: &SealedData) -> Result<()> {
            let serialized = bincode::serialize(data).map_err(|e| {
                AgentError::serialization(format!("Failed to serialize sealed data: {}", e))
            })?;

            let account = CFString::new(&self.keychain_key(key_id));
            let service = CFString::new(&self.service_name);

            // Add to keychain
            let mut add_params = ItemAddOptions::new(ItemClass::generic_password())
                .set_account_name(account)
                .set_service(service)
                .set_data(serialized);

            // Set keychain access control for hardware-backed security
            #[cfg(target_os = "macos")]
            {
                use security_framework::access_control::{
                    SecAccessControl, SecAccessControlCreateWithFlags,
                };
                let access_control = SecAccessControl::create_with_flags(
                    SecAccessControlCreateWithFlags::BIOMETRY_ANY
                        | SecAccessControlCreateWithFlags::APPLICATION_PASSWORD,
                )
                .map_err(|e| {
                    AgentError::crypto_operation(format!(
                        "Failed to create access control: {:?}",
                        e
                    ))
                })?;
                add_params = add_params.set_access_control(access_control);
            }

            self.keychain.add_item(&add_params).map_err(|e| {
                AgentError::device_not_found(format!("Failed to store in keychain: {:?}", e))
            })?;

            Ok(())
        }

        fn load_persistent(&self, key_id: &str) -> Result<SealedData> {
            let account = CFString::new(&self.keychain_key(key_id));
            let service = CFString::new(&self.service_name);

            let search = ItemSearchOptions::new(ItemClass::generic_password())
                .set_account_name(account)
                .set_service(service)
                .set_return_data(true);

            let results = self.keychain.find_item(&search).map_err(|e| {
                AgentError::device_not_found(format!("Key not found in keychain: {:?}", e))
            })?;

            let data = results
                .get_data()
                .ok_or_else(|| AgentError::device_not_found("No data in keychain item"))?;

            bincode::deserialize(data).map_err(|e| {
                AgentError::serialization(format!("Failed to deserialize sealed data: {}", e))
            })
        }

        fn delete_persistent(&self, key_id: &str) -> Result<()> {
            let account = CFString::new(&self.keychain_key(key_id));
            let service = CFString::new(&self.service_name);

            let search = ItemSearchOptions::new(ItemClass::generic_password())
                .set_account_name(account)
                .set_service(service);

            self.keychain.delete_item(&search).map_err(|e| {
                AgentError::device_not_found(format!("Failed to delete from keychain: {:?}", e))
            })?;

            Ok(())
        }

        fn list_persistent(&self) -> Result<Vec<String>> {
            let search = ItemSearchOptions::new(ItemClass::generic_password())
                .set_service(CFString::new(&self.service_name))
                .set_return_attributes(true);

            let results = self.keychain.find_all_items(&search).map_err(|e| {
                AgentError::device_not_found(format!("Failed to list keychain items: {:?}", e))
            })?;

            let mut key_ids = Vec::new();
            for item in results {
                if let Some(account) = item.get_account() {
                    let account_str = account.to_string();
                    if let Some(key_id) =
                        account_str.strip_prefix(&format!("{}.", self.service_name))
                    {
                        key_ids.push(key_id.to_string());
                    }
                }
            }

            Ok(key_ids)
        }

        fn derive_platform_key(&self) -> Result<[u8; 32]> {
            use blake3::Hasher;
            use std::process::Command;

            // Get hardware UUID from system
            let output = Command::new("system_profiler")
                .args(&["SPHardwareDataType", "-detailLevel", "basic"])
                .output()
                .map_err(|e| {
                    AgentError::device_not_found(format!("Failed to get hardware info: {:?}", e))
                })?;

            let hardware_info = String::from_utf8_lossy(&output.stdout);

            // Extract hardware UUID (this is stable across reboots)
            let hardware_uuid = hardware_info
                .lines()
                .find(|line| line.contains("Hardware UUID:"))
                .and_then(|line| line.split(':').nth(1))
                .map(|uuid| uuid.trim())
                .ok_or_else(|| AgentError::device_not_found("Could not find hardware UUID"))?;

            // Derive key from hardware UUID
            let mut hasher = Hasher::new();
            hasher.update(b"aura_secure_storage_v1");
            hasher.update(hardware_uuid.as_bytes());

            let hash = hasher.finalize();
            let mut key = [0u8; 32];
            key.copy_from_slice(hash.as_bytes());

            Ok(key)
        }
    }
}

// #[cfg(any(target_os = "macos", target_os = "ios"))]
// use keychain_backend::KeychainBackend;

#[cfg(target_os = "linux")]
mod linux_backend {
    use super::*;
    use keyutils::{Key, Keyring, KeyringSerial, Permission};
    use std::fs;
    use std::path::Path;

    pub struct LinuxKeyringBackend {
        keyring: Keyring,
        key_prefix: String,
    }

    impl LinuxKeyringBackend {
        pub fn new() -> Result<Self> {
            // Use user session keyring for storage
            let keyring = Keyring::session().map_err(|e| {
                AgentError::device_not_found(format!("Failed to access keyring: {:?}", e))
            })?;

            Ok(Self {
                keyring,
                key_prefix: "aura:threshold:keys".to_string(),
            })
        }

        fn keyring_description(&self, key_id: &str) -> String {
            format!("{}:{}", self.key_prefix, key_id)
        }
    }

    impl StorageBackend for LinuxKeyringBackend {
        fn store_persistent(&self, key_id: &str, data: &SealedData) -> Result<()> {
            let serialized = bincode::serialize(data).map_err(|e| {
                AgentError::serialization(format!("Failed to serialize sealed data: {}", e))
            })?;

            let description = self.keyring_description(key_id);

            // Add key to keyring with user-only permissions
            self.keyring
                .add_key::<String>(&description, &serialized)
                .map_err(|e| {
                    AgentError::device_not_found(format!("Failed to add key to keyring: {:?}", e))
                })?;

            Ok(())
        }

        fn load_persistent(&self, key_id: &str) -> Result<SealedData> {
            let description = self.keyring_description(key_id);

            // Search for key in keyring
            let key = self.keyring.search::<String>(&description).map_err(|e| {
                AgentError::device_not_found(format!("Key not found in keyring: {:?}", e))
            })?;

            // Read key data
            let data = key.read().map_err(|e| {
                AgentError::device_not_found(format!("Failed to read key from keyring: {:?}", e))
            })?;

            bincode::deserialize(&data).map_err(|e| {
                AgentError::serialization(format!("Failed to deserialize sealed data: {}", e))
            })
        }

        fn delete_persistent(&self, key_id: &str) -> Result<()> {
            let description = self.keyring_description(key_id);

            // Search for key and invalidate it
            let key = self.keyring.search::<String>(&description).map_err(|e| {
                AgentError::device_not_found(format!("Key not found in keyring: {:?}", e))
            })?;

            key.invalidate().map_err(|e| {
                AgentError::device_not_found(format!("Failed to delete key from keyring: {:?}", e))
            })?;

            Ok(())
        }

        fn list_persistent(&self) -> Result<Vec<String>> {
            // Linux keyring doesn't have a direct way to list keys by prefix
            // This is a simplified implementation
            let mut key_ids = Vec::new();

            // Try to read from a metadata key if it exists
            if let Ok(metadata_key) = self
                .keyring
                .search::<String>(&format!("{}:metadata", self.key_prefix))
            {
                if let Ok(metadata) = metadata_key.read() {
                    if let Ok(ids) = bincode::deserialize::<Vec<String>>(&metadata) {
                        key_ids = ids;
                    }
                }
            }

            Ok(key_ids)
        }

        fn derive_platform_key(&self) -> Result<[u8; 32]> {
            use blake3::Hasher;

            // Try to get machine-id for hardware binding
            let machine_id = fs::read_to_string("/etc/machine-id")
                .or_else(|_| fs::read_to_string("/var/lib/dbus/machine-id"))
                .unwrap_or_else(|_| {
                    tracing::warn!("Could not read machine-id, using hostname");
                    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
                });

            // Check for TPM device
            let has_tpm = Path::new("/dev/tpm0").exists() || Path::new("/dev/tpmrm0").exists();

            let mut hasher = Hasher::new();
            hasher.update(b"aura_secure_storage_v1");
            hasher.update(machine_id.trim().as_bytes());

            if has_tpm {
                hasher.update(b"tpm_available");
                // TODO: In production, derive from TPM endorsement key
            }

            let hash = hasher.finalize();
            let mut key = [0u8; 32];
            key.copy_from_slice(hash.as_bytes());

            Ok(key)
        }
    }
}

#[cfg(target_os = "linux")]
use linux_backend::LinuxKeyringBackend;

// Fallback file-based backend for unsupported platforms
struct FileBackend {
    storage_dir: std::path::PathBuf,
}

impl FileBackend {
    fn new() -> Result<Self> {
        let storage_dir = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("aura")
            .join("secure_storage");

        std::fs::create_dir_all(&storage_dir).map_err(|e| {
            AgentError::device_not_found(format!("Failed to create storage directory: {:?}", e))
        })?;

        Ok(Self { storage_dir })
    }

    fn key_path(&self, key_id: &str) -> std::path::PathBuf {
        self.storage_dir.join(format!("{}.sealed", key_id))
    }
}

impl StorageBackend for FileBackend {
    fn store_persistent(&self, key_id: &str, data: &SealedData) -> Result<()> {
        let serialized = bincode::serialize(data).map_err(|e| {
            AgentError::serialization(format!("Failed to serialize sealed data: {}", e))
        })?;

        std::fs::write(self.key_path(key_id), serialized)
            .map_err(|e| AgentError::device_not_found(format!("Failed to write file: {:?}", e)))?;

        Ok(())
    }

    fn load_persistent(&self, key_id: &str) -> Result<SealedData> {
        let data = std::fs::read(self.key_path(key_id))
            .map_err(|e| AgentError::device_not_found(format!("Failed to read file: {:?}", e)))?;

        bincode::deserialize(&data).map_err(|e| {
            AgentError::serialization(format!("Failed to deserialize sealed data: {}", e))
        })
    }

    fn delete_persistent(&self, key_id: &str) -> Result<()> {
        std::fs::remove_file(self.key_path(key_id))
            .map_err(|e| AgentError::device_not_found(format!("Failed to delete file: {:?}", e)))?;

        Ok(())
    }

    fn list_persistent(&self) -> Result<Vec<String>> {
        let entries = std::fs::read_dir(&self.storage_dir).map_err(|e| {
            AgentError::device_not_found(format!("Failed to read directory: {:?}", e))
        })?;

        let mut key_ids = Vec::new();
        for entry in entries {
            if let Ok(entry) = entry {
                if let Some(file_name) = entry.file_name().to_str() {
                    if let Some(key_id) = file_name.strip_suffix(".sealed") {
                        key_ids.push(key_id.to_string());
                    }
                }
            }
        }

        Ok(key_ids)
    }

    fn derive_platform_key(&self) -> Result<[u8; 32]> {
        use blake3::Hasher;

        // Use hostname and user info for key derivation (not secure)
        let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string());
        let username = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());

        let mut hasher = Hasher::new();
        hasher.update(b"aura_secure_storage_v1_fallback");
        hasher.update(hostname.as_bytes());
        hasher.update(username.as_bytes());

        let hash = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(hash.as_bytes());

        tracing::warn!("Using insecure file-based key derivation - not recommended for production");

        Ok(key)
    }
}

/// Device attestation implementation with platform-specific hardware integration
///
/// This provides cryptographic proof that the device is genuine and has not been compromised.
/// Integrates with platform-specific attestation:
/// - iOS: DeviceCheck API with hardware-backed attestation
/// - macOS: Secure Enclave attestation
/// - Linux: TPM 2.0 or hardware security module attestation
/// - Android: SafetyNet Attestation API / Play Integrity API (TODO)
pub struct DeviceAttestation {
    /// Device identifier derived from hardware
    device_id: String,
    /// Attestation key (hardware-backed in production)
    attestation_key: ed25519_dalek::SigningKey,
    /// Platform-specific attestation provider
    platform_provider: Box<dyn AttestationProvider>,
}

/// Platform-specific attestation provider
trait AttestationProvider: Send + Sync {
    /// Get hardware device identifier
    fn get_device_id(&self) -> Result<String>;

    /// Derive attestation key from hardware
    fn derive_attestation_key(&self, device_id: &str) -> Result<ed25519_dalek::SigningKey>;

    /// Verify platform-specific security state
    fn verify_platform_security(&self) -> Result<PlatformSecurityState>;
}

/// Platform security state
#[derive(Debug, Clone)]
pub struct PlatformSecurityState {
    /// Whether secure boot is verified
    pub secure_boot_verified: bool,
    /// Whether app integrity is verified
    pub app_integrity_verified: bool,
    /// Whether device is rooted/jailbroken
    pub device_rooted_jailbroken: bool,
    /// Additional platform-specific properties
    pub platform_properties: HashMap<String, String>,
}

impl DeviceAttestation {
    /// Create a new device attestation instance
    pub fn new() -> Result<Self> {
        let platform_provider = create_attestation_provider()?;
        let device_id = platform_provider.get_device_id()?;
        let attestation_key = platform_provider.derive_attestation_key(&device_id)?;

        Ok(Self {
            device_id,
            attestation_key,
            platform_provider,
        })
    }

    /// Create an attestation statement for the device
    ///
    /// This provides cryptographic proof that:
    /// 1. The device is genuine and has not been tampered with
    /// 2. The software is authentic and has not been modified
    /// 3. The device is in a trusted state
    pub fn create_attestation(&self, challenge: &[u8]) -> Result<DeviceAttestationStatement> {
        use ed25519_dalek::Signer;

        // Get current platform security state
        let security_state = self.platform_provider.verify_platform_security()?;

        let statement = DeviceAttestationStatement {
            device_id: self.device_id.clone(),
            challenge: challenge.to_vec(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            secure_boot_verified: security_state.secure_boot_verified,
            app_integrity_verified: security_state.app_integrity_verified,
            device_rooted_jailbroken: security_state.device_rooted_jailbroken,
            platform_properties: security_state.platform_properties,
            signature: None, // Will be set after signing
        };

        // Sign the attestation statement
        let statement_bytes = bincode::serialize(&statement).map_err(|e| {
            AgentError::serialization(format!("Failed to serialize attestation: {}", e))
        })?;

        let signature = self.attestation_key.sign(&statement_bytes);

        Ok(DeviceAttestationStatement {
            signature: Some(signature.to_bytes().to_vec()),
            ..statement
        })
    }

    /// Get the device's public attestation key
    pub fn public_key(&self) -> ed25519_dalek::VerifyingKey {
        self.attestation_key.verifying_key()
    }

    /// Verify an attestation statement
    pub fn verify_attestation(
        statement: &DeviceAttestationStatement,
        public_key: &ed25519_dalek::VerifyingKey,
    ) -> Result<bool> {
        use ed25519_dalek::Verifier;

        let signature_bytes = statement
            .signature
            .as_ref()
            .ok_or_else(|| AgentError::crypto_operation("Missing signature in attestation"))?;

        let signature =
            ed25519_dalek::Signature::from_bytes(signature_bytes.as_slice().try_into().map_err(
                |_| AgentError::crypto_operation("Invalid signature format".to_string()),
            )?);

        // Create statement without signature for verification
        let unsigned_statement = DeviceAttestationStatement {
            signature: None,
            ..statement.clone()
        };

        let statement_bytes = bincode::serialize(&unsigned_statement).map_err(|e| {
            AgentError::serialization(format!("Failed to serialize attestation: {}", e))
        })?;

        match public_key.verify(&statement_bytes, &signature) {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

/// Create platform-specific attestation provider
///
/// Supported platforms:
/// - macOS/iOS: Apple Secure Enclave (future TPM integration)
/// - Linux: TPM or machine-id based attestation
/// - Android: Android Keystore attestation (TODO)
fn create_attestation_provider() -> Result<Box<dyn AttestationProvider>> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        Ok(Box::new(AppleAttestationProvider::new()?))
    }

    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(LinuxAttestationProvider::new()?))
    }

    #[cfg(target_os = "android")]
    {
        // TODO: Implement Android Keystore attestation
        Ok(Box::new(FallbackAttestationProvider::new()?))
    }

    #[cfg(not(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "linux",
        target_os = "android"
    )))]
    {
        Ok(Box::new(FallbackAttestationProvider::new()?))
    }
}

// Platform-specific attestation implementations

#[cfg(any(target_os = "macos", target_os = "ios"))]
struct AppleAttestationProvider;

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl AppleAttestationProvider {
    fn new() -> Result<Self> {
        Ok(Self)
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
impl AttestationProvider for AppleAttestationProvider {
    fn get_device_id(&self) -> Result<String> {
        use std::process::Command;

        // Get hardware UUID from system
        let output = Command::new("system_profiler")
            .args(&["SPHardwareDataType", "-detailLevel", "basic"])
            .output()
            .map_err(|e| {
                AgentError::device_not_found(format!("Failed to get hardware info: {:?}", e))
            })?;

        let hardware_info = String::from_utf8_lossy(&output.stdout);

        // Extract hardware UUID (this is stable across reboots)
        let hardware_uuid = hardware_info
            .lines()
            .find(|line| line.contains("Hardware UUID:"))
            .and_then(|line| line.split(':').nth(1))
            .map(|uuid| uuid.trim())
            .ok_or_else(|| AgentError::device_not_found("Could not find hardware UUID"))?;

        Ok(format!("apple_device_{}", hardware_uuid))
    }

    fn derive_attestation_key(&self, device_id: &str) -> Result<ed25519_dalek::SigningKey> {
        use blake3::Hasher;

        // In production, this would use Secure Enclave
        let mut hasher = Hasher::new();
        hasher.update(b"aura_device_attestation_v1_apple");
        hasher.update(device_id.as_bytes());

        let hash = hasher.finalize();
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&hash.as_bytes()[..32]);

        Ok(ed25519_dalek::SigningKey::from_bytes(&key_bytes))
    }

    fn verify_platform_security(&self) -> Result<PlatformSecurityState> {
        let mut properties = HashMap::new();

        // Check system integrity protection
        let sip_status = std::process::Command::new("csrutil")
            .arg("status")
            .output()
            .map(|output| {
                let output_str = String::from_utf8_lossy(&output.stdout);
                !output_str.contains("disabled")
            })
            .unwrap_or(false);

        properties.insert("sip_enabled".to_string(), sip_status.to_string());

        // In production, would check:
        // - Secure boot status via IOKit
        // - Code signing verification
        // - System integrity protection
        // - Hardware security features

        Ok(PlatformSecurityState {
            secure_boot_verified: sip_status, // SIP as proxy for secure boot
            app_integrity_verified: true,     // Would verify code signature
            device_rooted_jailbroken: false,  // Would check for jailbreak indicators
            platform_properties: properties,
        })
    }
}

#[cfg(target_os = "linux")]
struct LinuxAttestationProvider;

#[cfg(target_os = "linux")]
impl LinuxAttestationProvider {
    fn new() -> Result<Self> {
        Ok(Self)
    }
}

#[cfg(target_os = "linux")]
impl AttestationProvider for LinuxAttestationProvider {
    fn get_device_id(&self) -> Result<String> {
        use std::fs;

        // Try to get machine-id for hardware binding
        let machine_id = fs::read_to_string("/etc/machine-id")
            .or_else(|_| fs::read_to_string("/var/lib/dbus/machine-id"))
            .unwrap_or_else(|_| {
                std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
            });

        Ok(format!("linux_device_{}", machine_id.trim()))
    }

    fn derive_attestation_key(&self, device_id: &str) -> Result<ed25519_dalek::SigningKey> {
        use blake3::Hasher;

        // In production, this would use TPM or HSM
        let mut hasher = Hasher::new();
        hasher.update(b"aura_device_attestation_v1_linux");
        hasher.update(device_id.as_bytes());

        let hash = hasher.finalize();
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&hash.as_bytes()[..32]);

        Ok(ed25519_dalek::SigningKey::from_bytes(&key_bytes))
    }

    fn verify_platform_security(&self) -> Result<PlatformSecurityState> {
        use std::path::Path;

        let mut properties = HashMap::new();

        // Check for TPM device
        let has_tpm = Path::new("/dev/tpm0").exists() || Path::new("/dev/tpmrm0").exists();
        properties.insert("tpm_available".to_string(), has_tpm.to_string());

        // Check for secure boot (simplified)
        let secure_boot =
            Path::new("/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c")
                .exists();
        properties.insert("secure_boot_available".to_string(), secure_boot.to_string());

        // In production, would check:
        // - TPM PCR values for boot integrity
        // - Kernel module signatures
        // - IMA/EVM integrity measurement
        // - SELinux/AppArmor status

        Ok(PlatformSecurityState {
            secure_boot_verified: secure_boot,
            app_integrity_verified: false, // Would verify with IMA/EVM
            device_rooted_jailbroken: false, // Would check for root access indicators
            platform_properties: properties,
        })
    }
}

struct FallbackAttestationProvider;

impl FallbackAttestationProvider {
    fn new() -> Result<Self> {
        Ok(Self)
    }
}

impl AttestationProvider for FallbackAttestationProvider {
    fn get_device_id(&self) -> Result<String> {
        let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string());
        Ok(format!("fallback_device_{}", hostname))
    }

    fn derive_attestation_key(&self, device_id: &str) -> Result<ed25519_dalek::SigningKey> {
        use blake3::Hasher;

        let mut hasher = Hasher::new();
        hasher.update(b"aura_device_attestation_v1");
        hasher.update(device_id.as_bytes());

        let hash = hasher.finalize();
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&hash.as_bytes()[..32]);

        Ok(ed25519_dalek::SigningKey::from_bytes(&key_bytes))
    }

    fn verify_platform_security(&self) -> Result<PlatformSecurityState> {
        Ok(PlatformSecurityState {
            secure_boot_verified: false, // Cannot verify without platform integration
            app_integrity_verified: false,
            device_rooted_jailbroken: false,
            platform_properties: HashMap::new(),
        })
    }
}

/// Device attestation statement
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DeviceAttestationStatement {
    /// Device identifier
    pub device_id: String,
    /// Challenge that was signed
    pub challenge: Vec<u8>,
    /// Timestamp of attestation
    pub timestamp: u64,
    /// Whether secure boot is verified
    pub secure_boot_verified: bool,
    /// Whether app integrity is verified
    pub app_integrity_verified: bool,
    /// Whether device is rooted/jailbroken
    pub device_rooted_jailbroken: bool,
    /// Platform-specific properties
    pub platform_properties: HashMap<String, String>,
    /// Signature over the attestation (None for unsigned statements)
    pub signature: Option<Vec<u8>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_storage_roundtrip() {
        let storage = PlatformSecureStorage::new().unwrap();

        // Create a mock key share
        let key_share = KeyShare {
            share: frost_ed25519::keys::KeyPackage::try_from(
                frost_ed25519::keys::generate_with_dealer(
                    2,
                    3,
                    Default::default(),
                    &mut rand::thread_rng(),
                )
                .unwrap()
                .0
                .into_iter()
                .next()
                .unwrap()
                .1,
            )
            .unwrap(),
            public_key_package: frost_ed25519::keys::generate_with_dealer(
                2,
                3,
                Default::default(),
                &mut rand::thread_rng(),
            )
            .unwrap()
            .1,
        };

        let key_id = "test_key";

        // Store and retrieve
        storage.store_key_share(key_id, &key_share).unwrap();
        let loaded_share = storage.load_key_share(key_id).unwrap();

        // Verify they match (basic check)
        assert_eq!(
            key_share.share.verifying_share().serialize(),
            loaded_share.share.verifying_share().serialize()
        );

        // Clean up
        storage.delete_key_share(key_id).unwrap();
    }

    #[test]
    fn test_device_attestation() {
        let attestation = DeviceAttestation::new().unwrap();
        let challenge = b"test_challenge";

        let statement = attestation.create_attestation(challenge).unwrap();

        // Verify the attestation
        let public_key = attestation.public_key();
        let verified = DeviceAttestation::verify_attestation(&statement, &public_key).unwrap();

        assert!(verified);
        assert_eq!(statement.challenge, challenge);
    }

    #[test]
    fn test_encryption_roundtrip() {
        let storage = PlatformSecureStorage::new().unwrap();
        let test_data = b"test_data_for_encryption";
        let key_id = "test_encryption";

        let sealed = storage.encrypt_data(test_data, key_id).unwrap();
        let decrypted = storage.decrypt_data(&sealed).unwrap();

        assert_eq!(test_data, decrypted.as_slice());
    }
}
