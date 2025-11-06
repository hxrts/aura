//! Agent-specific effects for orchestrating device and network operations
//!
//! This module defines effects that are specific to the agent layer,
//! focusing on orchestration between local device storage and the distributed journal.
//!
//! These effect traits are the interface layer between the agent business logic
//! and the effect system. Implementations can be swapped for testing and simulation.

use aura_types::{AccountId, DeviceId};
// TODO: CapabilityToken needs to be defined in aura-types
type CapabilityToken = String; // Placeholder
use super::choreographic::ChoreographicRole;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Device storage operations effect
///
/// This effect abstracts access to the local device's secure storage,
/// treating it as an I/O boundary that can be mocked for testing.
#[async_trait]
pub trait DeviceStorageEffects: Send + Sync {
    /// Store data securely on the local device
    async fn store_secure(&self, key: &str, value: &[u8]) -> Result<(), DeviceStorageError>;

    /// Retrieve data from the local device's secure storage
    async fn retrieve_secure(&self, key: &str) -> Result<Option<Vec<u8>>, DeviceStorageError>;

    /// Delete data from the local device's secure storage
    async fn delete_secure(&self, key: &str) -> Result<(), DeviceStorageError>;

    /// List all keys in the secure storage
    async fn list_keys(&self) -> Result<Vec<String>, DeviceStorageError>;

    /// Check if device supports hardware security
    async fn has_hardware_security(&self) -> bool;

    /// Get device attestation if available
    async fn get_device_attestation(&self)
        -> Result<Option<DeviceAttestation>, DeviceStorageError>;
}

/// Session management effects for the agent
///
/// Handles agent-specific session lifecycle, different from protocol sessions.
#[async_trait]
pub trait SessionManagementEffects: Send + Sync {
    /// Create a new agent session
    async fn create_session(&self, session_data: SessionData) -> Result<String, SessionError>;

    /// Get session information
    async fn get_session(&self, session_id: &str) -> Result<Option<SessionData>, SessionError>;

    /// Update session state
    async fn update_session(
        &self,
        session_id: &str,
        update: SessionUpdate,
    ) -> Result<(), SessionError>;

    /// End a session
    async fn end_session(&self, session_id: &str) -> Result<SessionData, SessionError>;

    /// List active sessions
    async fn list_active_sessions(&self) -> Result<Vec<String>, SessionError>;

    /// Cleanup expired sessions
    async fn cleanup_expired_sessions(
        &self,
        max_age_seconds: u64,
    ) -> Result<Vec<String>, SessionError>;
}

/// Authentication and authorization effects
#[async_trait]
pub trait AuthenticationEffects: Send + Sync {
    /// Verify a capability token
    async fn verify_capability(&self, token: &CapabilityToken) -> Result<bool, AuthError>;

    /// Check if device is authorized for operation
    async fn is_authorized(&self, device_id: DeviceId, operation: &str) -> Result<bool, AuthError>;

    /// Get device identity
    async fn get_device_identity(&self) -> Result<DeviceId, AuthError>;

    /// Create a capability token
    async fn create_capability(
        &self,
        permissions: Vec<String>,
    ) -> Result<CapabilityToken, AuthError>;
}

/// Configuration management effects
#[async_trait]
pub trait ConfigurationEffects: Send + Sync {
    /// Get configuration value as JSON
    async fn get_config_json(&self, key: &str) -> Result<Option<serde_json::Value>, ConfigError>;

    /// Set configuration value from JSON
    async fn set_config_json(&self, key: &str, value: serde_json::Value)
        -> Result<(), ConfigError>;

    /// Get all configuration
    async fn get_all_config(&self) -> Result<HashMap<String, serde_json::Value>, ConfigError>;
}

