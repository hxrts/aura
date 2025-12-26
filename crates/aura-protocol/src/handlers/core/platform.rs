//! Platform detection utilities
//!
//! Automatic detection of platform capabilities including secure enclaves,
//! storage backends, and network interfaces.

use aura_core::effects::StorageEffects;
use aura_effects::storage::FilesystemStorageHandler;

use super::FactoryError;

/// Platform detection utilities
pub struct PlatformDetector;

impl PlatformDetector {
    /// Detect the current platform
    pub fn detect_platform() -> Result<PlatformInfo, FactoryError> {
        let storage: std::sync::Arc<dyn StorageEffects> =
            std::sync::Arc::new(PathStorageAdapter::with_default_path());
        Self::detect_platform_with_storage(storage.as_ref())
    }

    /// Detect the current platform using provided storage effects (for deterministic tests)
    pub fn detect_platform_with_storage(
        storage: &dyn StorageEffects,
    ) -> Result<PlatformInfo, FactoryError> {
        Ok(PlatformInfo {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            has_secure_enclave: Self::detect_secure_enclave(storage),
            available_storage_backends: Self::detect_storage_backends(),
            available_network_interfaces: Self::detect_network_interfaces(),
        })
    }

    /// Detect if secure enclave is available
    fn detect_secure_enclave(_storage: &dyn StorageEffects) -> bool {
        // Platform-specific detection logic
        match std::env::consts::OS {
            "macos" => {
                // Check for Apple Secure Enclave on macOS
                std::env::consts::ARCH == "aarch64"
                    || std::process::Command::new("system_profiler")
                        .args(["SPHardwareDataType"])
                        .output()
                        .map(|output| String::from_utf8_lossy(&output.stdout).contains("Apple"))
                        .unwrap_or(false)
            }
            "linux" => {
                // Check for Intel SGX or AMD SEV on Linux
                std::path::Path::new("/dev/sgx_enclave").exists()
                    || std::path::Path::new("/dev/sgx/enclave").exists()
                    || std::path::Path::new("/dev/sev").exists()
            }
            "windows" => {
                // Check for Intel SGX on Windows (conservative approach)
                std::env::var("PROCESSOR_IDENTIFIER")
                    .map(|proc| proc.to_lowercase().contains("intel"))
                    .unwrap_or(false)
            }
            _ => false, // Conservative default for other platforms
        }
    }

    /// Detect available storage backends
    fn detect_storage_backends() -> Vec<String> {
        let mut backends = vec!["memory".to_string()];

        // Always available
        backends.push("filesystem".to_string());

        // Platform-specific backends
        #[cfg(target_os = "macos")]
        backends.push("keychain".to_string());

        #[cfg(target_os = "windows")]
        backends.push("credential_store".to_string());

        #[cfg(target_os = "linux")]
        backends.push("secret_service".to_string());

        backends
    }

    /// Detect available network interfaces
    fn detect_network_interfaces() -> Vec<String> {
        let mut interfaces = Vec::new();

        // Always include loopback
        interfaces.push("loopback".to_string());

        // Platform-specific interface detection
        match std::env::consts::OS {
            "linux" | "macos" => {
                // Check for common network interfaces on Unix-like systems
                if let Ok(output) = std::process::Command::new("ifconfig").output() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        if !line.starts_with(' ') && !line.starts_with('\t') && line.contains(':') {
                            if let Some(iface_name) = line.split(':').next() {
                                let name = iface_name.trim();
                                if !name.is_empty() && name != "lo" && name != "lo0" {
                                    interfaces.push(name.to_string());
                                }
                            }
                        }
                    }
                }
            }
            "windows" => {
                // Check network adapters on Windows
                if let Ok(output) = std::process::Command::new("ipconfig")
                    .args(["/all"])
                    .output()
                {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        if line.contains("adapter") && line.contains(':') {
                            interfaces.push("ethernet".to_string());
                            break;
                        }
                    }
                }
            }
            _ => {
                // Conservative default for other platforms
                interfaces.push("default".to_string());
            }
        }

        // Ensure we always have at least one interface
        if interfaces.len() == 1 {
            interfaces.push("default".to_string());
        }

        interfaces
    }
}

/// Create a path-based storage handler for platform detection
#[allow(dead_code)]
fn create_path_storage_adapter() -> FilesystemStorageHandler {
    FilesystemStorageHandler::with_default_path()
}

/// Path-based storage adapter for platform detection
///
/// Uses the proper FilesystemStorageHandler from aura-effects instead of
/// reimplementing. This maintains the architectural boundary and avoids
/// direct runtime/filesystem usage outside effects layer.
pub type PathStorageAdapter = FilesystemStorageHandler;

/// Platform information
#[derive(Debug, Clone)]
pub struct PlatformInfo {
    /// Operating system
    pub os: String,
    /// Architecture
    pub arch: String,
    /// Whether secure enclave is available
    pub has_secure_enclave: bool,
    /// Available storage backends
    pub available_storage_backends: Vec<String>,
    /// Available network interfaces
    pub available_network_interfaces: Vec<String>,
}

impl PlatformInfo {
    /// Check if a storage backend is available
    pub fn has_storage_backend(&self, backend: &str) -> bool {
        self.available_storage_backends
            .contains(&backend.to_string())
    }

    /// Check if a network interface is available
    pub fn has_network_interface(&self, interface: &str) -> bool {
        self.available_network_interfaces
            .contains(&interface.to_string())
    }

    /// Get the best storage backend from preferences
    pub fn best_storage_backend(&self, preferences: &[String]) -> Option<String> {
        for pref in preferences {
            if self.has_storage_backend(pref) {
                return Some(pref.clone());
            }
        }

        // Fallback to first available
        self.available_storage_backends.first().cloned()
    }
}
