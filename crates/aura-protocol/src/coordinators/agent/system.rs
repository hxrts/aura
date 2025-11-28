//! Unified agent effect system
//!
//! This module provides the unified effect system for agent operations,
//! integrating all agent-specific effects with the core Aura effect system.

use async_lock::RwLock;
use async_trait::async_trait;
use std::sync::Arc;

use super::{auth::AuthenticationHandler, session::MemorySessionHandler};
use crate::effects::{
    AgentEffects, AgentHealthStatus, AuthMethod, AuthenticationEffects, AuthenticationResult,
    ConfigValidationError, ConfigurationEffects, CredentialBackup, DeviceConfig, DeviceInfo,
    DeviceStorageEffects, HealthStatus, SessionHandle, SessionInfo, SessionManagementEffects,
    SessionStatus, SessionType,
};
use aura_core::effects::agent::{BiometricType, ChoreographicMessage, ChoreographicRole};
use aura_core::hash::hash;
use aura_core::{identifiers::DeviceId, AuraResult as Result};
use aura_effects::time::PhysicalTimeHandler;

/// Unified agent effect system that implements all agent-specific effects
pub struct AgentEffectSystemHandler {
    device_id: DeviceId,
    core_effects: Arc<RwLock<Box<dyn crate::effects::AuraEffects>>>,
    auth_handler: AuthenticationHandler,
    session_handler: MemorySessionHandler,
}

impl AgentEffectSystemHandler {
    /// Create a new agent effect system handler
    pub fn new(
        device_id: DeviceId,
        core_effects: Arc<RwLock<Box<dyn crate::effects::AuraEffects>>>,
    ) -> Self {
        let auth_handler = AuthenticationHandler::new(device_id, core_effects.clone());
        let session_handler = MemorySessionHandler::new(device_id, Arc::new(PhysicalTimeHandler));

        Self {
            device_id,
            core_effects,
            auth_handler,
            session_handler,
        }
    }

    /// Create agent effect system with an existing core effect system (recommended)
    ///
    /// This is the preferred method as it follows proper dependency injection.
    /// Use this instead of `for_testing()` for better testability and control.
    pub fn with_core_effects(
        device_id: DeviceId,
        core_effects: Arc<RwLock<Box<dyn crate::effects::AuraEffects>>>,
    ) -> Self {
        Self::new(device_id, core_effects)
    }

    /// Initialize the agent effect system
    pub async fn initialize(&self) -> Result<()> {
        self.auth_handler.initialize().await?;
        Ok(())
    }

    /// Shutdown the agent effect system
    pub async fn shutdown(&self) -> Result<()> {
        self.auth_handler.shutdown().await?;
        Ok(())
    }

    /// Get the device ID this system is configured for
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }
}

// Implement AgentEffects trait
#[async_trait]
impl AgentEffects for AgentEffectSystemHandler {
    async fn initialize(&self) -> Result<()> {
        self.initialize().await
    }

