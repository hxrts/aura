//! Core Agent Runtime Composition
//!
//! This module provides the main AuraAgent implementation that composes
//! handlers into a unified device runtime. The agent follows
//! the runtime composition pattern by combining effect handlers rather than
//! implementing effects directly.

use crate::config::AgentConfig;
use crate::errors::{AuraError, Result as AgentResult};
use crate::handlers::{
    AgentEffectSystemHandler, OtaOperations, RecoveryOperations, StorageOperations,
};
use crate::maintenance::{MaintenanceController, SnapshotOutcome};
use crate::runtime::AuraEffectSystem;
use aura_core::effects::{agent::SessionType, ConsoleEffects, StorageEffects};
use aura_core::identifiers::{AccountId, AuthorityId, DeviceId};
use aura_sync::WriterFence;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid;

/// Core agent runtime that composes handlers through effect system
///
/// The AuraAgent represents a complete device runtime composed from:
/// - Core effect handlers (from aura-protocol)
/// - Agent-specific handlers (authentication, storage, recovery, OTA)
/// - Effect system composition for unified runtime behavior
///
/// This follows the runtime composition pattern:
/// - **Handler Composition**: Combines specialized handlers into unified runtime
/// - **Effect Injection**: All behavior controllable through injected effects
/// - **Mode Awareness**: Supports production, testing, and simulation modes
/// - **Clean Architecture**: No middleware layers - direct effect composition
pub struct AuraAgent {
    /// Authority ID for this agent runtime (public identity)
    authority_id: AuthorityId,
    /// Device ID for internal ratchet tree operations (private)
    /// TODO: In multi-device authorities, this should be looked up from authority state
    device_id: DeviceId,
    /// Agent effect system handler that unifies all agent operations
    _agent_handler: AgentEffectSystemHandler,
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

/// Authority information (replaces DeviceInfo in authority-centric model)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Authority identifier (public identity)
    pub authority_id: AuthorityId,
    /// Account this authority belongs to
    /// NOTE: Deprecated - authority_id is the primary identifier
    pub account_id: Option<AccountId>,
    /// Authority display name
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
    ///
    /// # Arguments
    /// * `core_effects` - The effect system for this agent
    /// * `authority_id` - The public authority identifier
    ///
    /// # Device ID Derivation
    /// For single-device authorities, device_id is derived from authority_id.
    /// TODO: For multi-device authorities, device_id should be passed explicitly
    /// or looked up from authority state.
    pub fn new(core_effects: AuraEffectSystem, authority_id: AuthorityId) -> Self {
        let core_effects = Arc::new(RwLock::new(core_effects));

        // Derive device_id from authority_id (1:1 mapping for single-device authorities)
        // TODO: For multi-device authorities, this should be looked up from authority state
        let device_id = DeviceId(authority_id.0);

        // Create storage operations handler for secure storage
        let storage_ops = StorageOperations::new(
            core_effects.clone(),
            device_id,
            format!("agent_{}", authority_id.0.simple()),
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
            authority_id,
            device_id,
            _agent_handler: (), // Stub: AgentEffectSystemHandler is unit type
            core_effects,
            storage_ops,
            recovery_ops,
            ota_ops,
            maintenance,
            config_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Create agent for testing with mock effects
    ///
    /// Note: This method is deprecated. Use `aura_testkit::create_test_fixture().await`
    /// and construct agent via `AuraAgent::new()` in new code.
    pub fn for_testing(authority_id: AuthorityId) -> Self {
        // Derive device_id for EffectRegistry (still uses device_id internally)
        let device_id = DeviceId(authority_id.0);

        // Use new EffectRegistry pattern for standardized testing setup
        let effects_arc = crate::runtime::EffectRegistry::testing()
            .with_device_id(device_id)
            .build()
            .expect("Failed to create test effect system");
        // Unwrap the Arc - we're the only owner at this point
        let effects = Arc::try_unwrap(effects_arc)
            .unwrap_or_else(|arc| (*arc).clone());
        Self::new(effects, authority_id)
    }

    /// Get authority ID (public identity)
    pub fn authority_id(&self) -> AuthorityId {
        self.authority_id
    }

    /// Get device ID (internal identifier)
    ///
    /// NOTE: This method is for internal use only. External code should use authority_id().
    /// Device IDs are implementation details of the ratchet tree and should not be exposed
    /// in public APIs.
    #[doc(hidden)]
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
            .log_info(&format!("Agent initialized for authority {}", self.authority_id))
            .await;

        Ok(())
    }

    /// Get authority information
    pub async fn device_info(&self) -> AgentResult<DeviceInfo> {
        // Get config through effects
        let config = self.get_config().await?;

        // TODO fix - For now, use placeholder values until proper effect methods are implemented
        let hardware_security = false; // TODO: Implement through proper effect call
        let attestation_available = false; // TODO: Implement through proper effect call
        let storage_usage = 0; // TODO: Calculate through storage effects
        let last_sync = None; // TODO: Implement through config effects

        Ok(DeviceInfo {
            authority_id: self.authority_id,
            account_id: config.account_id,
            device_name: format!("Authority-{}", self.authority_id.0), // TODO: Store in journal as CRDT fact
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
        // Validate authority ID matches
        if config.authority_id != self.authority_id {
            return Err(AuraError::invalid("Authority ID mismatch"));
        }

        // Serialize config
        let config_bytes = serde_json::to_vec(&config)
            .map_err(|e| AuraError::internal(format!("Failed to serialize config: {}", e)))?;

        // Store through effects
        let effects = self.core_effects.read().await;
        let config_key = format!("agent/config/{}", self.authority_id.0);
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
            &format!("Saved config for authority {}", self.authority_id),
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
        // Get current journal state to check capabilities
        let core_effects = self.core_effects.read().await;
        let journal_result = aura_core::effects::JournalEffects::get_journal(&*core_effects).await;
        let journal = match journal_result {
            Ok(journal) => journal,
            Err(e) => {
                tracing::error!("Failed to get journal for capability verification: {:?}", e);
                return Ok(false); // Deny by default on error
            }
        };

        // Check if current capabilities allow this operation
        let resource = "agent:operations"; // Default resource scope for agent operations
        let permission = format!("{}:{}", resource, capability);
        let authorized = journal.caps.allows(&permission);

        tracing::debug!(
            capability = capability,
            resource = resource,
            authorized = authorized,
            "Agent capability verification"
        );

        Ok(authorized)
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
    ///
    /// NOTE: This method takes AuthorityId for the new admin. Internally, it derives
    /// the DeviceId for the maintenance controller.
    /// TODO: Refactor MaintenanceController to use AuthorityId
    pub async fn replace_admin(
        &self,
        account_id: AccountId,
        new_admin_authority: AuthorityId,
        activation_epoch: u64,
    ) -> AgentResult<()> {
        // Derive device_id from authority_id for now (1:1 mapping)
        // TODO: MaintenanceController should be refactored to use AuthorityId
        let new_admin_device = DeviceId(new_admin_authority.0);

        self.maintenance
            .replace_admin_stub(account_id, new_admin_device, activation_epoch)
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
        let config_key = format!("agent/config/{}", self.authority_id.0);

        match StorageEffects::retrieve(&*effects, &config_key).await {
            Ok(Some(bytes)) => {
                // Deserialize existing config
                let config: AgentConfig = serde_json::from_slice(&bytes).map_err(|e| {
                    AuraError::internal(format!("Failed to deserialize config: {}", e))
                })?;

                ConsoleEffects::log_debug(
                    &*effects,
                    &format!("Loaded config for authority {}", self.authority_id),
                )
                .await
                .ok();
                Ok(config)
            }
            Ok(None) => {
                // No config exists, create default
                let config = AgentConfig {
                    authority_id: self.authority_id,
                    account_id: None,
                };

                ConsoleEffects::log_debug(
                    &*effects,
                    &format!(
                        "No existing config found, creating default for authority {}",
                        self.authority_id
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
        let test_key = format!("agent/init_check/{}", self.authority_id.0);
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
            &format!("Storage initialized for authority {}", self.authority_id),
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
            &format!("Session management ready for authority {}", self.authority_id),
        )
        .await
        .ok();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_macros::aura_test;

    #[aura_test]
    async fn test_agent_creation() -> aura_core::AuraResult<()> {
        let authority_id = AuthorityId(uuid::Uuid::from_bytes([0u8; 16]));
        let effects = AuraEffectSystem::new();
        let agent = AuraAgent::new(effects, authority_id);

        assert_eq!(agent.authority_id(), authority_id);
        Ok(())
    }

    #[aura_test]
    async fn test_agent_initialization() -> aura_core::AuraResult<()> {
        let authority_id = AuthorityId(uuid::Uuid::from_bytes([0u8; 16]));
        let effects = AuraEffectSystem::new();
        let agent = AuraAgent::new(effects, authority_id);

        // Should not panic and should complete successfully
        agent.initialize().await?;

        // Should be able to get config after initialization
        let config = agent.get_config().await?;
        assert_eq!(config.authority_id, authority_id);
        Ok(())
    }

    #[aura_test]
    async fn test_secure_storage_operations() -> aura_core::AuraResult<()> {
        let authority_id = AuthorityId(uuid::Uuid::from_bytes([0u8; 16]));
        let effects = AuraEffectSystem::new();
        let agent = AuraAgent::new(effects, authority_id);

        agent.initialize().await?;

        let key = "test_key";
        let data = b"test_data";

        // Store data
        agent.store_secure_data(key, data).await?;

        // Retrieve data
        let retrieved = agent.retrieve_secure_data(key).await?;

        assert_eq!(retrieved, Some(data.to_vec()));

        // Delete data
        agent.delete_secure_data(key).await?;

        // Verify deletion
        let after_delete = agent.retrieve_secure_data(key).await?;

        assert_eq!(after_delete, None);
        Ok(())
    }

    #[aura_test]
    async fn test_session_management() -> aura_core::AuraResult<()> {
        use aura_testkit::test_device_pair;

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

        Ok(())
    }

    #[aura_test]
    async fn test_config_management() -> aura_core::AuraResult<()> {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let effects = AuraEffectSystem::new();
        let agent = AuraAgent::new(effects, device_id);

        agent.initialize().await?;

        // Get initial config
        let config = agent.get_config().await?;
        let _original_name = format!("Device-{}", device_id.0); // Placeholder

        // TODO: Device name stored in journal as CRDT fact, not in config
        agent.update_config(config.clone()).await?;

        // TODO: Verify device name update through journal effects
        // For now, just verify config update succeeded
        let _updated_config = agent.get_config().await?;
        Ok(())
    }
}
