//! Core Agent Runtime Composition
//!
//! This module provides the main AuraAgent implementation that composes
//! handlers and middleware into a unified device runtime. The agent follows
//! the runtime composition pattern by combining effect handlers rather than
//! implementing effects directly.

use crate::config::AgentConfig;
use crate::effects::*;
use crate::errors::{AgentError, Result as AgentResult};
use crate::handlers::AuthenticationHandler;
use crate::middleware::{AgentMiddlewareStack, MiddlewareStackBuilder};
// TODO: Remove dependency on aura_protocol until it compiles
// use aura_protocol::effects::AuraEffectSystem;
use aura_types::{identifiers::{DeviceId, AccountId}, AuraError};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Temporary stub for AuraEffectSystem until aura-protocol compiles
#[derive(Debug, Clone)]
pub struct AuraEffectSystem {
    device_id: DeviceId,
}

impl AuraEffectSystem {
    pub fn for_testing(device_id: DeviceId) -> Self {
        Self { device_id }
    }
    
    pub fn for_production(device_id: DeviceId) -> Result<Self, String> {
        Ok(Self { device_id })
    }
    
    pub fn for_simulation(device_id: DeviceId, _seed: u64) -> Self {
        Self { device_id }
    }
}

/// Core agent runtime that composes handlers and middleware
///
/// The AuraAgent represents a complete device runtime composed from:
/// - Core effect handlers (from aura-protocol)  
/// - Agent-specific handlers (authentication)
/// - Middleware stack (metrics, tracing, validation)
///
/// This follows the runtime composition pattern:
/// - **Handler Composition**: Combines specialized handlers into unified runtime
/// - **Middleware Integration**: Layers cross-cutting concerns over operations
/// - **Mode Awareness**: Supports production, testing, and simulation modes
/// - **Effect Injection**: All behavior controllable through injected effects
pub struct AuraAgent {
    /// Device ID for this agent runtime
    device_id: DeviceId,
    /// Authentication handler for device security
    auth_handler: AuthenticationHandler,
    /// Optional middleware stack for cross-cutting concerns
    middleware: Option<AgentMiddlewareStack>,
    /// Core effect system
    core_effects: Arc<RwLock<AuraEffectSystem>>,
    /// Configuration cache
    config_cache: Arc<RwLock<Option<AgentConfig>>>,
}

/// Device information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Device identifier
    pub device_id: DeviceId,
    /// Account this device belongs to
    pub account_id: Option<AccountId>,
    /// Device name
    pub device_name: String,
    /// Hardware security available
    pub hardware_security: bool,
    /// Device attestation available
    pub attestation_available: bool,
    /// Last sync timestamp
    pub last_sync: Option<u64>,
    /// Storage usage in bytes
    pub storage_usage: u64,
    /// Maximum storage in bytes
    pub storage_limit: u64,
}

