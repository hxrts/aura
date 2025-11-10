//! Unified agent effect system
//!
//! This module provides the unified effect system for agent operations,
//! integrating all agent-specific effects with the core Aura effect system
//! following the established architecture pattern.

use async_trait::async_trait;
use std::sync::Arc;

use aura_core::{
    handlers::{context::AuraContext, AuraHandler, AuraHandlerError, EffectType, ExecutionMode},
    identifiers::DeviceId,
    sessions::LocalSessionType,
};

use crate::effects::{
    AgentEffects, AuthError, AuthenticationEffects, ConfigError, ConfigurationEffects,
    DeviceStorageEffects, DeviceStorageError, SessionError, SessionManagementEffects,
};

use crate::handlers::{
    authentication::{
        MemoryAuthenticationHandler, MockAuthenticationHandler, RealAuthenticationHandler,
    },
    configuration::{
        MemoryConfigurationHandler, MockConfigurationHandler, RealConfigurationHandler,
    },
    device_storage::{
        MemoryDeviceStorageHandler, MockDeviceStorageHandler, RealDeviceStorageHandler,
    },
    session::{MemorySessionHandler, MockSessionHandler, RealSessionHandler},
};

/// Unified agent effect system that implements all agent-specific effects
pub struct AgentEffectSystem {
    device_id: DeviceId,
    execution_mode: ExecutionMode,
    device_storage: Box<dyn DeviceStorageEffects + Send + Sync>,
    session_management: Box<dyn SessionManagementEffects + Send + Sync>,
    authentication: Box<dyn AuthenticationEffects + Send + Sync>,
    configuration: Box<dyn ConfigurationEffects + Send + Sync>,
}

impl AgentEffectSystem {
    /// Create a new agent effect system with the specified handlers
    pub fn new(
        device_id: DeviceId,
        execution_mode: ExecutionMode,
        device_storage: Box<dyn DeviceStorageEffects + Send + Sync>,
        session_management: Box<dyn SessionManagementEffects + Send + Sync>,
        authentication: Box<dyn AuthenticationEffects + Send + Sync>,
        configuration: Box<dyn ConfigurationEffects + Send + Sync>,
    ) -> Self {
        Self {
            device_id,
            execution_mode,
            device_storage,
            session_management,
            authentication,
            configuration,
        }
    }

    /// Create agent effect system for testing
    pub fn for_testing(device_id: DeviceId) -> Self {
        let device_storage = Box::new(MemoryDeviceStorageHandler::new(device_id));
        let session_management = Box::new(MemorySessionHandler::new(device_id));
        let authentication = Box::new(MemoryAuthenticationHandler::with_full_permissions(
            device_id,
        ));
        let configuration = Box::new(MemoryConfigurationHandler::with_defaults(device_id));

        Self::new(
            device_id,
            ExecutionMode::Testing,
            device_storage,
            session_management,
            authentication,
            configuration,
        )
    }

