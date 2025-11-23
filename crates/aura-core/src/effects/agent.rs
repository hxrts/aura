//! Agent-Specific Effect Traits
//!
//! **Layer 1 (aura-core)**: Foundational effect trait definitions for agent operations.
//!
//! These effect traits define capabilities specific to device-side agent operations.
//! They compose core system effects into higher-level device workflows.
//!
//! This module was moved from aura-protocol/src/effects/agent.rs (Layer 4) because
//! these are foundational capability trait definitions, similar to CryptoEffects,
//! NetworkEffects, etc., and belong in the interface layer.

use crate::{
    identifiers::{AccountId, DeviceId, SessionId},
    AuraResult as Result,
};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Device information structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Device identifier
    pub device_id: DeviceId,
    /// Account this device belongs to
    pub account_id: Option<AccountId>,
    /// Human-readable device name
    pub device_name: String,
    /// Whether hardware security is available
    pub hardware_security: bool,
    /// Whether device attestation is available
    pub attestation_available: bool,
    /// Last sync timestamp
    pub last_sync: Option<u64>,
    /// Storage usage in bytes
    pub storage_usage: u64,
    /// Maximum storage in bytes
    pub storage_limit: u64,
}

/// High-level agent effects that compose core system capabilities
/// into device-specific workflows
#[async_trait]
pub trait AgentEffects: Send + Sync {
    /// Initialize the agent runtime
    async fn initialize(&self) -> Result<()>;

    /// Get comprehensive device information
    async fn get_device_info(&self) -> Result<DeviceInfo>;

    /// Perform agent shutdown procedures
    async fn shutdown(&self) -> Result<()>;

    /// Sync with distributed systems
    async fn sync_distributed_state(&self) -> Result<()>;

    /// Get agent health status
    async fn health_check(&self) -> Result<AgentHealthStatus>;
}

/// Agent health status information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHealthStatus {
    pub overall_status: HealthStatus,
    pub storage_status: HealthStatus,
    pub network_status: HealthStatus,
    pub authentication_status: HealthStatus,
    pub session_status: HealthStatus,
    pub last_check: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded { reason: String },
    Unhealthy { error: String },
}

/// Device-specific secure storage effects that enhance core storage
/// with biometric protection and device-specific security features
#[async_trait]
pub trait DeviceStorageEffects: Send + Sync {
    /// Store credential with biometric protection
    async fn store_credential(&self, key: &str, credential: &[u8]) -> Result<()>;

    /// Retrieve credential with biometric authentication
    async fn retrieve_credential(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// Delete credential securely
    async fn delete_credential(&self, key: &str) -> Result<()>;

    /// List all stored credential keys (metadata only)
    async fn list_credentials(&self) -> Result<Vec<String>>;

    /// Store device-specific configuration
    async fn store_device_config(&self, config: &[u8]) -> Result<()>;

    /// Retrieve device configuration
    async fn retrieve_device_config(&self) -> Result<Option<Vec<u8>>>;

    /// Backup credentials to secure backup location
    async fn backup_credentials(&self) -> Result<CredentialBackup>;

    /// Restore credentials from backup with verification
    async fn restore_credentials(&self, backup: &CredentialBackup) -> Result<()>;

    /// Securely wipe all stored credentials
    async fn secure_wipe(&self) -> Result<()>;
}

/// Credential backup structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialBackup {
    pub device_id: DeviceId,
    pub timestamp: u64,
    pub encrypted_credentials: Vec<u8>,
    pub backup_hash: [u8; 32],
    pub metadata: HashMap<String, String>,
}

/// Authentication effects for device unlock and biometric operations
#[async_trait]
pub trait AuthenticationEffects: Send + Sync {
    /// Authenticate device using available methods (biometric, PIN, etc.)
    async fn authenticate_device(&self) -> Result<AuthenticationResult>;

    /// Check if device is currently authenticated
    async fn is_authenticated(&self) -> Result<bool>;

    /// Lock the device (clear authentication state)
    async fn lock_device(&self) -> Result<()>;

    /// Get available authentication methods
    async fn get_auth_methods(&self) -> Result<Vec<AuthMethod>>;

    /// Enroll new biometric data
    async fn enroll_biometric(&self, biometric_type: BiometricType) -> Result<()>;

