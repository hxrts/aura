//! Agent-specific middleware system
//!
//! This module provides middleware for agent operations including:
//! - Identity management (DKD protocols)
//! - Session coordination (state management)
//! - Policy enforcement (access control)
//! - Device management (secure storage)
//! - Backup coordination (recovery protocols)

pub mod stack;
pub mod handler;
pub mod identity_management;
pub mod session_coordination;
pub mod policy_enforcement;
pub mod device_management;
pub mod backup_coordination;

pub use stack::*;
pub use handler::*;
pub use identity_management::*;
pub use session_coordination::*;
pub use policy_enforcement::*;
pub use device_management::*;
pub use backup_coordination::*;

use crate::error::Result;
use aura_types::{DeviceId, AccountId};

/// Context for agent middleware operations
#[derive(Debug, Clone)]
pub struct AgentContext {
    /// Account being operated on
    pub account_id: AccountId,
    
    /// Device performing the operation
    pub device_id: DeviceId,
    
    /// Operation being performed
    pub operation_type: String,
    
    /// Request timestamp
    pub timestamp: u64,
    
    /// Session identifier (if in session)
    pub session_id: Option<String>,
    
    /// Additional metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl AgentContext {
    /// Create a new agent context
    pub fn new(account_id: AccountId, device_id: DeviceId, operation_type: String) -> Self {
        use crate::utils::time::timestamp_secs;
        Self {
            account_id,
            device_id,
            operation_type,
            timestamp: timestamp_secs(),
            session_id: None,
            metadata: std::collections::HashMap::new(),
        }
    }
    
    /// Add session context
    pub fn with_session(mut self, session_id: String) -> Self {
        self.session_id = Some(session_id);
        self
    }
    
    /// Add metadata to the context
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Agent operation types
#[derive(Debug, Clone)]
pub enum AgentOperation {
    /// Initialize agent (bootstrap)
    Initialize {
        threshold: u32,
        share_count: u32,
    },
    
    /// Derive identity using DKD
    DeriveIdentity {
        app_id: String,
        context: String,
    },
    
    /// Start session coordination
    StartSession {
        session_type: String,
        participants: Vec<DeviceId>,
    },
    
    /// Store encrypted data
    StoreData {
        data: Vec<u8>,
        capabilities: Vec<String>,
    },
    
    /// Retrieve encrypted data
    RetrieveData {
        data_id: String,
        required_capability: String,
    },
    
    /// Initiate backup/recovery
    InitiateBackup {
        backup_type: String,
        guardians: Vec<String>,
    },
    
    /// Check agent status
    GetStatus,
}

/// Trait for agent middleware components
pub trait AgentMiddleware: Send + Sync {
    /// Process an agent operation
    fn process(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
        next: &dyn AgentHandler,
    ) -> Result<serde_json::Value>;
    
    /// Get middleware name for debugging
    fn name(&self) -> &str;
}

/// Trait for handling agent operations
pub trait AgentHandler: Send + Sync {
    /// Handle an agent operation
    fn handle(
        &self,
        operation: AgentOperation,
        context: &AgentContext,
    ) -> Result<serde_json::Value>;
}