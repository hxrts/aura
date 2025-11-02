//! Aura Authentication
//!
//! Layer 2 of the Aura security model: Identity verification and proof of who signed something.
//!
//! This crate handles proving WHO signed something (identity verification):
//! - "This signature proves DeviceId X signed this message"
//! - "This threshold signature proves M-of-N devices signed this"
//! - "This guardian signature proves GuardianId Y signed this"
//!
//! Authentication is stateless - it verifies signatures against public keys without
//! requiring knowledge of permissions or capabilities.

#![allow(missing_docs)]

pub mod device;
pub mod event_validation;
pub mod guardian;
pub mod session;
pub mod threshold;

// Integration tests
#[cfg(test)]
mod integration_tests;

// Re-export commonly used types
pub use device::verify_device_signature;
pub use guardian::verify_guardian_signature;
pub use session::verify_session_ticket;
pub use threshold::verify_threshold_signature;

// Re-export event validation functions
pub use event_validation::{
    validate_device_signature, validate_guardian_signature, validate_threshold_signature,
    EventValidator,
};

// EventAuthorization and ThresholdSig are defined in this module and exported by default

// Convenience functions
pub use device::verify_signature;

/// Authentication errors
#[derive(Debug, thiserror::Error)]
pub enum AuthenticationError {
    #[error("Invalid device signature: {0}")]
    InvalidDeviceSignature(String),

    #[error("Invalid threshold signature: {0}")]
    InvalidThresholdSignature(String),

    #[error("Invalid guardian signature: {0}")]
    InvalidGuardianSignature(String),

    #[error("Invalid session ticket: {0}")]
    InvalidSessionTicket(String),

    #[error("Crypto error: {0}")]
    CryptoError(String),
}

pub type Result<T> = std::result::Result<T, AuthenticationError>;

/// Authentication context for verifying signatures and credentials
///
/// This provides access to public keys, threshold configurations,
/// and other authentication data needed for verification.
#[derive(Debug, Clone)]
pub struct AuthenticationContext {
    /// Device public keys indexed by DeviceId
    device_keys: std::collections::HashMap<aura_types::DeviceId, aura_crypto::Ed25519VerifyingKey>,

    /// Guardian public keys indexed by GuardianId
    guardian_keys:
        std::collections::HashMap<aura_types::GuardianId, aura_crypto::Ed25519VerifyingKey>,

    /// Threshold configurations indexed by AccountId
    threshold_configs: std::collections::HashMap<aura_types::AccountId, ThresholdConfig>,

    /// Group public keys for threshold verification indexed by AccountId
    group_keys: std::collections::HashMap<aura_types::AccountId, aura_crypto::Ed25519VerifyingKey>,
}

/// Threshold signature configuration
#[derive(Debug, Clone)]
pub struct ThresholdConfig {
    /// Required number of signatures
    pub threshold: u16,

    /// Participating devices
    pub participants: Vec<aura_types::DeviceId>,
}

impl AuthenticationContext {
    /// Create a new authentication context
    pub fn new() -> Self {
        Self {
            device_keys: std::collections::HashMap::new(),
            guardian_keys: std::collections::HashMap::new(),
            threshold_configs: std::collections::HashMap::new(),
            group_keys: std::collections::HashMap::new(),
        }
    }

    /// Get the public key for a device
    pub fn get_device_public_key(
        &self,
        device_id: &aura_types::DeviceId,
    ) -> Result<&aura_crypto::Ed25519VerifyingKey> {
        self.device_keys.get(device_id).ok_or_else(|| {
            AuthenticationError::InvalidDeviceSignature(format!("Unknown device: {}", device_id))
        })
    }

    /// Add a device public key
    pub fn add_device_key(
        &mut self,
        device_id: aura_types::DeviceId,
        public_key: aura_crypto::Ed25519VerifyingKey,
    ) {
        self.device_keys.insert(device_id, public_key);
    }

    /// Get threshold configuration for an account
    pub fn get_threshold_config(
        &self,
        account_id: &aura_types::AccountId,
    ) -> Result<&ThresholdConfig> {
        self.threshold_configs.get(account_id).ok_or_else(|| {
            AuthenticationError::InvalidThresholdSignature(format!(
                "No threshold config for account: {}",
                account_id
            ))
        })
    }