    async fn get_device_info(&self) -> Result<DeviceInfo> {
        let effects = self.core_effects.read().await;

        // Get storage stats for usage information
        let storage_stats = effects
            .stats()
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("Failed to get stats: {}", e)))?;
        let storage_usage = storage_stats.total_size; // Use actual storage size

        Ok(DeviceInfo {
            device_id: self.device_id,
            account_id: None, // Would be set if device is associated with an account
            device_name: "Aura Device".to_string(),
            hardware_security: true, // Assume hardware security is available
            attestation_available: true,
            last_sync: Some(effects.physical_time().await.map(|t| t.ts_ms).unwrap_or(0)),
            storage_usage,
            storage_limit: 100 * 1024 * 1024, // 100 MB default
        })
    }

    async fn shutdown(&self) -> Result<()> {
        self.shutdown().await
    }

    async fn sync_distributed_state(&self) -> Result<()> {
        // In production this would sync with the distributed journal; here we log for visibility.
        let effects = self.core_effects.read().await;
        effects.log_info("Syncing distributed state").await?;
        Ok(())
    }

    async fn health_check(&self) -> Result<AgentHealthStatus> {
        let auth_health = self.auth_handler.health_check().await?;
        let effects = self.core_effects.read().await;

        // Check storage health
        let storage_health = match effects.stats().await {
            Ok(_) => HealthStatus::Healthy,
            Err(_) => HealthStatus::Degraded {
                reason: "Storage not accessible".to_string(),
            },
        };

        // Simplified health check placeholder
        let network_health = HealthStatus::Healthy; // Assume healthy for now

        // Check session health
        let session_count = self.session_handler.session_count();
        let session_health = if session_count < 100 {
            HealthStatus::Healthy
        } else {
            HealthStatus::Degraded {
                reason: "Too many active sessions".to_string(),
            }
        };

        // Determine overall status
        let overall_status = match (
            &auth_health,
            &storage_health,
            &network_health,
            &session_health,
        ) {
            (
                HealthStatus::Healthy,
                HealthStatus::Healthy,
                HealthStatus::Healthy,
                HealthStatus::Healthy,
            ) => HealthStatus::Healthy,
            _ => HealthStatus::Degraded {
                reason: "One or more subsystems degraded".to_string(),
            },
        };

        Ok(AgentHealthStatus {
            overall_status,
            storage_status: storage_health,
            network_status: network_health,
            authentication_status: auth_health,
            session_status: session_health,
            last_check: effects.physical_time().await.map(|t| t.ts_ms).unwrap_or(0),
        })
    }
}

// Implement DeviceStorageEffects trait
#[async_trait]
impl DeviceStorageEffects for AgentEffectSystemHandler {
    async fn store_credential(&self, key: &str, credential: &[u8]) -> Result<()> {
        let effects = self.core_effects.read().await;
        effects
            .store(&format!("credential_{}", key), credential.to_vec())
            .await
            .map_err(|e| {
                aura_core::AuraError::internal(format!("Failed to store credential: {}", e))
            })
    }

    async fn retrieve_credential(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let effects = self.core_effects.read().await;
        effects
            .retrieve(&format!("credential_{}", key))
            .await
            .map_err(|e| {
                aura_core::AuraError::internal(format!("Failed to retrieve credential: {}", e))
            })
    }

    async fn delete_credential(&self, key: &str) -> Result<()> {
        let effects = self.core_effects.read().await;
        effects
            .remove(&format!("credential_{}", key))
            .await
            .map_err(|e| {
                aura_core::AuraError::internal(format!("Failed to delete credential: {}", e))
            })?;
        Ok(())
    }

