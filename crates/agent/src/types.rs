// Core types for DeviceAgent

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;
// Removed legacy current_timestamp import - using effects instead

/// Context capsule for deterministic key derivation
///
/// Contains all context information needed for DKD operations including
/// application identity, context labeling, and policy constraints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextCapsule {
    /// Application identifier for context separation
    pub app_id: String,
    /// Human-readable context label for this derivation
    pub context_label: String,
    /// Optional policy constraint (content identifier)
    pub policy_hint: Option<String>, // CID
    /// Optional transport configuration hint
    pub transport_hint: Option<String>,
    /// Time-to-live in seconds (default 24h)
    pub ttl: Option<u64>,       // seconds (default 24h)
    /// Unix timestamp when capsule was issued
    pub issued_at: u64,         // unix seconds
}

impl ContextCapsule {
    /// Create a simple context capsule for common use case
    pub fn simple_with_effects(app_id: &str, context_label: &str, effects: &aura_crypto::Effects) -> crate::Result<Self> {
        Ok(ContextCapsule {
            app_id: app_id.to_string(),
            context_label: context_label.to_string(),
            policy_hint: None,
            transport_hint: None,
            ttl: Some(24 * 3600), // 24 hours
            issued_at: effects.now().map_err(|e| crate::AgentError::CryptoError(format!("Failed to get timestamp: {:?}", e)))?,
        })
    }
    
    /// Create a simple context capsule for testing (legacy compatibility)
    pub fn simple(app_id: &str, context_label: &str) -> Self {
        ContextCapsule {
            app_id: app_id.to_string(),
            context_label: context_label.to_string(),
            policy_hint: None,
            transport_hint: None,
            ttl: Some(24 * 3600), // 24 hours
            issued_at: 0, // Use epoch timestamp for testing
        }
    }
    
    /// Compute context ID (BLAKE3 hash of canonical CBOR)
    pub fn context_id(&self) -> crate::Result<[u8; 32]> {
        // Serialize to canonical CBOR (sorted keys, omit None fields)
        let cbor_bytes = serde_cbor::to_vec(self)
            .map_err(|e| crate::AgentError::SerializationError(format!(
                "ContextCapsule serialization failed: {}",
                e
            )))?;
        Ok(blake3::hash(&cbor_bytes).into())
    }
    
    /// Compute capsule MAC for tamper detection
    pub fn compute_mac(&self, seed_capsule: &[u8]) -> crate::Result<[u8; 32]> {
        let cbor_bytes = serde_cbor::to_vec(self)
            .map_err(|e| crate::AgentError::SerializationError(format!(
                "ContextCapsule serialization failed: {}",
                e
            )))?;
        let key: &[u8; 32] = seed_capsule.try_into()
            .map_err(|_| crate::AgentError::CryptoError(
                "seed_capsule must be 32 bytes".to_string()
            ))?;
        Ok(blake3::keyed_hash(key, &cbor_bytes).into())
    }
}

/// Derived identity from DKD
/// 
/// SECURITY: This type contains sensitive cryptographic material.
/// The seed_fingerprint is zeroized on drop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedIdentity {
    /// Original context capsule used for derivation
    pub capsule: ContextCapsule,
    /// Derived public key for this context
    #[serde(with = "verifying_key_serde")]
    pub pk_derived: VerifyingKey,
    /// Fingerprint of the seed used for derivation (for audit/debug)
    pub seed_fingerprint: [u8; 32], // For audit/debug
}

impl Drop for DerivedIdentity {
    fn drop(&mut self) {
        // Zeroize the seed fingerprint
        self.seed_fingerprint.zeroize();
    }
}

mod verifying_key_serde {
    use ed25519_dalek::VerifyingKey;
    use serde::{Deserialize, Deserializer, Serializer};
    
    pub fn serialize<S>(key: &VerifyingKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(key.as_bytes())
    }
    
    pub fn deserialize<'de, D>(deserializer: D) -> Result<VerifyingKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        VerifyingKey::from_bytes(bytes.as_slice().try_into().map_err(serde::de::Error::custom)?)
            .map_err(serde::de::Error::custom)
    }
}

