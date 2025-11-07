//! Crypto-specific middleware system
//!
//! This module provides middleware for cryptographic operations including:
//! - Key derivation (DKD protocols)
//! - Threshold operations (FROST signatures)
//! - Secure random number generation
//! - Timing protection (constant-time operations)
//! - Key rotation (coordinated key updates)
//! - Hardware security (TEE/HSM integration)
//! - Audit logging (cryptographic operation tracking)

pub mod audit_logging;
pub mod handler;
pub mod hardware_security;
pub mod key_derivation;
pub mod key_rotation;
pub mod secure_random;
pub mod serde_utils;
pub mod stack;
pub mod timing_protection;

pub use audit_logging::*;
pub use handler::*;
pub use hardware_security::*;
pub use key_derivation::*;
pub use key_rotation::*;
pub use secure_random::*;
pub use serde_utils::*;
pub use stack::*;
pub use timing_protection::*;

use crate::Result;
use aura_types::{AccountId, DeviceId};

/// Context for crypto middleware operations
#[derive(Debug, Clone)]
pub struct CryptoContext {
    /// Account being operated on
    pub account_id: AccountId,

    /// Device performing the operation
    pub device_id: DeviceId,

    /// Operation being performed
    pub operation_type: String,

    /// Request timestamp
    pub timestamp: u64,

    /// Security level required for this operation
    pub security_level: SecurityLevel,

    /// Session context for operation
    pub session_context: String,

    /// Additional metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl CryptoContext {
    /// Create a new crypto context
    pub fn new(
        account_id: AccountId,
        device_id: DeviceId,
        operation_type: String,
        security_level: SecurityLevel,
    ) -> Self {
        Self {
            account_id,
            device_id,
            operation_type,
            #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in middleware context timestamp
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            security_level,
            session_context: "default".to_string(),
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add metadata to the context
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Set session context for the operation
    pub fn with_session_context(mut self, session_context: String) -> Self {
        self.session_context = session_context;
        self
    }
}

/// Security levels for cryptographic operations
#[derive(Debug, Clone, PartialEq, PartialOrd, serde::Serialize)]
pub enum SecurityLevel {
    /// Basic operations (low security)
    Basic,

    /// Standard operations (medium security)
    Standard,

    /// High-security operations (high security)
    High,

    /// Critical operations (maximum security)
    Critical,
}

/// Crypto operation types
#[derive(Debug, Clone)]
pub enum CryptoOperation {
    /// Derive a key using DKD
    DeriveKey {
        app_id: String,
        context: String,
        derivation_path: Vec<u32>,
    },

    /// Generate FROST signature
    GenerateSignature {
        message: Vec<u8>,
        signing_package: Vec<u8>,
    },

    /// Verify FROST signature
    VerifySignature {
        message: Vec<u8>,
        signature: Vec<u8>,
        public_key: Vec<u8>,
    },

    /// Generate secure random bytes
    GenerateRandom { num_bytes: usize },

    /// Rotate keys for an account
    RotateKeys {
        old_threshold: u32,
        new_threshold: u32,
        new_participants: Vec<DeviceId>,
    },

    /// Encrypt data
    Encrypt {
        plaintext: Vec<u8>,
        recipient_keys: Vec<Vec<u8>>,
    },

    /// Decrypt data
    Decrypt {
        ciphertext: Vec<u8>,
        private_key: Vec<u8>,
    },

    /// Hash data
    Hash { data: Vec<u8>, algorithm: String },
}

/// Trait for crypto middleware components
pub trait CryptoMiddleware: Send + Sync {
    /// Process a crypto operation
    fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
        next: &dyn CryptoHandler,
    ) -> Result<serde_json::Value>;

    /// Get middleware name for debugging
    fn name(&self) -> &str;
}

/// Trait for handling crypto operations
pub trait CryptoHandler: Send + Sync {
    /// Handle a crypto operation
    fn handle(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
    ) -> Result<serde_json::Value>;
}