    /// Add threshold configuration
    pub fn add_threshold_config(
        &mut self,
        account_id: aura_types::AccountId,
        config: ThresholdConfig,
    ) {
        self.threshold_configs.insert(account_id, config);
    }

    /// Get the guardian public key
    pub fn get_guardian_public_key(
        &self,
        guardian_id: &aura_types::GuardianId,
    ) -> Result<&aura_crypto::Ed25519VerifyingKey> {
        self.guardian_keys.get(guardian_id).ok_or_else(|| {
            AuthenticationError::InvalidGuardianSignature(format!(
                "Unknown guardian: {}",
                guardian_id
            ))
        })
    }

    /// Add a guardian public key
    pub fn add_guardian_key(
        &mut self,
        guardian_id: aura_types::GuardianId,
        public_key: aura_crypto::Ed25519VerifyingKey,
    ) {
        self.guardian_keys.insert(guardian_id, public_key);
    }

    /// Get the group public key for threshold verification
    pub fn get_group_public_key(
        &self,
        account_id: &aura_types::AccountId,
    ) -> Result<&aura_crypto::Ed25519VerifyingKey> {
        self.group_keys.get(account_id).ok_or_else(|| {
            AuthenticationError::InvalidThresholdSignature(format!(
                "No group key for account: {}",
                account_id
            ))
        })
    }

    /// Add a group public key for threshold verification
    pub fn add_group_key(
        &mut self,
        account_id: aura_types::AccountId,
        group_key: aura_crypto::Ed25519VerifyingKey,
    ) {
        self.group_keys.insert(account_id, group_key);
    }

    /// Verify device authentication
    pub fn verify_device_authentication(&self, device_id: &aura_types::DeviceId) -> Result<()> {
        // Simply check if we have a public key for this device
        self.get_device_public_key(device_id)?;
        Ok(())
    }

    /// Verify guardian authentication
    pub fn verify_guardian_authentication(
        &self,
        guardian_id: &aura_types::GuardianId,
    ) -> Result<()> {
        self.guardian_keys.get(guardian_id).ok_or_else(|| {
            AuthenticationError::InvalidGuardianSignature(format!(
                "Unknown guardian: {}",
                guardian_id
            ))
        })?;
        Ok(())
    }

    /// Verify session authentication
    pub fn verify_session_authentication(
        &self,
        _session_id: &uuid::Uuid,
        issuer: &aura_types::DeviceId,
    ) -> Result<()> {
        // Verify the issuer device is known
        self.verify_device_authentication(issuer)
    }

    /// Verify capability signature (simplified for now)
    pub fn verify_capability_signature(&self, _capability_data: &[u8]) -> Result<()> {
        // Simplified implementation - would verify the capability token signature
        Ok(())
    }
}

impl Default for AuthenticationContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Event authorization types that prove who authorized an event
/// This enum mirrors the one from journal/protocols/events.rs to maintain compatibility
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EventAuthorization {
    /// Threshold signature from M-of-N participants
    ThresholdSignature(ThresholdSig),
    /// Single device certificate signature
    DeviceCertificate {
        device_id: aura_types::DeviceId,
        #[serde(with = "aura_crypto::signature_serde")]
        signature: aura_crypto::Ed25519Signature,
    },
    /// Guardian signature (for recovery approvals)
    GuardianSignature {
        guardian_id: aura_types::GuardianId,
        #[serde(with = "aura_crypto::signature_serde")]
        signature: aura_crypto::Ed25519Signature,
    },
    /// Lifecycle-internal authorization used during protocol execution
    LifecycleInternal,
}

/// Threshold signature structure
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThresholdSig {
    /// The aggregated Ed25519 signature
    #[serde(with = "aura_crypto::signature_serde")]
    pub signature: aura_crypto::Ed25519Signature,
    /// Indices of devices that participated in signing
    pub signers: Vec<u8>,
    /// Individual signature shares (for auditing)
    pub signature_shares: Vec<Vec<u8>>,
}