/// Presence ticket issued for a derived identity
///
/// # Security Model (Enhanced)
///
/// Presence tickets now include:
/// - Challenge-response binding (prevents precomputation attacks)
/// - Operation-specific scoping (limits what the ticket can do)
/// - Rate limiting tracking (prevents ticket abuse)
/// - Device attestation placeholder (for TPM/SEP binding)
///
/// # Production Requirements
///
/// - Challenge MUST be server-generated random bytes
/// - Attestation MUST be bound to TPM/Secure Enclave quote
/// - Rate limits MUST be enforced server-side
/// - Credentials SHOULD be revocable via session epoch bump
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCredential {
    /// Device that issued this credential
    pub issued_by: aura_journal::DeviceId,
    /// Unix timestamp when credential expires
    pub expires_at: u64,
    /// Session epoch (credentials invalid if epoch is bumped)
    pub session_epoch: u64,
    /// Capability token (Biscuit or HPKE-wrapped secret)
    pub capability: Vec<u8>,
    /// Challenge provided by server/verifier (prevents precomputation)
    pub challenge: [u8; 32],
    /// Operation scope: what this credential authorizes (e.g., "read:messages", "write:profile")
    pub operation_scope: String,
    /// Nonce for replay prevention (monotonic counter per device)
    pub nonce: u64,
    /// Device attestation (placeholder for TPM/SEP quote)
    /// In production, this should be a platform-specific attestation token
    pub device_attestation: Option<Vec<u8>>,
}

/// Configuration for DeviceAgent
///
/// Core identity configuration including device credentials and threshold parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityConfig {
    /// Unique device identifier in the account
    pub device_id: aura_journal::DeviceId,
    /// Account identifier this device belongs to
    pub account_id: aura_journal::AccountId,
    /// Participant identifier for protocol coordination
    pub participant_id: aura_coordination::ParticipantId,
    /// Path to sealed key share (encrypted)
    pub share_path: String,
    /// Threshold configuration (minimum signatures required)
    pub threshold: u16,
    /// Total number of participants in threshold scheme
    pub total_participants: u16,
}

impl IdentityConfig {
    /// Load config from TOML file
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let config: IdentityConfig = toml::from_str(&contents)?;
        Ok(config)
    }
    
    /// Save config to TOML file
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let contents = toml::to_string_pretty(self)?;
        std::fs::write(path, contents)?;
        Ok(())
    }
}

/// Session statistics for monitoring and observability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatistics {
    /// Total number of sessions (all statuses)
    pub total_sessions: usize,
    /// Number of currently active sessions
    pub active_sessions: usize,
    /// Number of successfully completed sessions
    pub completed_sessions: usize,
    /// Number of failed/aborted sessions
    pub failed_sessions: usize,
    /// Number of timed-out sessions
    pub timed_out_sessions: usize,
    /// Session count by protocol type
    pub sessions_by_protocol: std::collections::BTreeMap<aura_journal::ProtocolType, usize>,
}

impl SessionStatistics {
    /// Create empty statistics
    pub fn new() -> Self {
        Self {
            total_sessions: 0,
            active_sessions: 0,
            completed_sessions: 0,
            failed_sessions: 0,
            timed_out_sessions: 0,
            sessions_by_protocol: std::collections::BTreeMap::new(),
        }
    }
    
    /// Calculate success rate (completed / total)
    pub fn success_rate(&self) -> f64 {
        if self.total_sessions == 0 {
            0.0
        } else {
            self.completed_sessions as f64 / self.total_sessions as f64
        }
    }
    
    /// Calculate failure rate (failed + timed_out) / total
    pub fn failure_rate(&self) -> f64 {
        if self.total_sessions == 0 {
            0.0
        } else {
            (self.failed_sessions + self.timed_out_sessions) as f64 / self.total_sessions as f64
        }
    }
}

impl Default for SessionStatistics {
    fn default() -> Self {
        Self::new()
    }
}