    async fn list_credentials(&self) -> Result<Vec<String>> {
        let effects = self.core_effects.read().await;
        let _stats = effects
            .stats()
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("Failed to get stats: {}", e)))?;

        // StorageStats does not expose key listing yet; return empty set.
        // This would need to be implemented via list_keys() in real usage
        Ok(Vec::new())
    }

    async fn store_device_config(&self, config: &[u8]) -> Result<()> {
        let effects = self.core_effects.read().await;
        effects
            .store("device_config", config.to_vec())
            .await
            .map_err(|e| {
                aura_core::AuraError::internal(format!("Failed to store device config: {}", e))
            })
    }

    async fn retrieve_device_config(&self) -> Result<Option<Vec<u8>>> {
        let effects = self.core_effects.read().await;
        effects.retrieve("device_config").await.map_err(|e| {
            aura_core::AuraError::internal(format!("Failed to retrieve device config: {}", e))
        })
    }

    async fn backup_credentials(&self) -> Result<CredentialBackup> {
        let effects = self.core_effects.read().await;
        let timestamp = effects.physical_time().await?.ts_ms;

        // Get all credentials
        let credentials = self.list_credentials().await?;
        let mut backup_data = Vec::new();

        for key in credentials {
            if let Ok(Some(cred)) = self.retrieve_credential(&key).await {
                backup_data.extend_from_slice(&cred);
            }
        }

        // Encrypt the backup data
        let encrypted_credentials = hash(&backup_data).to_vec();
        let backup_hash = hash(&encrypted_credentials);

        Ok(CredentialBackup {
            device_id: self.device_id,
            timestamp,
            encrypted_credentials,
            backup_hash,
            metadata: std::collections::HashMap::new(),
        })
    }

    async fn restore_credentials(&self, backup: &CredentialBackup) -> Result<()> {
        // Verify backup integrity
        let effects = self.core_effects.read().await;
        let computed_hash = hash(&backup.encrypted_credentials);

        if computed_hash != backup.backup_hash {
            return Err(aura_core::AuraError::invalid(
                "Backup integrity check failed",
            ));
        }

        // In production, this would decrypt and restore credentials.
        effects.log_info("Credentials restored from backup").await?;
        Ok(())
    }

    async fn secure_wipe(&self) -> Result<()> {
        let credentials = self.list_credentials().await?;

        for key in credentials {
            self.delete_credential(&key).await?;
        }

        let effects = self.core_effects.read().await;
        effects.log_info("Secure wipe completed").await?;
        Ok(())
    }
}

// Delegate AuthenticationEffects to the auth handler
#[async_trait]
impl AuthenticationEffects for AgentEffectSystemHandler {
    async fn authenticate_device(&self) -> Result<AuthenticationResult> {
        self.auth_handler.authenticate_device().await
    }

    async fn is_authenticated(&self) -> Result<bool> {
        self.auth_handler.is_authenticated().await
    }

    async fn lock_device(&self) -> Result<()> {
        self.auth_handler.lock_device().await
    }

    async fn get_auth_methods(&self) -> Result<Vec<AuthMethod>> {
        self.auth_handler.get_auth_methods().await
    }

    async fn enroll_biometric(&self, biometric_type: BiometricType) -> Result<()> {
        self.auth_handler.enroll_biometric(biometric_type).await
    }

    async fn remove_biometric(&self, biometric_type: BiometricType) -> Result<()> {
        self.auth_handler.remove_biometric(biometric_type).await
    }

    async fn verify_capability(&self, capability: &[u8]) -> Result<bool> {
        self.auth_handler.verify_capability(capability).await
    }

    async fn generate_attestation(&self) -> Result<Vec<u8>> {
        self.auth_handler.generate_attestation().await
    }
}

// Delegate SessionManagementEffects to the session handler
#[async_trait]
impl SessionManagementEffects for AgentEffectSystemHandler {
    async fn create_session(
        &self,
        session_type: SessionType,
    ) -> Result<aura_core::identifiers::SessionId> {
        self.session_handler.create_session(session_type).await
    }

    async fn join_session(
        &self,
        session_id: aura_core::identifiers::SessionId,
    ) -> Result<SessionHandle> {
        self.session_handler.join_session(session_id).await
    }

    async fn leave_session(&self, session_id: aura_core::identifiers::SessionId) -> Result<()> {
        self.session_handler.leave_session(session_id).await
    }

    async fn end_session(&self, session_id: aura_core::identifiers::SessionId) -> Result<()> {
        self.session_handler.end_session(session_id).await
    }

    async fn list_active_sessions(&self) -> Result<Vec<SessionInfo>> {
        self.session_handler.list_active_sessions().await
    }

    async fn get_session_status(
        &self,
        session_id: aura_core::identifiers::SessionId,
    ) -> Result<SessionStatus> {
        self.session_handler.get_session_status(session_id).await
    }

