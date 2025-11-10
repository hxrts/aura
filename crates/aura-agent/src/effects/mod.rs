//! Agent-Specific Effect Trait Re-exports
//!
//! This module re-exports agent-specific effect traits from aura-protocol.
//! Agent effect definitions have been moved to aura-protocol as per the
//! unified architecture.

// Re-export all agent-specific effect traits from aura-protocol
pub use aura_protocol::effects::{
    AgentEffects, AgentHealthStatus, AuthMethod, AuthenticationEffects, AuthenticationResult,
    BiometricType, ConfigValidationError, ConfigurationEffects, CredentialBackup, DeviceConfig,
    DeviceInfo, DeviceStorageEffects, HealthStatus, SessionHandle, SessionInfo,
    SessionManagementEffects, SessionMessage, SessionRole, SessionStatus, SessionType,
};

// Re-export core effect traits from aura-protocol for convenience
pub use aura_protocol::effects::{
    ChoreographicEffects, ConsoleEffects, CryptoEffects, JournalEffects, LedgerEffects,
    NetworkEffects, RandomEffects, StorageEffects, TimeEffects,
};

// Re-export unified effect system from the effects module
pub use aura_protocol::effects::AuraEffectSystem;

// Agent-specific session types
use aura_core::{
    identifiers::{AccountId, DeviceId},
    AuraResult,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Session data for agent operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionData {
    pub session_id: String,
    pub account_id: AccountId,
    pub device_id: DeviceId,
    pub epoch: u64,
    pub start_time: u64,
    pub participants: Vec<ChoreographicRole>,
    pub my_role: ChoreographicRole,
    pub session_type: SessionType,
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Session update data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionUpdate {
    UpdateMetadata(HashMap<String, serde_json::Value>),
    AddParticipant(ChoreographicRole),
    RemoveParticipant(DeviceId),
}

// Re-export for convenience
pub use aura_protocol::effects::ChoreographicRole;