impl AuraAgent {
    /// Create a new agent runtime by composing handlers
    ///
    /// This is the primary constructor that composes core effects and agent handlers
    /// into a unified runtime. All operations will be performed through composed handlers.
    pub fn new(core_effects: AuraEffectSystem, device_id: DeviceId) -> Self {
        let core_effects = Arc::new(RwLock::new(core_effects));
        
        Self {
            device_id,
            auth_handler: AuthenticationHandler::new(device_id, core_effects.clone()),
            middleware: None,
            core_effects,
            config_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Create agent with middleware stack
    ///
    /// This adds a middleware stack for cross-cutting concerns like metrics,
    /// tracing, and validation.
    pub fn with_middleware(mut self, middleware: AgentMiddlewareStack) -> Self {
        self.middleware = Some(middleware);
        self
    }

    /// Create agent for testing with mock effects
    pub fn for_testing(device_id: DeviceId) -> Self {
        let effects = AuraEffectSystem::for_testing(device_id);
        Self::new(effects, device_id)
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Initialize the agent
    ///
    /// This performs initial setup operations through effects:
    /// - Load or create default configuration
    /// - Initialize secure storage
    /// - Set up session management
    pub async fn initialize(&self) -> AgentResult<()> {
        // Load configuration through effects
        let config = self.load_or_create_config().await?;

        // Cache the config
        {
            let mut cache = self.config_cache.write().await;
            *cache = Some(config.clone());
        }

        // Initialize device storage through effects
        self.initialize_storage(&config).await?;

        // Initialize session management
        self.initialize_sessions().await?;

        // Log initialization completion
        let effects = self.core_effects.read().await;
        effects
            .log_info(&format!("Agent initialized for device {}", self.device_id))
            .await;

        Ok(())
    }

    /// Get device information
    pub async fn device_info(&self) -> AgentResult<DeviceInfo> {
        // Get config through effects
        let config = self.get_config().await?;

        // For now, use placeholder values until proper effect methods are implemented
        let hardware_security = false; // TODO: Implement through proper effect call
        let attestation_available = false; // TODO: Implement through proper effect call
        let storage_usage = 0; // TODO: Calculate through storage effects
        let last_sync = None; // TODO: Implement through config effects

        Ok(DeviceInfo {
            device_id: self.device_id,
            account_id: config.device.account_id,
            device_name: config.device.device_name,
            hardware_security,
            attestation_available,
            last_sync,
            storage_usage,
            storage_limit: config.device.max_storage_size,
        })
    }

    /// Store secure data using device storage effects
    pub async fn store_secure_data(&self, key: &str, data: &[u8]) -> AgentResult<()> {
        // TODO: Implement proper storage through effect execution
        // For now, return success as a placeholder
        Ok(())
    }

    /// Retrieve secure data using device storage effects
    pub async fn retrieve_secure_data(&self, key: &str) -> AgentResult<Option<Vec<u8>>> {
        // TODO: Implement proper storage retrieval through effect execution
        // For now, return None as placeholder
        Ok(None)
    }

    /// Delete secure data using device storage effects
    pub async fn delete_secure_data(&self, key: &str) -> AgentResult<()> {
        // TODO: Implement proper storage deletion through effect execution
        // For now, return success as placeholder
        Ok(())
    }

    /// Get current configuration
    pub async fn get_config(&self) -> AgentResult<AgentConfig> {
        // Check cache first
        {
            let cache = self.config_cache.read().await;
            if let Some(config) = cache.as_ref() {
                return Ok(config.clone());
            }
        }

        // Load from effects and cache
        let config = self.load_or_create_config().await?;
        {
            let mut cache = self.config_cache.write().await;
            *cache = Some(config.clone());
        }

        Ok(config)
    }

    /// Update configuration through effects
    pub async fn update_config(&self, config: AgentConfig) -> AgentResult<()> {
        // Validate configuration
        config
            .validate()
            .map_err(|e| AgentError::ValidationFailed(e))?;

        // Update cache
        {
            let mut cache = self.config_cache.write().await;
            *cache = Some(config.clone());
        }

        // TODO: Store through effects system
        Ok(())
    }

    /// Create a new session through effects
    pub async fn create_session(&self, session_type: &str) -> AgentResult<String> {
        // TODO: Implement proper session creation through effects
        // For now, return a placeholder session ID
        let session_id = format!("session_{}_{}", self.device_id.as_simple(), chrono::Utc::now().timestamp());
        Ok(session_id)
    }

    /// End a session through effects
    pub async fn end_session(&self, session_id: &str) -> AgentResult<()> {
        // TODO: Implement proper session ending through effects
        // For now, return success as placeholder
        Ok(())
    }

    /// List active sessions through effects
    pub async fn list_active_sessions(&self) -> AgentResult<Vec<String>> {
        // TODO: Implement proper session listing through effects
        // For now, return empty list as placeholder
        Ok(vec![])
    }

    /// Verify capability through effects
    pub async fn verify_capability(&self, capability: &str) -> AgentResult<bool> {
        // TODO: Implement proper capability verification through effects
        // For now, return false as placeholder (deny by default)
        Ok(false)
    }

    /// Sync with distributed journal through effects
    pub async fn sync_journal(&self) -> AgentResult<()> {
        // TODO: Implement proper journal sync through effects
        // For now, return success as placeholder
        Ok(())
    }

    // Private helper methods

    async fn load_or_create_config(&self) -> AgentResult<AgentConfig> {
        // Create default config for now
        // TODO: Implement proper config loading/saving through effects
        let default_config = AgentConfig::builder().device_id(self.device_id).build();
        Ok(default_config)
    }

    async fn initialize_storage(&self, config: &AgentConfig) -> AgentResult<()> {
        // TODO: Implement proper storage initialization through effects
        // For now, skip hardware security checks and storage initialization
        Ok(())
    }

    async fn initialize_sessions(&self) -> AgentResult<()> {
        // TODO: Implement proper session cleanup through effects
        // For now, skip session management initialization
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_creation() {
        let device_id = DeviceId::new();
        let effects = AuraEffectSystem::for_testing(device_id);
        let agent = AuraAgent::new(effects, device_id);

        assert_eq!(agent.device_id(), device_id);
    }

    #[tokio::test]
    async fn test_agent_initialization() {
        let device_id = DeviceId::new();
        let effects = AuraEffectSystem::for_testing(device_id);
        let agent = AuraAgent::new(effects, device_id);

        // Should not panic and should complete successfully
        agent
            .initialize()
            .await
            .expect("Initialization should succeed");

        // Should be able to get config after initialization
        let config = agent.get_config().await.expect("Should get config");
        assert_eq!(config.device.device_id, device_id);
    }

    #[tokio::test]
    async fn test_secure_storage_operations() {
        let device_id = DeviceId::new();
        let effects = AuraEffectSystem::for_testing(device_id);
        let agent = AuraAgent::new(effects, device_id);

        agent
            .initialize()
            .await
            .expect("Initialization should succeed");

        let key = "test_key";
        let data = b"test_data";

        // Store data
        agent
            .store_secure_data(key, data)
            .await
            .expect("Store should succeed");

        // Retrieve data
        let retrieved = agent
            .retrieve_secure_data(key)
            .await
            .expect("Retrieve should succeed");

        assert_eq!(retrieved, Some(data.to_vec()));

        // Delete data
        agent
            .delete_secure_data(key)
            .await
            .expect("Delete should succeed");

        // Verify deletion
        let after_delete = agent
            .retrieve_secure_data(key)
            .await
            .expect("Retrieve after delete should succeed");

        assert_eq!(after_delete, None);
    }

    #[tokio::test]
    async fn test_session_management() {
        let device_id = DeviceId::new();
        let effects = AuraEffectSystem::for_testing(device_id);
        let agent = AuraAgent::new(effects, device_id);

        agent
            .initialize()
            .await
            .expect("Initialization should succeed");

        // Create session
        let session_id = agent
            .create_session("recovery")
            .await
            .expect("Create session should succeed");

        assert!(!session_id.is_empty());

        // List sessions
        let sessions = agent
            .list_active_sessions()
            .await
            .expect("List sessions should succeed");

        assert!(sessions.contains(&session_id));

        // End session
        agent
            .end_session(&session_id)
            .await
            .expect("End session should succeed");
    }

    #[tokio::test]
    async fn test_config_management() {
        let device_id = DeviceId::new();
        let effects = AuraEffectSystem::for_testing(device_id);
        let agent = AuraAgent::new(effects, device_id);

        agent
            .initialize()
            .await
            .expect("Initialization should succeed");

        // Get initial config
        let mut config = agent.get_config().await.expect("Get config should succeed");
        let original_name = config.device.device_name.clone();

        // Update config
        config.device.device_name = "Updated Device".to_string();
        agent
            .update_config(config.clone())
            .await
            .expect("Update config should succeed");

        // Verify update
        let updated_config = agent.get_config().await.expect("Get config should succeed");
        assert_eq!(updated_config.device.device_name, "Updated Device");
        assert_ne!(updated_config.device.device_name, original_name);
    }
}