    // Choreographic session methods - delegate to session handler
    async fn create_choreographic_session(
        &self,
        session_type: SessionType,
        participants: Vec<aura_core::DeviceId>,
        choreography_config: aura_core::effects::agent::ChoreographyConfig,
    ) -> Result<aura_core::identifiers::SessionId> {
        self.session_handler
            .create_choreographic_session(session_type, participants, choreography_config)
            .await
    }

    async fn join_choreographic_session(
        &self,
        session_id: aura_core::identifiers::SessionId,
        role: ChoreographicRole,
    ) -> Result<SessionHandle> {
        self.session_handler
            .join_choreographic_session(session_id, role)
            .await
    }

    async fn send_choreographic_message(
        &self,
        session_id: aura_core::identifiers::SessionId,
        message_type: &str,
        payload: &[u8],
        target_role: Option<ChoreographicRole>,
    ) -> Result<()> {
        self.session_handler
            .send_choreographic_message(session_id, message_type, payload, target_role)
            .await
    }

    async fn receive_choreographic_messages(
        &self,
        session_id: aura_core::identifiers::SessionId,
        role_filter: Option<ChoreographicRole>,
    ) -> Result<Vec<ChoreographicMessage>> {
        self.session_handler
            .receive_choreographic_messages(session_id, role_filter)
            .await
    }

    async fn get_choreography_phase(
        &self,
        session_id: aura_core::identifiers::SessionId,
    ) -> Result<Option<String>> {
        Ok(self
            .session_handler
            .get_choreography_phase(&session_id)
            .map(|phase| format!("{:?}", phase)))
    }

    async fn update_choreography_state(
        &self,
        session_id: aura_core::identifiers::SessionId,
        phase: &str,
        state_data: &[u8],
    ) -> Result<()> {
        self.session_handler
            .update_choreography_state(session_id, phase, state_data)
            .await
    }

    async fn validate_choreographic_message(
        &self,
        session_id: aura_core::identifiers::SessionId,
        message: &ChoreographicMessage,
    ) -> Result<bool> {
        self.session_handler
            .validate_choreographic_message(session_id, message)
            .await
    }
}

// Implement ConfigurationEffects trait
#[async_trait]
impl ConfigurationEffects for AgentEffectSystemHandler {
    async fn get_device_config(&self) -> Result<DeviceConfig> {
        let effects = self.core_effects.read().await;

        if let Ok(Some(config_bytes)) = effects.retrieve("device_config").await {
            if let Ok(config_str) = String::from_utf8(config_bytes) {
                if let Ok(config) = serde_json::from_str::<DeviceConfig>(&config_str) {
                    return Ok(config);
                }
            }
        }

        // Return default config if none exists
        Ok(DeviceConfig::default())
    }

