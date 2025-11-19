//! Agent Effects
//!
//! Effect traits for agent-level operations (authentication, sessions, configuration, etc.)

use aura_core::{AuraError, AuraResult, AuthorityId, DeviceId};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Agent health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHealthStatus {
    pub is_healthy: bool,
    pub services: HashMap<String, HealthStatus>,
}

/// Service health status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HealthStatus {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Authentication method
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthMethod {
    Biometric(BiometricType),
    Password,
    PublicKey,
}

/// Biometric type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BiometricType {
    Fingerprint,
    FaceId,
    TouchId,
}

/// Authentication result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthenticationResult {
    pub success: bool,
    pub authority_id: Option<AuthorityId>,
}

/// Session handle
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionHandle(pub uuid::Uuid);

/// Session info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub handle: SessionHandle,
    pub authority_id: AuthorityId,
    pub session_type: SessionType,
    pub status: SessionStatus,
}

/// Session type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionType {
    Interactive,
    Automated,
    Recovery,
}

/// Session status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Active,
    Suspended,
    Terminated,
}

/// Session role
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionRole {
    Initiator,
    Responder,
}

/// Session message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub session_id: SessionHandle,
    pub payload: Vec<u8>,
}

/// Device configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    pub device_id: DeviceId,
    pub device_name: String,
}

/// Device info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: DeviceId,
    pub device_name: String,
    pub os_version: String,
}

/// Credential backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialBackup {
    pub encrypted_data: Vec<u8>,
}

/// Configuration validation error
#[derive(Debug, Clone, thiserror::Error)]
#[error("Configuration validation error: {0}")]
pub struct ConfigValidationError(pub String);

/// Agent effects for high-level agent operations
#[async_trait]
pub trait AgentEffects: Send + Sync {
    /// Get agent health status
    async fn health_check(&self) -> AuraResult<AgentHealthStatus>;
}

/// Authentication effects
#[async_trait]
pub trait AuthenticationEffects: Send + Sync {
    /// Authenticate with the given method
    async fn authenticate(&self, method: AuthMethod) -> AuraResult<AuthenticationResult>;
}

/// Session management effects
#[async_trait]
pub trait SessionManagementEffects: Send + Sync {
    /// Create a new session
    async fn create_session(&self, authority_id: AuthorityId) -> AuraResult<SessionHandle>;

    /// Get session info
    async fn get_session(&self, handle: SessionHandle) -> AuraResult<SessionInfo>;

    /// Terminate a session
    async fn terminate_session(&self, handle: SessionHandle) -> AuraResult<()>;
}

/// Configuration effects
#[async_trait]
pub trait ConfigurationEffects: Send + Sync {
    /// Get device configuration
    async fn get_device_config(&self) -> AuraResult<DeviceConfig>;

    /// Update device configuration
    async fn update_device_config(&self, config: DeviceConfig) -> AuraResult<()>;
}

/// Device storage effects (platform-specific)
#[async_trait]
pub trait DeviceStorageEffects: Send + Sync {
    /// Store credential backup
    async fn store_backup(&self, backup: CredentialBackup) -> AuraResult<()>;

    /// Retrieve credential backup
    async fn retrieve_backup(&self) -> AuraResult<Option<CredentialBackup>>;
}