    /// Remove enrolled biometric data
    async fn remove_biometric(&self, biometric_type: BiometricType) -> Result<()>;

    /// Verify capability token for operations
    async fn verify_capability(&self, capability: &[u8]) -> Result<bool>;

    /// Generate device attestation
    async fn generate_attestation(&self) -> Result<Vec<u8>>;
}

/// Authentication result from device unlock
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationResult {
    pub success: bool,
    pub method_used: Option<AuthMethod>,
    pub session_token: Option<Vec<u8>>,
    pub expires_at: Option<u64>,
    pub error: Option<String>,
}

/// Available authentication methods
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthMethod {
    Biometric(BiometricType),
    Pin,
    Password,
    HardwareKey,
    DeviceCredential,
}

/// Supported biometric types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BiometricType {
    Fingerprint,
    FaceId,
    TouchId,
    VoiceId,
}

/// Session management effects for device-side session coordination
///
/// Enhanced to support choreographic patterns and consistent protocol implementation.
#[async_trait]
pub trait SessionManagementEffects: Send + Sync {
    /// Create new device session for distributed protocols
    async fn create_session(&self, session_type: SessionType) -> Result<SessionId>;

    /// Create choreographic session with participants and roles
    async fn create_choreographic_session(
        &self,
        session_type: SessionType,
        participants: Vec<DeviceId>,
        choreography_config: ChoreographyConfig,
    ) -> Result<SessionId>;

    /// Join existing session as participant
    async fn join_session(&self, session_id: SessionId) -> Result<SessionHandle>;

    /// Join choreographic session with specific role
    async fn join_choreographic_session(
        &self,
        session_id: SessionId,
        role: ChoreographicRole,
    ) -> Result<SessionHandle>;

    /// Leave session gracefully
    async fn leave_session(&self, session_id: SessionId) -> Result<()>;

    /// End session (if session owner)
    async fn end_session(&self, session_id: SessionId) -> Result<()>;

    /// List active sessions for this device
    async fn list_active_sessions(&self) -> Result<Vec<SessionInfo>>;

    /// Get session status and metadata
    async fn get_session_status(&self, session_id: SessionId) -> Result<SessionStatus>;

    /// Send choreographic message within session context
    async fn send_choreographic_message(
        &self,
        session_id: SessionId,
        message_type: &str,
        payload: &[u8],
        target_role: Option<ChoreographicRole>,
    ) -> Result<()>;

    /// Send message within session context (legacy compatibility)
    async fn send_session_message(&self, session_id: SessionId, message: &[u8]) -> Result<()>;

    /// Receive choreographic messages for session with role filtering
    async fn receive_choreographic_messages(
        &self,
        session_id: SessionId,
        role_filter: Option<ChoreographicRole>,
    ) -> Result<Vec<ChoreographicMessage>>;

    /// Receive messages for session (legacy compatibility)
    async fn receive_session_messages(&self, session_id: SessionId) -> Result<Vec<SessionMessage>>;

    /// Get choreography phase for session
    async fn get_choreography_phase(&self, session_id: SessionId) -> Result<Option<String>>;

    /// Update choreography state
    async fn update_choreography_state(
        &self,
        session_id: SessionId,
        phase: &str,
        state_data: &[u8],
    ) -> Result<()>;

    /// Validate choreography message against current phase
    async fn validate_choreographic_message(
        &self,
        session_id: SessionId,
        message: &ChoreographicMessage,
    ) -> Result<bool>;
}

/// Types of sessions the agent can participate in
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionType {
    Recovery,
    KeyRotation,
    ThresholdOperation,
    Coordination,
    Backup,
    Custom(String),
}

/// Session handle for ongoing operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHandle {
    pub session_id: SessionId,
    pub role: SessionRole,
    pub participants: Vec<DeviceId>,
    pub created_at: u64,
}

/// Role in a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionRole {
    Coordinator,
    Initiator,
    Participant,
    Approver,
    Observer,
}

/// Session information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: SessionId,
    pub session_type: SessionType,
    pub role: SessionRole,
    pub participants: Vec<DeviceId>,
    pub status: SessionStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub timeout_at: Option<u64>,
    pub operation: Option<String>,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Session status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Created,
    Active,
    Paused,
    WaitingForApprovals,
    Completed,
    Failed { error: String },
    Expired,
    Cancelled,
    TimedOut,
}