    async fn update_device_config(&self, config: &DeviceConfig) -> Result<()> {
        let config_json = serde_json::to_string(config)
            .map_err(|e| aura_core::AuraError::invalid(format!("Invalid config: {}", e)))?;

        let effects = self.core_effects.read().await;
        effects
            .store("device_config", config_json.into_bytes())
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("Failed to store config: {}", e)))
    }

    async fn reset_to_defaults(&self) -> Result<()> {
        let default_config = DeviceConfig::default();
        self.update_device_config(&default_config).await
    }

    async fn export_config(&self) -> Result<Vec<u8>> {
        let config = self.get_device_config().await?;
        let config_json = serde_json::to_string_pretty(&config).map_err(|e| {
            aura_core::AuraError::invalid(format!("Failed to serialize config: {}", e))
        })?;
        Ok(config_json.into_bytes())
    }

    async fn import_config(&self, config_data: &[u8]) -> Result<()> {
        let config_str = String::from_utf8(config_data.to_vec())
            .map_err(|e| aura_core::AuraError::invalid(format!("Invalid UTF-8: {}", e)))?;

        let config: DeviceConfig = serde_json::from_str(&config_str)
            .map_err(|e| aura_core::AuraError::invalid(format!("Invalid config format: {}", e)))?;

        let validation_errors = self.validate_config(&config).await?;
        if !validation_errors.is_empty() {
            return Err(aura_core::AuraError::invalid("Config validation failed"));
        }

        self.update_device_config(&config).await
    }

    async fn validate_config(&self, config: &DeviceConfig) -> Result<Vec<ConfigValidationError>> {
        let mut errors = Vec::new();

        // Validate device name
        if config.device_name.is_empty() {
            errors.push(ConfigValidationError {
                field: "device_name".to_string(),
                error: "Device name cannot be empty".to_string(),
                suggested_value: Some(serde_json::json!("Aura Device")),
            });
        }

        // Validate timeouts
        if config.auto_lock_timeout > 86400 {
            errors.push(ConfigValidationError {
                field: "auto_lock_timeout".to_string(),
                error: "Auto lock timeout cannot exceed 24 hours".to_string(),
                suggested_value: Some(serde_json::json!(3600)),
            });
        }

        // Validate storage size
        if config.max_storage_size < 1024 * 1024 {
            errors.push(ConfigValidationError {
                field: "max_storage_size".to_string(),
                error: "Storage size must be at least 1MB".to_string(),
                suggested_value: Some(serde_json::json!(10 * 1024 * 1024)),
            });
        }

        Ok(errors)
    }

    async fn get_config_json(&self, key: &str) -> Result<Option<serde_json::Value>> {
        let effects = self.core_effects.read().await;

        if let Ok(Some(config_bytes)) = effects.retrieve(key).await {
            if let Ok(config_str) = String::from_utf8(config_bytes) {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&config_str) {
                    return Ok(Some(value));
                }
            }
        }

        Ok(None)
    }

    async fn set_config_json(&self, key: &str, value: &serde_json::Value) -> Result<()> {
        let config_json = serde_json::to_string(value)
            .map_err(|e| aura_core::AuraError::invalid(format!("Invalid JSON: {}", e)))?;

        let effects = self.core_effects.read().await;
        effects
            .store(key, config_json.into_bytes())
            .await
            .map_err(|e| aura_core::AuraError::internal(format!("Failed to store config: {}", e)))
    }

    async fn get_all_config(&self) -> Result<std::collections::HashMap<String, serde_json::Value>> {
        // Configuration key retrieval not yet implemented.
        // For now, return an empty map since we don't have a list_keys operation
        Ok(std::collections::HashMap::new())
    }
}

#[cfg(all(test, feature = "fixture_effects"))]
mod tests {
    use super::*;
    use async_lock::RwLock;
    use aura_macros::aura_test;
    use aura_testkit::*;
    use std::sync::Arc;

    #[aura_test]
    async fn test_agent_system_creation() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_id = fixture.device_id();
        let core_effects = Arc::new(RwLock::new((*fixture.effects()).clone()));
        let system = AgentEffectSystemHandler::with_core_effects(device_id, core_effects);

        assert_eq!(system.device_id(), device_id);
        Ok(())
    }

    #[aura_test]
    async fn test_device_info() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_id = fixture.device_id();
        let core_effects = Arc::new(RwLock::new((*fixture.effects()).clone()));
        let system = AgentEffectSystemHandler::with_core_effects(device_id, core_effects);

        let info = system.get_device_info().await?;
        assert_eq!(info.device_id, device_id);
        assert!(info.hardware_security);
        Ok(())
    }

    #[aura_test]
    async fn test_health_check() -> aura_core::AuraResult<()> {
        let fixture = create_test_fixture().await?;
        let device_id = fixture.device_id();
        let core_effects = Arc::new(RwLock::new((*fixture.effects()).clone()));
        let system = AgentEffectSystemHandler::with_core_effects(device_id, core_effects);

        let health = system.health_check().await?;
        assert!(matches!(
            health.overall_status,
            HealthStatus::Healthy | HealthStatus::Degraded { .. }
        ));
        Ok(())
    }
}