    /// Create agent effect system for production
    pub fn for_production(device_id: DeviceId) -> Result<Self, AuraHandlerError> {
        let device_storage = Box::new(RealDeviceStorageHandler::new(device_id).map_err(|e| {
            AuraHandlerError::HandlerCreationFailed {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )),
            }
        })?);

        let session_management = Box::new(
            RealSessionHandler::with_default_path(device_id).map_err(|e| {
                AuraHandlerError::HandlerCreationFailed {
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )),
                }
            })?,
        );

        let authentication = Box::new(RealAuthenticationHandler::new(device_id).map_err(|e| {
            AuraHandlerError::HandlerCreationFailed {
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )),
            }
        })?);

        let configuration = Box::new(
            RealConfigurationHandler::with_default_path(device_id).map_err(|e| {
                AuraHandlerError::HandlerCreationFailed {
                    source: Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )),
                }
            })?,
        );

        Ok(Self::new(
            device_id,
            ExecutionMode::Production,
            device_storage,
            session_management,
            authentication,
            configuration,
        ))
    }

    /// Create agent effect system for simulation
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        let device_storage = Box::new(MemoryDeviceStorageHandler::new(device_id));
        let session_management = Box::new(MemorySessionHandler::new(device_id));
        let authentication = Box::new(MemoryAuthenticationHandler::with_full_permissions(
            device_id,
        ));
        let configuration = Box::new(MemoryConfigurationHandler::with_defaults(device_id));

        Self::new(
            device_id,
            ExecutionMode::Simulation { seed },
            device_storage,
            session_management,
            authentication,
            configuration,
        )
    }

    /// Create with mock handlers (all failing)
    pub fn failing(device_id: DeviceId) -> Self {
        let device_storage = Box::new(MockDeviceStorageHandler::failing(device_id));
        let session_management = Box::new(MockSessionHandler::failing(device_id));
        let authentication = Box::new(MockAuthenticationHandler::failing(device_id));
        let configuration = Box::new(MockConfigurationHandler::failing(device_id));

        Self::new(
            device_id,
            ExecutionMode::Testing,
            device_storage,
            session_management,
            authentication,
            configuration,
        )
    }

    /// Get the device ID this system is configured for
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Check if this system supports agent-specific effects
    fn supports_agent_effect(&self, effect_type: EffectType) -> bool {
        matches!(
            effect_type,
            EffectType::DeviceStorage
                | EffectType::SessionManagement
                | EffectType::Authentication
                | EffectType::Configuration
        )
    }

    /// Execute agent-specific effects
    async fn execute_agent_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        _ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match effect_type {
            EffectType::DeviceStorage => {
                self.execute_device_storage_effect(operation, parameters)
                    .await
            }
            EffectType::SessionManagement => {
                self.execute_session_management_effect(operation, parameters)
                    .await
            }
            EffectType::Authentication => {
                self.execute_authentication_effect(operation, parameters)
                    .await
            }
            EffectType::Configuration => {
                self.execute_configuration_effect(operation, parameters)
                    .await
            }
            _ => Err(AuraHandlerError::UnsupportedEffect { effect_type }),
        }
    }

    /// Execute device storage effects
    async fn execute_device_storage_effect(
        &mut self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "store_secure" => {
                let params: (String, Vec<u8>) = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                self.device_storage
                    .store_secure(&params.0, &params.1)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            "retrieve_secure" => {
                let key: String = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                let result = self
                    .device_storage
                    .retrieve_secure(&key)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "delete_secure" => {
                let key: String = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                self.device_storage.delete_secure(&key).await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;

                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            "list_keys" => {
                let result = self.device_storage.list_keys().await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "has_hardware_security" => {
                let result = self.device_storage.has_hardware_security().await;
                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "get_device_attestation" => {
                let result = self
                    .device_storage
                    .get_device_attestation()
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::DeviceStorage,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute session management effects
    async fn execute_session_management_effect(
        &mut self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "create_session" => {
                let session_data = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                let result = self
                    .session_management
                    .create_session(session_data)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "get_session" => {
                let session_id: String = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                let result = self
                    .session_management
                    .get_session(&session_id)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "end_session" => {
                let session_id: String = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                let result = self
                    .session_management
                    .end_session(&session_id)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "list_active_sessions" => {
                let result = self
                    .session_management
                    .list_active_sessions()
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::SessionManagement,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute authentication effects
    async fn execute_authentication_effect(
        &mut self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "verify_capability" => {
                let token = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                let result = self
                    .authentication
                    .verify_capability(&token)
                    .await
                    .map_err(|e| AuraHandlerError::AuthorizationFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "is_authorized" => {
                let params: (DeviceId, String) = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                let result = self
                    .authentication
                    .is_authorized(params.0, &params.1)
                    .await
                    .map_err(|e| AuraHandlerError::AuthorizationFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "get_device_identity" => {
                let result = self
                    .authentication
                    .get_device_identity()
                    .await
                    .map_err(|e| AuraHandlerError::AuthorizationFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "create_capability" => {
                let permissions: Vec<String> = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                let result = self
                    .authentication
                    .create_capability(permissions)
                    .await
                    .map_err(|e| AuraHandlerError::AuthorizationFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Authentication,
                operation: operation.to_string(),
            }),
        }
    }

    /// Execute configuration effects
    async fn execute_configuration_effect(
        &mut self,
        operation: &str,
        parameters: &[u8],
    ) -> Result<Vec<u8>, AuraHandlerError> {
        match operation {
            "get_config_json" => {
                let key: String = bincode::deserialize(parameters).map_err(|e| {
                    AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    }
                })?;

                let result = self
                    .configuration
                    .get_config_json(&key)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            "set_config_json" => {
                let params: (String, serde_json::Value) = bincode::deserialize(parameters)
                    .map_err(|e| AuraHandlerError::ParameterDeserializationFailed {
                        source: Box::new(e),
                    })?;

                self.configuration
                    .set_config_json(&params.0, params.1)
                    .await
                    .map_err(|e| AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    })?;

                Ok(bincode::serialize(&()).unwrap_or_default())
            }
            "get_all_config" => {
                let result = self.configuration.get_all_config().await.map_err(|e| {
                    AuraHandlerError::ExecutionFailed {
                        source: Box::new(e),
                    }
                })?;

                Ok(bincode::serialize(&result).unwrap_or_default())
            }
            _ => Err(AuraHandlerError::UnsupportedOperation {
                effect_type: EffectType::Configuration,
                operation: operation.to_string(),
            }),
        }
    }
}

// Implement individual effect traits for the unified system
#[async_trait]
impl DeviceStorageEffects for AgentEffectSystem {
    async fn store_secure(&self, key: &str, value: &[u8]) -> Result<(), DeviceStorageError> {
        self.device_storage.store_secure(key, value).await
    }

    async fn retrieve_secure(&self, key: &str) -> Result<Option<Vec<u8>>, DeviceStorageError> {
        self.device_storage.retrieve_secure(key).await
    }

    async fn delete_secure(&self, key: &str) -> Result<(), DeviceStorageError> {
        self.device_storage.delete_secure(key).await
    }

    async fn list_keys(&self) -> Result<Vec<String>, DeviceStorageError> {
        self.device_storage.list_keys().await
    }

    async fn has_hardware_security(&self) -> bool {
        self.device_storage.has_hardware_security().await
    }

    async fn get_device_attestation(
        &self,
    ) -> Result<Option<crate::effects::DeviceAttestation>, DeviceStorageError> {
        self.device_storage.get_device_attestation().await
    }
}

#[async_trait]
impl SessionManagementEffects for AgentEffectSystem {
    async fn create_session(
        &self,
        session_data: crate::effects::SessionData,
    ) -> Result<String, SessionError> {
        self.session_management.create_session(session_data).await
    }

    async fn get_session(
        &self,
        session_id: &str,
    ) -> Result<Option<crate::effects::SessionData>, SessionError> {
        self.session_management.get_session(session_id).await
    }

    async fn update_session(
        &self,
        session_id: &str,
        update: crate::effects::SessionUpdate,
    ) -> Result<(), SessionError> {
        self.session_management
            .update_session(session_id, update)
            .await
    }

    async fn end_session(
        &self,
        session_id: &str,
    ) -> Result<crate::effects::SessionData, SessionError> {
        self.session_management.end_session(session_id).await
    }

    async fn list_active_sessions(&self) -> Result<Vec<String>, SessionError> {
        self.session_management.list_active_sessions().await
    }

    async fn cleanup_expired_sessions(
        &self,
        max_age_seconds: u64,
    ) -> Result<Vec<String>, SessionError> {
        self.session_management
            .cleanup_expired_sessions(max_age_seconds)
            .await
    }
}

#[async_trait]
impl AuthenticationEffects for AgentEffectSystem {
    async fn verify_capability(
        &self,
        token: &aura_core::capabilities::CapabilityToken,
    ) -> Result<bool, AuthError> {
        self.authentication.verify_capability(token).await
    }

    async fn is_authorized(&self, device_id: DeviceId, operation: &str) -> Result<bool, AuthError> {
        self.authentication
            .is_authorized(device_id, operation)
            .await
    }

    async fn get_device_identity(&self) -> Result<DeviceId, AuthError> {
        self.authentication.get_device_identity().await
    }

    async fn create_capability(
        &self,
        permissions: Vec<String>,
    ) -> Result<aura_core::capabilities::CapabilityToken, AuthError> {
        self.authentication.create_capability(permissions).await
    }
}

#[async_trait]
impl ConfigurationEffects for AgentEffectSystem {
    async fn get_config_json(&self, key: &str) -> Result<Option<serde_json::Value>, ConfigError> {
        self.configuration.get_config_json(key).await
    }

    async fn set_config_json(
        &self,
        key: &str,
        value: serde_json::Value,
    ) -> Result<(), ConfigError> {
        self.configuration.set_config_json(key, value).await
    }

    async fn get_all_config(
        &self,
    ) -> Result<std::collections::HashMap<String, serde_json::Value>, ConfigError> {
        self.configuration.get_all_config().await
    }
}

impl AgentEffects for AgentEffectSystem {
    fn device_id(&self) -> DeviceId {
        self.device_id
    }

    fn is_simulation(&self) -> bool {
        matches!(self.execution_mode, ExecutionMode::Simulation { .. })
    }
}

impl AgentEffectSystem {
    /// Generate a session ID using effects-based UUID generation
    pub async fn generate_session_id(&self) -> Result<String, SessionError> {
        // Use a deterministic UUID generation for effects compliance
        // In production, this would come from the effects system
        // TODO fix - For now, we'll use a placeholder that's effects-compliant
        Ok(format!("session_{}", self.device_id.0))
    }

    /// Get current timestamp using effects-based time
    pub async fn current_timestamp(&self) -> Result<u64, SessionError> {
        // This should use the effects system for time
        // TODO fix - For now, return epoch 0 as placeholder - production would use actual effects
        Ok(0)
    }
}

#[async_trait]
impl AuraHandler for AgentEffectSystem {
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &mut AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        if self.supports_agent_effect(effect_type) {
            self.execute_agent_effect(effect_type, operation, parameters, ctx)
                .await
        } else {
            Err(AuraHandlerError::UnsupportedEffect { effect_type })
        }
    }

    async fn execute_session(
        &mut self,
        _session: LocalSessionType,
        _ctx: &mut AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Agent system doesn't directly handle session types
        // Session types would be handled by the choreography layer
        Ok(())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.supports_agent_effect(effect_type)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

/// Factory for creating agent effect systems
pub struct AgentEffectSystemFactory;

impl AgentEffectSystemFactory {
    /// Create an agent effect system for testing
    pub fn for_testing(device_id: DeviceId) -> AgentEffectSystem {
        AgentEffectSystem::for_testing(device_id)
    }

    /// Create an agent effect system for production
    pub fn for_production(device_id: DeviceId) -> Result<AgentEffectSystem, AuraHandlerError> {
        AgentEffectSystem::for_production(device_id)
    }

    /// Create an agent effect system for simulation
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> AgentEffectSystem {
        AgentEffectSystem::for_simulation(device_id, seed)
    }

    /// Create a failing agent effect system for testing error conditions
    pub fn failing(device_id: DeviceId) -> AgentEffectSystem {
        AgentEffectSystem::failing(device_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_effect_system_creation() {
        let device_id = DeviceId::new();

        // Test testing system
        let system = AgentEffectSystemFactory::for_testing(device_id);
        assert_eq!(system.device_id(), device_id);
        assert_eq!(system.execution_mode(), ExecutionMode::Testing);

        // Test simulation system
        let system = AgentEffectSystemFactory::for_simulation(device_id, 42);
        assert_eq!(system.device_id(), device_id);
        assert_eq!(
            system.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );

        // Test failing system
        let system = AgentEffectSystemFactory::failing(device_id);
        assert_eq!(system.device_id(), device_id);
    }

    #[tokio::test]
    async fn test_effect_support() {
        let device_id = DeviceId::new();
        let system = AgentEffectSystemFactory::for_testing(device_id);

        // Should support agent effects
        assert!(system.supports_effect(EffectType::DeviceStorage));
        assert!(system.supports_effect(EffectType::SessionManagement));
        assert!(system.supports_effect(EffectType::Authentication));
        assert!(system.supports_effect(EffectType::Configuration));

        // Should not support non-agent effects
        assert!(!system.supports_effect(EffectType::Crypto));
        assert!(!system.supports_effect(EffectType::Network));
    }

    #[tokio::test]
    async fn test_agent_effects_trait() {
        let device_id = DeviceId::new();
        let system = AgentEffectSystemFactory::for_testing(device_id);

        // Test AgentEffects trait implementation
        assert_eq!(system.device_id(), device_id);
        assert!(!system.is_simulation());

        let simulation_system = AgentEffectSystemFactory::for_simulation(device_id, 42);
        assert!(simulation_system.is_simulation());
    }

    #[tokio::test]
    async fn test_individual_effect_traits() {
        let device_id = DeviceId::new();
        let system = AgentEffectSystemFactory::for_testing(device_id);

        // Test DeviceStorageEffects
        system
            .store_secure("test_key", b"test_value")
            .await
            .unwrap();
        let retrieved = system.retrieve_secure("test_key").await.unwrap();
        assert_eq!(retrieved, Some(b"test_value".to_vec()));

        // Test AuthenticationEffects
        let identity = system.get_device_identity().await.unwrap();
        assert_eq!(identity, device_id);

        let authorized = system
            .is_authorized(device_id, "test_operation")
            .await
            .unwrap();
        assert!(authorized);

        // Test ConfigurationEffects
        system
            .set_config_json("test_config", serde_json::json!("test_value"))
            .await
            .unwrap();
        let config_value = system.get_config_json("test_config").await.unwrap();
        assert_eq!(config_value, Some(serde_json::json!("test_value")));
    }
}
