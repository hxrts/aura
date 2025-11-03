//! Agent operation handlers

use super::{AgentHandler, AgentContext};
use crate::error::{AuraError, Result};
use crate::middleware::AgentOperation;
use crate::device_secure_store::SecureStorage;
use crate::utils::time::AgentTimeProvider;
use aura_types::DeviceId;
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use base64::Engine;

/// Main agent handler that processes operations using secure storage
pub struct CoreAgentHandler {
    /// Secure storage interface
    storage: Arc<dyn SecureStorage>,
    
    /// Agent state tracking
    state: Arc<RwLock<AgentState>>,
    
    /// Time provider for consistent timestamps
    time_provider: Arc<AgentTimeProvider>,
}

impl CoreAgentHandler {
    /// Create a new core agent handler
    pub fn new(storage: Arc<dyn SecureStorage>) -> Self {
        Self {
            storage,
            state: Arc::new(RwLock::new(AgentState::new())),
            time_provider: Arc::new(AgentTimeProvider::production()),
        }
    }
    
    /// Create a new core agent handler with custom time provider
    pub fn with_time_provider(storage: Arc<dyn SecureStorage>, time_provider: Arc<AgentTimeProvider>) -> Self {
        Self {
            storage,
            state: Arc::new(RwLock::new(AgentState::new())),
            time_provider,
        }
    }
    
    /// Get current agent state
    pub fn get_state(&self) -> Result<AgentState> {
        Ok(self.state.read().map_err(|_| {
            AuraError::internal_error("Failed to acquire read lock on agent state")
        })?.clone())
    }
}

impl AgentHandler for CoreAgentHandler {
    fn handle(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
    ) -> Result<serde_json::Value> {
        match operation {
            AgentOperation::Initialize { threshold, share_count } => {
                let mut state = self.state.write().map_err(|_| {
                    AuraError::internal_error("Failed to acquire write lock on agent state")
                })?;
                
                // Initialize agent with FROST key shares
                state.initialize(threshold, share_count)?;
                
                // Store initialization data securely
                let init_data = serde_json::json!({
                    "threshold": threshold,
                    "share_count": share_count,
                    "initialized_at": context.timestamp,
                    "device_id": context.device_id.to_string(),
                    "account_id": context.account_id.to_string(),
                });
                
                let key = format!("agent_init_{}", context.account_id);
                self.storage.store_data(
                    &key,
                    &serde_json::to_vec(&init_data).map_err(|e| {
                        AuraError::serialization_error(e.to_string())
                    })?,
                    crate::device_secure_store::SecurityLevel::HSM,
                )?;
                
                Ok(serde_json::json!({
                    "operation": "initialize",
                    "threshold": threshold,
                    "share_count": share_count,
                    "success": true
                }))
            }
            
            AgentOperation::DeriveIdentity { app_id, context: app_context } => {
                let state = self.state.read().map_err(|_| {
                    AuraError::internal_error("Failed to acquire read lock on agent state")
                })?;
                
                if !state.is_initialized() {
                    return Err(AuraError::not_initialized(""));
                }
                
                // Derive identity using DKD protocol (simplified)
                let identity_key = format!("{}:{}:{}", context.account_id, app_id, app_context);
                let identity_hash = blake3::hash(identity_key.as_bytes());
                
                Ok(serde_json::json!({
                    "operation": "derive_identity",
                    "app_id": app_id,
                    "context": app_context,
                    "identity_hash": hex::encode(identity_hash.as_bytes()),
                    "success": true
                }))
            }
            
            AgentOperation::StartSession { session_type, participants } => {
                let mut state = self.state.write().map_err(|_| {
                    AuraError::internal_error("Failed to acquire write lock on agent state")
                })?;
                
                if !state.is_initialized() {
                    return Err(AuraError::not_initialized(""));
                }
                
                let session_id = uuid::Uuid::new_v4().to_string();
                state.start_session(session_id.clone(), session_type.clone(), participants.clone())?;
                
                Ok(serde_json::json!({
                    "operation": "start_session",
                    "session_id": session_id,
                    "session_type": session_type,
                    "participants": participants.len(),
                    "success": true
                }))
            }
            
            AgentOperation::StoreData { data, capabilities } => {
                let state = self.state.read().map_err(|_| {
                    AuraError::internal_error("Failed to acquire read lock on agent state")
                })?;
                
                if !state.is_initialized() {
                    return Err(AuraError::not_initialized(""));
                }
                
                let data_id = uuid::Uuid::new_v4().to_string();
                let key = format!("data_{}_{}", context.account_id, data_id);
                
                // Store with metadata
                let stored_data = serde_json::json!({
                    "data": base64::engine::general_purpose::STANDARD.encode(&data),
                    "capabilities": capabilities,
                    "stored_at": context.timestamp,
                    "device_id": context.device_id.to_string(),
                });
                
                self.storage.store_data(
                    &key,
                    &serde_json::to_vec(&stored_data).map_err(|e| {
                        AuraError::serialization_error(e.to_string())
                    })?,
                    crate::device_secure_store::SecurityLevel::HSM,
                )?;
                
                Ok(serde_json::json!({
                    "operation": "store_data",
                    "data_id": data_id,
                    "data_size": data.len(),
                    "capabilities": capabilities,
                    "success": true
                }))
            }
            
            AgentOperation::RetrieveData { data_id, required_capability } => {
                let state = self.state.read().map_err(|_| {
                    AuraError::internal_error("Failed to acquire read lock on agent state")
                })?;
                
                if !state.is_initialized() {
                    return Err(AuraError::not_initialized(""));
                }
                
                let key = format!("data_{}_{}", context.account_id, data_id);
                
                match self.storage.retrieve_data(&key) {
                    Ok(stored_bytes) => {
                        let stored_data: serde_json::Value = serde_json::from_slice(&stored_bytes)
                            .map_err(|e| AuraError::deserialization_error(e.to_string()))?;
                        
                        let capabilities = stored_data["capabilities"].as_array()
                            .ok_or_else(|| AuraError::invalid_data("Missing capabilities".to_string()))?;
                        
                        // Check if required capability is present
                        let has_capability = capabilities.iter()
                            .any(|cap| cap.as_str() == Some(&required_capability));
                        
                        if !has_capability {
                            return Err(AuraError::permission_denied(
                                format!("Missing required capability: {}", required_capability)
                            ));
                        }
                        
                        let data_b64 = stored_data["data"].as_str()
                            .ok_or_else(|| AuraError::invalid_data("Missing data".to_string()))?;
                        
                        let data = base64::engine::general_purpose::STANDARD.decode(data_b64)
                            .map_err(|e| AuraError::invalid_data(format!("Invalid base64: {}", e)))?;
                        
                        Ok(serde_json::json!({
                            "operation": "retrieve_data",
                            "data_id": data_id,
                            "data_size": data.len(),
                            "data": base64::engine::general_purpose::STANDARD.encode(&data),
                            "success": true
                        }))
                    }
                    Err(_) => {
                        Err(AuraError::data_not_found(data_id))
                    }
                }
            }
            
            AgentOperation::InitiateBackup { backup_type, guardians } => {
                let mut state = self.state.write().map_err(|_| {
                    AuraError::internal_error("Failed to acquire write lock on agent state")
                })?;
                
                if !state.is_initialized() {
                    return Err(AuraError::not_initialized(""));
                }
                
                let backup_id = uuid::Uuid::new_v4().to_string();
                state.start_backup(backup_id.clone(), backup_type.clone(), guardians.clone())?;
                
                Ok(serde_json::json!({
                    "operation": "initiate_backup",
                    "backup_id": backup_id,
                    "backup_type": backup_type,
                    "guardians": guardians.len(),
                    "success": true
                }))
            }
            
            AgentOperation::GetStatus => {
                let state = self.state.read().map_err(|_| {
                    AuraError::internal_error("Failed to acquire read lock on agent state")
                })?;
                
                Ok(serde_json::json!({
                    "operation": "get_status",
                    "initialized": state.is_initialized(),
                    "active_sessions": state.active_sessions.len(),
                    "active_backups": state.active_backups.len(),
                    "success": true
                }))
            }
        }
    }
}

