//! Shared Session Types and Utilities
//!
//! Common types and utilities used across all session management modules.

use aura_core::effects::SessionType;
use aura_core::types::identifiers::{AccountId, DeviceId, SessionId};
use aura_protocol::effects::ChoreographicRole;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Session handle for managing active sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHandle {
    /// Session ID
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
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
pub struct DeviceInfo {
    /// Authority identifier (public identity)
    pub authority_id: aura_core::types::identifiers::AuthorityId,
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
    pub fn device_id(&self) -> DeviceId {
        match self {
            SessionManagementRole::Initiator(id) => *id,
            SessionManagementRole::Participant(id, _) => *id,
            SessionManagementRole::Coordinator(id) => *id,
            SessionManagementRole::Manager(id) => *id,
        }
    }

    /// Get role name for choreography framework
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
    pub fn participant_index(&self) -> Option<u32> {
        match self {
            SessionManagementRole::Participant(_, idx) => Some(*idx),
            _ => None,
        }
    }
}

/// Session management message types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCreateRequest {
    pub session_type: SessionType,
    pub participants: Vec<DeviceId>,
    pub initiator: DeviceId,
    pub account_id: AccountId,
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInvitation {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub session_type: SessionType,
    pub initiator: DeviceId,
    pub role: ChoreographicRole,
    pub start_time: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionResponse {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub participant: DeviceId,
    pub accepted: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEstablished {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub participants: Vec<DeviceId>,
    pub start_time: u64,
    pub my_role: ChoreographicRole,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionFailed {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub reason: String,
    pub failed_participants: Vec<DeviceId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataUpdate {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub metadata_changes: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataSync {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub updated_metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantChange {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub operation: ParticipantOperation,
    pub target_participant: DeviceId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParticipantUpdate {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub updated_participants: Vec<DeviceId>,
    pub operation: ParticipantOperation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParticipantOperation {
    Add,
    Remove,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEnd {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTerminated {
    #[serde(with = "session_id_serde")]
    pub session_id: SessionId,
    pub end_time: u64,
    pub reason: String,
}

/// Helper functions
///
/// Get session type suffix for session IDs
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

mod session_id_serde {
    use aura_core::types::identifiers::SessionId;
    use serde::{de::Error, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(session_id: &SessionId, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&session_id.to_string())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SessionId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value
            .parse::<SessionId>()
            .map_err(|e| D::Error::custom(format!("invalid session id `{value}`: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::{ParticipantChange, ParticipantOperation, SessionHandle};
    use aura_core::effects::SessionType;
    use aura_core::types::identifiers::DeviceId;
    use aura_protocol::effects::{ChoreographicRole, RoleIndex};
    use std::collections::HashMap;

    #[test]
    fn participant_operation_serializes_as_legacy_strings() {
        let change = ParticipantChange {
            session_id: "session-123e4567-e89b-12d3-a456-426614174000"
                .parse()
                .expect("parse session id"),
            operation: ParticipantOperation::Add,
            target_participant: DeviceId::new_from_entropy([1u8; 32]),
        };

        let value = serde_json::to_value(&change).expect("serialize participant change");
        assert_eq!(value["operation"], "add");
    }

    #[test]
    fn participant_operation_rejects_invalid_strings() {
        let payload = serde_json::json!({
            "session_id": "session-123e4567-e89b-12d3-a456-426614174000",
            "operation": "append",
            "target_participant": DeviceId::new_from_entropy([2u8; 32]),
        });

        let decoded = serde_json::from_value::<ParticipantChange>(payload);
        assert!(decoded.is_err(), "invalid operation should fail to decode");
    }

    #[test]
    fn session_handle_deserializes_prefixed_session_id() {
        let device_id = DeviceId::new_from_entropy([3u8; 32]);
        let handle = SessionHandle {
            session_id: "123e4567-e89b-12d3-a456-426614174000"
                .parse()
                .expect("parse session id"),
            session_type: SessionType::Coordination,
            participants: vec![device_id],
            my_role: ChoreographicRole::new(
                device_id,
                aura_core::AuthorityId::new_from_entropy([0u8; 32]),
                RoleIndex::new(1).expect("role index"),
            ),
            epoch: 0,
            start_time: 0,
            metadata: HashMap::new(),
        };
        let mut payload = serde_json::to_value(handle).expect("serialize handle");
        payload["session_id"] =
            serde_json::Value::String("session-123e4567-e89b-12d3-a456-426614174000".to_string());

        let decoded = serde_json::from_value::<SessionHandle>(payload).expect("decode handle");
        assert_eq!(
            decoded.session_id.to_string(),
            "session-123e4567-e89b-12d3-a456-426614174000"
        );
    }

    #[test]
    fn session_handle_serializes_session_id_as_string() {
        let device_id = DeviceId::new_from_entropy([4u8; 32]);
        let handle = SessionHandle {
            session_id: "session-123e4567-e89b-12d3-a456-426614174000"
                .parse()
                .expect("parse session id"),
            session_type: SessionType::Coordination,
            participants: vec![device_id],
            my_role: ChoreographicRole::new(
                device_id,
                aura_core::AuthorityId::new_from_entropy([0u8; 32]),
                RoleIndex::new(1).expect("role index"),
            ),
            epoch: 0,
            start_time: 0,
            metadata: HashMap::new(),
        };

        let value = serde_json::to_value(&handle).expect("serialize handle");
        assert_eq!(
            value["session_id"],
            serde_json::Value::String("session-123e4567-e89b-12d3-a456-426614174000".to_string())
        );
    }
}
