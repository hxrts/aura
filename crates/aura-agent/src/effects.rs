//! Agent-Specific Effect Trait Re-exports
//!
//! This module provides a convenient public API for effect traits and types used by agents.
//!
//! ## Dependency Architecture
//!
//! - **Effect Trait Definitions**: Come from `aura-core` (interface layer)
//! - **Agent-Specific Effects**: Defined in `aura-protocol` (orchestration layer)
//! - **Effect System Coordinator**: `AuraEffectSystem` from `aura-protocol` (coordinates handlers)
//!
//! This follows the layering: Interface → Orchestration → Runtime Composition

// Re-export core effect trait definitions from aura-core (interface layer)
pub use aura_core::effects::{
    ConsoleEffects, CryptoEffects, JournalEffects, NetworkEffects, RandomEffects, StorageEffects,
    TimeEffects,
};

// Re-export agent-specific effects from aura-protocol (orchestration layer)
pub use aura_protocol::effects::{
    AgentEffects, AgentHealthStatus, AuthMethod, AuthenticationEffects, AuthenticationResult,
    BiometricType, ChoreographicEffects, ConfigValidationError, ConfigurationEffects,
    CredentialBackup, DeviceConfig, DeviceInfo, DeviceStorageEffects, HealthStatus, LedgerEffects,
    SessionHandle, SessionInfo, SessionManagementEffects, SessionMessage, SessionRole,
    SessionStatus, SessionType,
};

// Re-export effect system coordinator from aura-protocol (multi-handler composition)
// NOTE: AuraEffectSystem is correctly in aura-protocol because it:
//   - Coordinates multiple effect handlers (composition)
//   - Maintains stateful execution context
//   - Provides unified handler orchestration
pub use aura_protocol::effects::AuraEffectSystem;

// Agent-specific session types
use aura_core::identifiers::{AccountId, DeviceId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
