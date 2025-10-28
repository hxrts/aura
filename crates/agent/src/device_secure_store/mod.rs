//! Platform-specific secure storage implementations
//!
//! This module provides secure storage for cryptographic keys and sensitive data
//! across different platforms using their native secure storage systems.

// Re-export the main types and traits
pub use secure_storage::{
    AttestationStatement, DeviceAttestation, PlatformSecureStorage, SecureStorage, SecurityLevel,
};

// Platform-specific exports  
#[cfg(target_os = "android")]
pub use android::{AndroidKeystoreStorage, AndroidKeystoreFactory, RealAndroidKeystoreStorage};

// Main secure storage interface and platform factory
mod secure_storage;

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
#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android", target_os = "linux")))]
mod memory;