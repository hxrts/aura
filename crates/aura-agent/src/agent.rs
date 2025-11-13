//! Core Agent Runtime Composition
//!
//! This module provides the main AuraAgent implementation that composes
//! handlers and middleware into a unified device runtime. The agent follows
//! the runtime composition pattern by combining effect handlers rather than
//! implementing effects directly.

use crate::config::AgentConfig;
use crate::effects::*;
use crate::errors::{AuraError, Result as AgentResult};
use crate::handlers::{
    AgentEffectSystemHandler, OtaOperations, RecoveryOperations, StorageOperations,
};
use crate::maintenance::{MaintenanceController, SnapshotOutcome};
use crate::middleware::AgentMiddlewareStack;
use aura_core::effects::{ConsoleEffects, StorageEffects};
use aura_core::identifiers::{AccountId, DeviceId};
use aura_protocol::effects::{AuraEffectSystem, SessionType};
use aura_sync::WriterFence;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid;

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
    /// Agent effect system handler that unifies all agent operations
    agent_handler: AgentEffectSystemHandler,
    /// Optional middleware stack for cross-cutting concerns
    middleware: Option<AgentMiddlewareStack>,
    /// Core effect system
    core_effects: Arc<RwLock<AuraEffectSystem>>,
    /// Storage operations handler
    storage_ops: StorageOperations,
    /// Recovery operations handler
    recovery_ops: RecoveryOperations,
    /// OTA upgrade operations handler
    ota_ops: OtaOperations,
    /// Maintenance workflows (snapshots, GC, OTA state)
    maintenance: MaintenanceController,
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

        // Create storage operations handler for secure storage
        let storage_ops = StorageOperations::new(
            core_effects.clone(),
            device_id,
            format!("agent_{}", device_id.0.simple()),
        );

        // Create recovery operations handler (default account_id for now)
        let recovery_ops = RecoveryOperations::new(
            core_effects.clone(),
            device_id,
            AccountId(uuid::Uuid::from_bytes([0u8; 16])), // Default, will be updated when config loads
        );

        let ota_ops = OtaOperations::new(device_id);
        let maintenance = MaintenanceController::new(core_effects.clone(), device_id);

        Self {
            device_id,
            agent_handler: AgentEffectSystemHandler::new(device_id, core_effects.clone()),
            middleware: None,
            core_effects,
            storage_ops,
            recovery_ops,
            ota_ops,
            maintenance,
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
        let config = aura_protocol::effects::EffectSystemConfig::for_testing(device_id);
        let effects = AuraEffectSystem::new(config).expect("Failed to create test effect system");
        Self::new(effects, device_id)
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Access maintenance controller (snapshots, GC, OTA state).
    pub fn maintenance(&self) -> &MaintenanceController {
        &self.maintenance
    }

    /// Access recovery operations handler.
    pub fn recovery(&self) -> &RecoveryOperations {
        &self.recovery_ops
    }

    /// Access OTA upgrade operations handler.
    pub fn ota(&self) -> &OtaOperations {
        &self.ota_ops
    }

    /// Get the global writer fence for snapshot proposals.
    pub fn writer_fence(&self) -> WriterFence {
        self.maintenance.writer_fence()
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
        let _ = effects
            .log_info(&format!("Agent initialized for device {}", self.device_id))
            .await;

        Ok(())
    }

    /// Get device information
    pub async fn device_info(&self) -> AgentResult<DeviceInfo> {
        // Get config through effects
        let config = self.get_config().await?;

        // TODO fix - For now, use placeholder values until proper effect methods are implemented
        let hardware_security = false; // TODO: Implement through proper effect call
        let attestation_available = false; // TODO: Implement through proper effect call
        let storage_usage = 0; // TODO: Calculate through storage effects
        let last_sync = None; // TODO: Implement through config effects

        Ok(DeviceInfo {
            device_id: self.device_id,
            account_id: config.account_id,
            device_name: format!("Device-{}", self.device_id.0), // TODO: Store in journal as CRDT fact
            hardware_security,
            attestation_available,
            last_sync,
            storage_usage,
            storage_limit: 1024 * 1024 * 1024, // 1GB default, TODO: Store in journal
        })
    }

    /// Store secure data using device storage effects
    pub async fn store_secure_data(&self, key: &str, data: &[u8]) -> AgentResult<()> {
        self.storage_ops
            .store_data_with_key(key, data)
            .await
            .map_err(|e| AuraError::internal(format!("Storage operation failed: {}", e)))
    }

    /// Retrieve secure data using device storage effects
    pub async fn retrieve_secure_data(&self, key: &str) -> AgentResult<Option<Vec<u8>>> {
        self.storage_ops
            .retrieve_data(key)
            .await
            .map_err(|e| AuraError::internal(format!("Storage operation failed: {}", e)))
    }

    /// Delete secure data using device storage effects
    pub async fn delete_secure_data(&self, key: &str) -> AgentResult<()> {
        self.storage_ops
            .delete_data(key)
            .await
            .map_err(|e| AuraError::internal(format!("Storage operation failed: {}", e)))
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
        // Validate device ID matches
        if config.device_id != self.device_id {
            return Err(AuraError::invalid("Device ID mismatch"));
        }

        // Serialize config
        let config_bytes = serde_json::to_vec(&config)
            .map_err(|e| AuraError::internal(format!("Failed to serialize config: {}", e)))?;

        // Store through effects
        let effects = self.core_effects.read().await;
        let config_key = format!("agent/config/{}", self.device_id.0);
        StorageEffects::store(&*effects, &config_key, config_bytes)
            .await
            .map_err(|e| AuraError::internal(format!("Failed to store config: {}", e)))?;

        // Update cache
        {
            let mut cache = self.config_cache.write().await;
            *cache = Some(config.clone());
        }

        ConsoleEffects::log_debug(
            &*effects,
            &format!("Saved config for device {}", self.device_id),
        )
        .await
        .ok();

        Ok(())
    }

    /// Create a new session through effects
    pub async fn create_session(&self, session_type: &str) -> AgentResult<String> {
        let session_ops = self.get_or_create_session_ops().await?;

        // Parse session type from string
        // Note: "recovery" sessions removed - use RecoveryOperations instead
        let session_type_enum = match session_type {
            "coordination" => SessionType::Coordination,
            "threshold" => SessionType::ThresholdOperation,
            "key_rotation" => SessionType::KeyRotation,
            _ => SessionType::Custom(session_type.to_string()),
        };

        let participants = vec![self.device_id]; // Self-participant for now
        let handle = session_ops
            .create_session(session_type_enum, participants)
            .await
            .map_err(|e| AuraError::internal(format!("Session creation failed: {}", e)))?;

        Ok(handle.session_id)
    }

    /// End a session through effects
    pub async fn end_session(&self, session_id: &str) -> AgentResult<()> {
        let session_ops = self.get_or_create_session_ops().await?;
        let _ = session_ops
            .end_session(session_id)
            .await
            .map_err(|e| AuraError::internal(format!("Session end failed: {}", e)))?;
        Ok(())
    }

    /// List active sessions through effects
    pub async fn list_active_sessions(&self) -> AgentResult<Vec<String>> {
        let session_ops = self.get_or_create_session_ops().await?;
        session_ops
            .list_active_sessions()
            .await
            .map_err(|e| AuraError::internal(format!("Session listing failed: {}", e)))
    }

    /// Verify capability through effects
    pub async fn verify_capability(&self, capability: &str) -> AgentResult<bool> {
        // TODO: Implement proper capability verification through effects
        // TODO fix - For now, return false as placeholder (deny by default)
        Ok(false)
    }

    /// Sync with distributed journal through effects
    pub async fn sync_journal(&self) -> AgentResult<()> {
        // TODO: Implement proper journal sync through effects
        // TODO fix - For now, return success as placeholder
        Ok(())
    }

    /// Trigger the maintenance snapshot workflow.
    pub async fn propose_snapshot(&self) -> AgentResult<SnapshotOutcome> {
        self.maintenance.propose_snapshot().await
    }

    /// Replace the administrator for an account (stub implementation).
    pub async fn replace_admin(
        &self,
        account_id: AccountId,
        new_admin: DeviceId,
        activation_epoch: u64,
    ) -> AgentResult<()> {
        self.maintenance
            .replace_admin_stub(account_id, new_admin, activation_epoch)
            .await
    }

    // Private helper methods

    async fn get_or_create_session_ops(&self) -> AgentResult<crate::handlers::SessionOperations> {
        // Get config to get account_id
        let config = self.get_config().await?;
        let account_id = config
            .account_id
            .unwrap_or_else(|| AccountId(uuid::Uuid::from_bytes([0u8; 16])));

        // Create fresh session operations each time
        // This is simpler than trying to cache non-cloneable operations
        Ok(crate::handlers::SessionOperations::new(
            self.core_effects.clone(),
            self.device_id,
            account_id,
        ))
    }

    async fn load_or_create_config(&self) -> AgentResult<AgentConfig> {
        let effects = self.core_effects.read().await;
        let config_key = format!("agent/config/{}", self.device_id.0);

        match StorageEffects::retrieve(&*effects, &config_key).await {
            Ok(Some(bytes)) => {
                // Deserialize existing config
                let config: AgentConfig = serde_json::from_slice(&bytes).map_err(|e| {
                    AuraError::internal(format!("Failed to deserialize config: {}", e))
                })?;

                ConsoleEffects::log_debug(
                    &*effects,
                    &format!("Loaded config for device {}", self.device_id),
                )
                .await
                .ok();
                Ok(config)
            }
            Ok(None) => {
                // No config exists, create default
                let config = AgentConfig {
                    device_id: self.device_id,
                    account_id: None,
                };

                ConsoleEffects::log_debug(
                    &*effects,
                    &format!(
                        "No existing config found, creating default for device {}",
                        self.device_id
                    ),
                )
                .await
                .ok();

                // Save the default config
                drop(effects); // Release read lock before calling update_config
                self.update_config(config.clone()).await?;

                Ok(config)
            }
            Err(e) => Err(AuraError::internal(format!("Failed to load config: {}", e))),
        }
    }

    async fn initialize_storage(&self, _config: &AgentConfig) -> AgentResult<()> {
        // Verify storage is accessible by performing a test write/read/delete
        let effects = self.core_effects.read().await;
        let test_key = format!("agent/init_check/{}", self.device_id.0);
        let test_data = b"initialized".to_vec();

        // Test write
        StorageEffects::store(&*effects, &test_key, test_data.clone())
            .await
            .map_err(|e| AuraError::internal(format!("Storage write test failed: {}", e)))?;

        // Test read
        let retrieved = StorageEffects::retrieve(&*effects, &test_key)
            .await
            .map_err(|e| AuraError::internal(format!("Storage read test failed: {}", e)))?;

        if retrieved.as_deref() != Some(test_data.as_slice()) {
            return Err(AuraError::internal("Storage verification failed"));
        }

        // Test delete
        StorageEffects::remove(&*effects, &test_key)
            .await
            .map_err(|e| AuraError::internal(format!("Storage delete test failed: {}", e)))?;

        ConsoleEffects::log_debug(
            &*effects,
            &format!("Storage initialized for device {}", self.device_id),
        )
        .await
        .ok();

        Ok(())
    }

    async fn initialize_sessions(&self) -> AgentResult<()> {
        // Session management is handled through MemorySessionHandler in aura-protocol
        // No initialization needed beyond what's already done in handler construction
        let effects = self.core_effects.read().await;
        ConsoleEffects::log_debug(
            &*effects,
            &format!("Session management ready for device {}", self.device_id),
        )
        .await
        .ok();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_agent_creation() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = aura_protocol::effects::EffectSystemConfig::for_testing(device_id);
        let effects = AuraEffectSystem::new(config).expect("Failed to create test effect system");
        let agent = AuraAgent::new(effects, device_id);

        assert_eq!(agent.device_id(), device_id);
    }

    #[tokio::test]
    async fn test_agent_initialization() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = aura_protocol::effects::EffectSystemConfig::for_testing(device_id);
        let effects = AuraEffectSystem::new(config).expect("Failed to create test effect system");
        let agent = AuraAgent::new(effects, device_id);

        // Should not panic and should complete successfully
        agent
            .initialize()
            .await
            .expect("Initialization should succeed");

        // Should be able to get config after initialization
        let config = agent.get_config().await.expect("Should get config");
        assert_eq!(config.device_id, device_id);
    }

    #[tokio::test]
    async fn test_secure_storage_operations() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = aura_protocol::effects::EffectSystemConfig::for_testing(device_id);
        let effects = AuraEffectSystem::new(config).expect("Failed to create test effect system");
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
        use aura_testkit::{test_device_pair, ChoreographyTestHarness};

        // Create a multi-device test harness
        let harness = test_device_pair();

        // Create coordinated session across devices
        let session = harness
            .create_coordinated_session("coordination")
            .await
            .expect("Should create coordinated session");

        assert!(!session.session_id().is_empty());
        assert_eq!(session.participants().len(), 2);

        // Verify session is active
        let status = session.status().await.expect("Should get session status");
        assert_eq!(status.session_type, "coordination");

        // End the session
        session
            .end()
            .await
            .expect("Should end session successfully");
    }

    #[tokio::test]
    async fn test_config_management() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let config = aura_protocol::effects::EffectSystemConfig::for_testing(device_id);
        let effects = AuraEffectSystem::new(config).expect("Failed to create test effect system");
        let agent = AuraAgent::new(effects, device_id);

        agent
            .initialize()
            .await
            .expect("Initialization should succeed");

        // Get initial config
        let mut config = agent.get_config().await.expect("Get config should succeed");
        let original_name = format!("Device-{}", device_id.0); // Placeholder

        // TODO: Device name stored in journal as CRDT fact, not in config
        agent
            .update_config(config.clone())
            .await
            .expect("Update config should succeed");

        // TODO: Verify device name update through journal effects
        // For now, just verify config update succeeded
        let _updated_config = agent.get_config().await.expect("Get config should succeed");
    }
}