/// Message within a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub from: DeviceId,
    pub to: Option<DeviceId>, // None for broadcast
    pub timestamp: u64,
    pub message_type: String,
    pub payload: Vec<u8>,
}

/// Choreographic role in a session
#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct ChoreographicRole {
    pub device_id: uuid::Uuid,
    pub role_index: u32,
}

impl ChoreographicRole {
    pub fn new(device_id: uuid::Uuid, role_index: u32) -> Self {
        Self {
            device_id,
            role_index,
        }
    }
}

/// Configuration for choreographic sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoreographyConfig {
    /// Protocol namespace
    pub namespace: String,
    /// Guard capabilities required
    pub guard_capabilities: Vec<String>,
    /// Flow budget limits
    pub flow_budget: Option<u64>,
    /// Journal facts to record
    pub journal_facts: Vec<String>,
    /// Timeout for the choreography in seconds
    pub timeout_seconds: u64,
    /// Maximum number of retries
    pub max_retries: u32,
}

/// Choreographic message with role and phase information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChoreographicMessage {
    pub from: DeviceId,
    pub to: Option<DeviceId>,
    pub source_role: ChoreographicRole,
    pub target_role: Option<ChoreographicRole>,
    pub protocol_namespace: String,
    pub phase: String,
    pub message_type: String,
    pub payload: Vec<u8>,
    pub timestamp: u64,
    pub sequence_number: u64,
    pub guard_capabilities: Vec<String>,
}

/// Configuration management effects for device settings
#[async_trait]
pub trait ConfigurationEffects: Send + Sync {
    /// Get device configuration
    async fn get_device_config(&self) -> Result<DeviceConfig>;

    /// Update device configuration
    async fn update_device_config(&self, config: &DeviceConfig) -> Result<()>;

    /// Reset configuration to defaults
    async fn reset_to_defaults(&self) -> Result<()>;

    /// Export configuration for backup
    async fn export_config(&self) -> Result<Vec<u8>>;

    /// Import configuration from backup
    async fn import_config(&self, config_data: &[u8]) -> Result<()>;

    /// Validate configuration settings
    async fn validate_config(&self, config: &DeviceConfig) -> Result<Vec<ConfigValidationError>>;

    /// Get configuration value as JSON
    async fn get_config_json(&self, key: &str) -> Result<Option<serde_json::Value>>;

    /// Set configuration value as JSON
    async fn set_config_json(&self, key: &str, value: &serde_json::Value) -> Result<()>;

    /// Get all configuration as key-value pairs
    async fn get_all_config(&self) -> Result<std::collections::HashMap<String, serde_json::Value>>;
}

/// Device configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub device_name: String,
    pub auto_lock_timeout: u32, // seconds
    pub biometric_enabled: bool,
    pub backup_enabled: bool,
    pub sync_interval: u32,    // seconds
    pub max_storage_size: u64, // bytes
    pub network_timeout: u32,  // milliseconds
    pub log_level: String,
    pub custom_settings: HashMap<String, serde_json::Value>,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            device_name: "Aura Device".to_string(),
            auto_lock_timeout: 300, // 5 minutes
            biometric_enabled: false,
            backup_enabled: true,
            sync_interval: 3600,                 // 1 hour
            max_storage_size: 100 * 1024 * 1024, // 100 MB
            network_timeout: 5000,               // 5 seconds
            log_level: "info".to_string(),
            custom_settings: HashMap::new(),
        }
    }
}

/// Configuration validation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigValidationError {
    pub field: String,
    pub error: String,
    pub suggested_value: Option<serde_json::Value>,
}

/// Configuration operation error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfigError {
    /// Configuration key not found
    NotFound(String),
    /// Invalid JSON value
    InvalidJson(String),
    /// Serialization/deserialization error
    SerializationError(String),
    /// Storage error
    StorageError(String),
    /// Permission denied
    PermissionDenied(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::NotFound(key) => write!(f, "Configuration key not found: {}", key),
            ConfigError::InvalidJson(msg) => write!(f, "Invalid JSON value: {}", msg),
            ConfigError::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
            ConfigError::StorageError(msg) => write!(f, "Storage error: {}", msg),
            ConfigError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
        }
    }
}

impl std::error::Error for ConfigError {}
