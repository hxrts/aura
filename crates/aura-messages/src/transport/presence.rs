//! Presence protocol messages
//!
//! Messages for device presence announcements and session management.

use crate::serialization::WireSerializable;
use aura_types::{DeviceId, SessionId};
use serde::{Deserialize, Serialize};

/// Presence message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PresenceMessage {
    /// Announce device presence
    Announce(PresenceAnnouncement),
    /// Update presence status
    Update(PresenceUpdate),
    /// Presence heartbeat
    Heartbeat(PresenceHeartbeat),
    /// Leave/disconnect notification
    Leave(PresenceLeave),
}

/// Device presence announcement
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceAnnouncement {
    pub device_id: DeviceId,
    pub session_id: Option<SessionId>,
    pub capabilities: Vec<String>,
    pub endpoint_info: EndpointInfo,
    pub presence_ticket: Vec<u8>,
    pub announcement_signature: Vec<u8>,
}

/// Endpoint information for connectivity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointInfo {
    pub transport_type: String,
    pub address: String,
    pub port: Option<u16>,
    pub metadata: std::collections::BTreeMap<String, String>,
}

/// Presence status update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceUpdate {
    pub device_id: DeviceId,
    pub new_status: PresenceStatus,
    pub session_id: Option<SessionId>,
    pub update_reason: Option<String>,
}

/// Device presence status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PresenceStatus {
    Online,
    Away,
    Busy,
    Offline,
    InProtocol { protocol_type: String },
}

/// Presence heartbeat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceHeartbeat {
    pub device_id: DeviceId,
    pub session_id: Option<SessionId>,
    pub sequence_number: u64,
    pub status: PresenceStatus,
}

/// Presence leave notification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceLeave {
    pub device_id: DeviceId,
    pub session_id: Option<SessionId>,
    pub reason: LeaveReason,
    pub graceful: bool,
}

/// Reasons for leaving presence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LeaveReason {
    UserInitiated,
    NetworkDisconnection,
    ProtocolComplete,
    Error { message: String },
    Timeout,
}

/// Presence ticket for verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PresenceTicket {
    pub device_id: DeviceId,
    pub session_epoch: u64,
    pub ticket_data: Vec<u8>,
    pub expires_at: u64,
    pub issued_at: u64,
    pub ticket_digest: [u8; 32],
}

// Implement wire serialization for all presence message types
impl WireSerializable for PresenceMessage {}
impl WireSerializable for PresenceAnnouncement {}
impl WireSerializable for EndpointInfo {}
impl WireSerializable for PresenceUpdate {}
impl WireSerializable for PresenceHeartbeat {}
impl WireSerializable for PresenceLeave {}
impl WireSerializable for PresenceTicket {}
