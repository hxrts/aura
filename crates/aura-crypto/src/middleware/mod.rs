//! Crypto-specific middleware system
//!
//! Provides essential crypto operations with clean effect-based architecture.

pub mod handler;
pub mod key_derivation;
pub mod secure_random;
pub mod serde_utils;

pub use handler::*;
pub use key_derivation::*;
pub use secure_random::*;
pub use serde_utils::*;

use crate::Result;
use aura_core::{AccountId, DeviceId};

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
        #[allow(clippy::disallowed_methods)] // [VERIFIED] Acceptable in middleware context
        #[allow(clippy::unwrap_used)] // [VERIFIED] SystemTime::now() should never fail
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            account_id,
            device_id,
            operation_type,
            timestamp,
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
        /// Application identifier for key derivation
        app_id: String,
        /// Derivation context
        context: String,
        /// Derivation path indices
        derivation_path: Vec<u32>,
    },

    /// Generate FROST signature
    GenerateSignature {
        /// Message to sign
        message: Vec<u8>,
        /// FROST signing package
        signing_package: Vec<u8>,
    },

    /// Verify FROST signature
    VerifySignature {
        /// Message that was signed
        message: Vec<u8>,
        /// Signature bytes
        signature: Vec<u8>,
        /// Public key for verification
        public_key: Vec<u8>,
    },

    /// Generate secure random bytes
    GenerateRandom {
        /// Number of random bytes to generate
        num_bytes: usize,
    },

    /// Rotate keys for an account
    RotateKeys {
        /// Current threshold before rotation
        old_threshold: u32,
        /// New threshold after rotation
        new_threshold: u32,
        /// Devices participating in the new key share
        new_participants: Vec<DeviceId>,
    },

    /// Encrypt data
    Encrypt {
        /// Data to encrypt
        plaintext: Vec<u8>,
        /// Recipient public keys
        recipient_keys: Vec<Vec<u8>>,
    },

    /// Decrypt data
    Decrypt {
        /// Data to decrypt
        ciphertext: Vec<u8>,
        /// Private key for decryption
        private_key: Vec<u8>,
    },

    /// Hash data
    Hash {
        /// Data to hash
        data: Vec<u8>,
        /// Hash algorithm (e.g., "blake3")
        algorithm: String,
    },
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
