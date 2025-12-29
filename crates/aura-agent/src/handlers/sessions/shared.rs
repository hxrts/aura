//! Shared Session Types and Utilities
//!
//! Common types and utilities used across all session management modules.

use aura_core::effects::SessionType;
use aura_core::identifiers::{AccountId, DeviceId};
use aura_protocol::effects::ChoreographicRole;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Session handle for managing active sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct SessionHandle {
    /// Session ID
    pub session_id: String,
    /// Session type
    pub session_type: SessionType,
    /// Participating devices
    pub participants: Vec<DeviceId>,
    /// This device's role
    pub my_role: ChoreographicRole,
    /// Session epoch
    pub epoch: u64,
    /// Session start time
    pub start_time: u64,
    /// Session metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Session statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct SessionStats {
    /// Total active sessions
    pub active_sessions: u32,
    /// Sessions by type
    pub sessions_by_type: HashMap<String, u32>,
    /// Total participants across all sessions
    pub total_participants: u32,
    /// Average session duration
    pub average_duration: f64,
    /// Last cleanup time
    pub last_cleanup: u64,
}

/// Device information (authority-centric)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct DeviceInfo {
    /// Authority identifier (public identity)
    pub authority_id: aura_core::identifiers::AuthorityId,
    /// Account this authority belongs to
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

/// Roles in session management choreography
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[allow(dead_code)] // Part of future session management API
pub enum SessionManagementRole {
    /// Device initiating session management operation
    Initiator(DeviceId),
    /// Device participating in session
    Participant(DeviceId, u32), // Device ID and participant index
    /// Device coordinating session lifecycle
    Coordinator(DeviceId),
    /// Device managing session metadata
    Manager(DeviceId),
}

impl SessionManagementRole {
    /// Get the device ID for this role
    #[allow(dead_code)] // Part of future session management API
    pub fn device_id(&self) -> DeviceId {
        match self {
            SessionManagementRole::Initiator(id) => *id,
            SessionManagementRole::Participant(id, _) => *id,
            SessionManagementRole::Coordinator(id) => *id,
            SessionManagementRole::Manager(id) => *id,
        }
    }

    /// Get role name for choreography framework
    #[allow(dead_code)] // Part of future session management API
    pub fn name(&self) -> String {
        match self {
            SessionManagementRole::Initiator(id) => format!("Initiator_{}", id.0.simple()),
            SessionManagementRole::Participant(id, idx) => {
                format!("Participant_{}_{}", id.0.simple(), idx)
            }
            SessionManagementRole::Coordinator(id) => format!("Coordinator_{}", id.0.simple()),
            SessionManagementRole::Manager(id) => format!("Manager_{}", id.0.simple()),
        }
    }

    /// Get participant index if this is a participant role
    #[allow(dead_code)] // Part of future session management API
    pub fn participant_index(&self) -> Option<u32> {
        match self {
            SessionManagementRole::Participant(_, idx) => Some(*idx),
            _ => None,
        }
    }
}

/// Session management message types

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct SessionCreateRequest {
    pub session_type: SessionType,
    pub participants: Vec<DeviceId>,
    pub initiator: DeviceId,
    pub account_id: AccountId,
    pub session_id: String,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct SessionInvitation {
    pub session_id: String,
    pub session_type: SessionType,
    pub initiator: DeviceId,
    pub role: ChoreographicRole,
    pub start_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct SessionResponse {
    pub session_id: String,
    pub participant: DeviceId,
    pub accepted: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct SessionEstablished {
    pub session_id: String,
    pub participants: Vec<DeviceId>,
    pub start_time: u64,
    pub my_role: ChoreographicRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct SessionFailed {
    pub session_id: String,
    pub reason: String,
    pub failed_participants: Vec<DeviceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct MetadataUpdate {
    pub session_id: String,
    pub metadata_changes: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct MetadataSync {
    pub session_id: String,
    pub updated_metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct ParticipantChange {
    pub session_id: String,
    pub operation: String, // "add" or "remove"
    pub target_participant: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct ParticipantUpdate {
    pub session_id: String,
    pub updated_participants: Vec<DeviceId>,
    pub operation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct SessionEnd {
    pub session_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)] // Part of future session management API
pub struct SessionTerminated {
    pub session_id: String,
    pub end_time: u64,
    pub reason: String,
}

/// Helper functions
///
/// Get session type suffix for session IDs
#[allow(dead_code)] // Part of future session management API
pub fn session_type_suffix(session_type: &SessionType) -> &'static str {
    match session_type {
        SessionType::ThresholdOperation => "threshold",
        SessionType::Recovery => "recovery",
        SessionType::KeyRotation => "rotation",
        SessionType::Coordination => "coord",
        SessionType::Backup => "backup",
        SessionType::Invitation => "invitation",
        SessionType::Rendezvous => "rendezvous",
        SessionType::Sync => "sync",
        SessionType::Custom(_) => "custom",
    }
}