/// Combined agent effects interface
///
/// This trait combines all agent-specific effects needed for orchestration.
pub trait AgentEffects:
    DeviceStorageEffects
    + SessionManagementEffects
    + AuthenticationEffects
    + ConfigurationEffects
    + Send
    + Sync
{
    /// Get the device ID for this agent
    fn device_id(&self) -> DeviceId;

    /// Check if running in simulation mode
    fn is_simulation(&self) -> bool;

    /// Generate threshold key shares (synchronous to maintain trait object compatibility)
    fn generate_key_shares(
        &self,
        threshold: usize,
        total: usize,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        // Simple synchronous implementation - actual implementation would use secure randomness
        let shares = (0..total)
            .map(|i| format!("share_{}_{}", threshold, i))
            .collect();
        Ok(shares)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Error Types
// ═══════════════════════════════════════════════════════════════════════════

/// Device storage operation errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum DeviceStorageError {
    /// Storage not available
    #[error("Storage not available")]
    NotAvailable,
    /// Key not found
    #[error("Key not found")]
    NotFound,
    /// Permission denied
    #[error("Permission denied")]
    PermissionDenied,
    /// Access denied
    #[error("Access denied")]
    AccessDenied,
    /// Storage quota exceeded
    #[error("Storage quota exceeded")]
    QuotaExceeded,
    /// Storage is full
    #[error("Storage is full")]
    StorageFull,
    /// Operation failed
    #[error("Operation failed: {0}")]
    OperationFailed(String),
    /// Encryption/decryption error
    #[error("Crypto error: {0}")]
    CryptoError(String),
    /// Other storage errors
    #[error("Storage error: {0}")]
    Other(String),
}

/// Session management errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum SessionError {
    /// Session not found
    #[error("Session not found")]
    NotFound,
    /// Session expired
    #[error("Session expired")]
    Expired,
    /// Invalid session state
    #[error("Invalid session state: {0}")]
    InvalidState(String),
    /// Permission denied
    #[error("Permission denied")]
    PermissionDenied,
    /// Session already exists
    #[error("Session already exists: {0}")]
    SessionAlreadyExists(String),
    /// Session not found by ID
    #[error("Session not found: {0}")]
    SessionNotFound(String),
    /// Storage operation failed
    #[error("Storage failed: {0}")]
    StorageFailed(String),
    /// Serialization failed
    #[error("Serialization failed: {0}")]
    SerializationFailed(String),
    /// Other session errors
    #[error("Session error: {0}")]
    Other(String),
}

/// Authentication errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum AuthError {
    /// Authentication failed
    #[error("Authentication failed")]
    AuthenticationFailed,
    /// Authorization failed
    #[error("Authorization failed")]
    AuthorizationFailed,
    /// Authorization denied
    #[error("Authorization denied")]
    AuthorizationDenied,
    /// Invalid credentials
    #[error("Invalid credentials")]
    InvalidCredentials,
    /// Device not registered
    #[error("Device not registered")]
    DeviceNotRegistered,
    /// Permission denied
    #[error("Permission denied")]
    PermissionDenied,
    /// Other auth errors
    #[error("Auth error: {0}")]
    Other(String),
}

/// Configuration errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    /// Configuration not found
    #[error("Configuration not found")]
    NotFound,
    /// Invalid configuration value
    #[error("Invalid value: {0}")]
    InvalidValue(String),
    /// Permission denied
    #[error("Permission denied")]
    PermissionDenied,
    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),
    /// Key not found
    #[error("Key not found: {0}")]
    KeyNotFound(String),
    /// Other config errors
    #[error("Config error: {0}")]
    Other(String),
}

// ═══════════════════════════════════════════════════════════════════════════
// Data Types
// ═══════════════════════════════════════════════════════════════════════════

/// Session data for agent-level choreographic sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    /// Unique session identifier
    pub session_id: String,
    /// Account this session belongs to
    pub account_id: AccountId,
    /// Device that owns this session
    pub device_id: DeviceId,
    /// Session epoch for ordering
    pub epoch: u64,
    /// When the session started (Unix timestamp)
    pub start_time: u64,
    /// All participants in the choreography
    pub participants: Vec<ChoreographicRole>,
    /// This device's role in the choreography
    pub my_role: ChoreographicRole,
    /// Type of session/protocol being run
    pub session_type: SessionType,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Session update information
#[derive(Debug, Clone)]
pub enum SessionUpdate {
    /// Update metadata
    UpdateMetadata(HashMap<String, serde_json::Value>),
    /// Add participant to choreography
    AddParticipant(ChoreographicRole),
    /// Remove participant from choreography
    RemoveParticipant(DeviceId),
    /// Change session epoch
    ChangeEpoch(u64),
}

/// Device attestation information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceAttestation {
    /// Device ID
    pub device_id: DeviceId,
    /// Attestation data
    pub attestation_data: Vec<u8>,
    /// Signature over attestation data
    pub signature: Vec<u8>,
    /// Certificate chain
    pub certificate_chain: Vec<Vec<u8>>,
}

// Note: ChoreographicRole is re-exported from choreographic module to avoid duplication

/// Type of distributed session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionType {
    /// Threshold cryptographic operation
    ThresholdOperation,
    /// Distributed key derivation
    KeyDerivation,
    /// Account recovery session
    Recovery,
    /// Key rotation session
    KeyRotation,
    /// Generic coordination session
    Coordination,
}