/// Agent state tracking
#[derive(Debug, Clone)]
pub struct AgentState {
    /// Whether agent is initialized
    pub initialized: bool,
    
    /// FROST threshold configuration
    pub threshold: Option<u32>,
    
    /// FROST share count
    pub share_count: Option<u32>,
    
    /// Active sessions
    pub active_sessions: HashMap<String, SessionInfo>,
    
    /// Active backup operations
    pub active_backups: HashMap<String, BackupInfo>,
}

impl AgentState {
    /// Create new uninitialized agent state
    pub fn new() -> Self {
        Self {
            initialized: false,
            threshold: None,
            share_count: None,
            active_sessions: HashMap::new(),
            active_backups: HashMap::new(),
        }
    }
    
    /// Initialize the agent
    pub fn initialize(&mut self, threshold: u32, share_count: u32) -> Result<()> {
        if self.initialized {
            return Err(AuraError::already_initialized("Device agent already initialized".to_string()));
        }
        
        self.initialized = true;
        self.threshold = Some(threshold);
        self.share_count = Some(share_count);
        
        Ok(())
    }
    
    /// Check if agent is initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
    
    /// Start a new session
    pub fn start_session(
        &mut self,
        session_id: String,
        session_type: String,
        participants: Vec<DeviceId>,
    ) -> Result<()> {
        if !self.initialized {
            return Err(AuraError::not_initialized(""));
        }
        
        use crate::utils::time::timestamp_secs;
        let session_info = SessionInfo {
            session_type,
            participants,
            started_at: timestamp_secs(),
        };
        
        self.active_sessions.insert(session_id, session_info);
        Ok(())
    }
    
    /// Start a backup operation
    pub fn start_backup(
        &mut self,
        backup_id: String,
        backup_type: String,
        guardians: Vec<String>,
    ) -> Result<()> {
        if !self.initialized {
            return Err(AuraError::not_initialized(""));
        }
        
        use crate::utils::time::timestamp_secs;
        let backup_info = BackupInfo {
            backup_type,
            guardians,
            started_at: timestamp_secs(),
        };
        
        self.active_backups.insert(backup_id, backup_info);
        Ok(())
    }
}

/// Session information
#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_type: String,
    pub participants: Vec<DeviceId>,
    pub started_at: u64,
}

/// Backup operation information
#[derive(Debug, Clone)]
pub struct BackupInfo {
    pub backup_type: String,
    pub guardians: Vec<String>,
    pub started_at: u64,
}

/// No-op handler for testing
pub struct NoOpHandler;

impl AgentHandler for NoOpHandler {
    fn handle(
        &self,
        operation: AgentOperation,
        _context: &AgentContext,
    ) -> Result<serde_json::Value> {
        Ok(serde_json::json!({
            "operation": format!("{:?}", operation),
            "handler": "no_op",
            "success": true
        }))
    }
}