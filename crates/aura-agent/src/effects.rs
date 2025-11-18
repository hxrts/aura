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

// Re-export individual effect traits
pub use aura_protocol::effect_traits::{
    LedgerEffects,
};

// Re-export orchestration types
pub use aura_protocol::orchestration::{
    ChoreographicEffects,
};

// Re-export agent-specific effects from aura-protocol (will be moved to aura-core in future)
pub use aura_protocol::effects::{
    AgentEffects, AgentHealthStatus, AuthMethod, AuthenticationEffects, AuthenticationResult,
    BiometricType, ConfigValidationError, ConfigurationEffects,
    CredentialBackup, DeviceConfig, DeviceInfo, DeviceStorageEffects, HealthStatus,
    SessionHandle, SessionInfo, SessionManagementEffects, SessionMessage, SessionRole,
    SessionStatus,
};

// Re-export effect system coordinator from aura-agent runtime (Layer 6)
// NOTE: AuraEffectSystem has moved from aura-protocol to aura-agent because:
//   - It's runtime composition infrastructure (Layer 6)
//   - Coordinates multiple effect handlers
//   - Maintains stateful execution context
pub use crate::runtime::AuraEffectSystem;

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
// Import ChoreographicRole from orchestration module
pub use aura_protocol::orchestration::ChoreographicRole;
