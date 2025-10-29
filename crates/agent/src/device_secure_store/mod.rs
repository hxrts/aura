//! Platform-specific secure storage implementations
//!
//! This module provides secure storage for cryptographic keys and sensitive data
//! across different platforms using their native secure storage systems.

// Re-export the main types and traits
pub use store_interface::{
    AttestationStatement, DeviceAttestation, PlatformSecureStorage, SecureStorage, SecurityLevel,
};

// Re-export common implementation types
pub use common::{PlatformKeyStore, SecureStoreImpl};

// Platform-specific exports
#[cfg(target_os = "android")]
pub use android::{create_android_secure_storage, AndroidKeystore};

#[cfg(target_os = "macos")]
pub use macos::{
    create_macos_secure_storage, create_macos_secure_storage_with_service, MacOSKeychain,
};

#[cfg(target_os = "ios")]
pub use ios::{create_ios_secure_storage, IOSKeychain};

#[cfg(target_os = "linux")]
pub use linux::{create_linux_secure_storage, LinuxKeyring};

// Main secure storage interface and platform factory
mod store_interface;

// Common base implementation for reducing platform duplication
mod common;

// Platform-specific implementations
#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "ios")]
mod ios;

#[cfg(target_os = "android")]
mod android;

#[cfg(target_os = "linux")]
mod linux;

// Fallback in-memory storage for unsupported platforms
#[cfg(not(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "android",
    target_os = "linux"
)))]
mod memory;
