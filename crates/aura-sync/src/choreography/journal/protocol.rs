//! Sync Protocol Implementation
//!
//! Defines the message types and protocol state machine for anti-entropy
//! CRDT synchronization between peers.

use super::{AttestedOp, DeviceId, Hash32, OpLog, OpLogSummary};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

/// Messages exchanged during synchronization protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncMessage {
    /// Request to start synchronization
    SyncRequest {
        /// ID of the requesting device
        requester_id: DeviceId,
        /// Local summary of requester's OpLog
        local_summary: OpLogSummary,
        /// Protocol version for compatibility
        protocol_version: u32,
    },

    /// Response to sync request with peer's summary
    SyncResponse {
        /// ID of the responding device
        responder_id: DeviceId,
        /// Local summary of responder's OpLog
        local_summary: OpLogSummary,
        /// Whether sync request is accepted
        accepted: bool,
        /// Reason for rejection if not accepted
        rejection_reason: Option<String>,
    },

    /// Request for specific operations by CID
    OperationRequest {
        /// ID of the requesting device
        requester_id: DeviceId,
        /// CIDs of requested operations
        requested_cids: BTreeSet<Hash32>,
        /// Maximum number of operations to send in response
        max_operations: usize,
    },

    /// Response with requested operations
    OperationResponse {
        /// ID of the responding device
        responder_id: DeviceId,
        /// Requested operations that were found
        operations: Vec<AttestedOp>,
        /// CIDs that were not found
        missing_cids: BTreeSet<Hash32>,
        /// Whether more operations are available (for pagination)
        has_more: bool,
    },

    /// Signal completion of synchronization
    SyncComplete {
        /// ID of the device completing sync
        sender_id: DeviceId,
        /// Summary statistics of the sync session
        sync_summary: SyncSessionSummary,
    },

    /// Error message during synchronization
    SyncError {
        /// ID of the device reporting the error
        sender_id: DeviceId,
        /// Error that occurred
        error: ProtocolError,
        /// Whether the sync session should be terminated
        terminate_session: bool,
    },
}

impl SyncMessage {
    /// Get the sender ID of this message
    pub fn sender_id(&self) -> DeviceId {
        match self {
            SyncMessage::SyncRequest { requester_id, .. } => *requester_id,
            SyncMessage::SyncResponse { responder_id, .. } => *responder_id,
            SyncMessage::OperationRequest { requester_id, .. } => *requester_id,
            SyncMessage::OperationResponse { responder_id, .. } => *responder_id,
            SyncMessage::SyncComplete { sender_id, .. } => *sender_id,
            SyncMessage::SyncError { sender_id, .. } => *sender_id,
        }
    }

    /// Check if this message indicates an error
    pub fn is_error(&self) -> bool {
        matches!(self, SyncMessage::SyncError { .. })
    }

    /// Check if this message completes the sync
    pub fn is_completion(&self) -> bool {
        matches!(self, SyncMessage::SyncComplete { .. })
    }
}

/// Summary of a completed sync session
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncSessionSummary {
    /// Number of operations sent during sync
    pub operations_sent: usize,
    /// Number of operations received during sync
    pub operations_received: usize,
    /// Duration of sync session in milliseconds
    pub duration_ms: u64,
    /// Whether sync completed successfully
    pub success: bool,
}

/// Errors that can occur during protocol execution
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Error)]
pub enum ProtocolError {
    /// The protocol version is not supported
    #[error("Unsupported protocol version: {version}")]
    UnsupportedProtocolVersion {
        /// The unsupported protocol version
        version: u32,
    },

    /// The message sequence was invalid
    #[error("Invalid message sequence: expected {expected}, got {actual}")]
    InvalidMessageSequence {
        /// The expected message type
        expected: String,
        /// The actual message type received
        actual: String,
    },

    /// Operation validation failed
    #[error("Operation validation failed: {reason}")]
    OperationValidationFailed {
        /// The reason for validation failure
        reason: String,
    },

    /// A resource limit was exceeded
    #[error("Resource limit exceeded: {limit_type}")]
    ResourceLimitExceeded {
        /// The type of resource limit that was exceeded
        limit_type: String,
    },

    /// Timeout waiting for response
    #[error("Timeout waiting for response")]
    ResponseTimeout,

    /// Peer disconnected unexpectedly
    #[error("Peer disconnected unexpectedly")]
    PeerDisconnected,

    /// Invalid operation CID
    #[error("Invalid operation CID: {cid:?}")]
    InvalidOperationCid {
        /// The invalid CID
        cid: Hash32,
    },

    /// Serialization error occurred
    #[error("Serialization error: {reason}")]
    SerializationError {
        /// The reason for serialization failure
        reason: String,
    },
}

/// State machine for sync protocol execution
#[derive(Debug, Clone, PartialEq)]
pub enum SyncState {
    /// Initial state - no sync in progress
    Idle,

    /// Waiting for sync response from peer
    WaitingForResponse {
        /// Peer we're trying to sync with
        peer_id: DeviceId,
        /// Time when request was sent
        request_time: std::time::Instant,
    },

    /// Exchanging operation lists
    ExchangingOperations {
        /// Peer we're syncing with
        peer_id: DeviceId,
        /// Operations we need from peer
        pending_requests: BTreeSet<Hash32>,
        /// Operations peer needs from us
        pending_sends: BTreeSet<Hash32>,
    },

    /// Sync completed successfully
    Completed {
        /// Peer we synced with
        peer_id: DeviceId,
        /// Summary of the sync session
        summary: SyncSessionSummary,
    },

    /// Sync failed with error
    Failed {
        /// Peer sync was attempted with
        peer_id: DeviceId,
        /// Error that caused failure
        error: ProtocolError,
    },
}

impl SyncState {
    /// Check if sync is currently active
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            SyncState::WaitingForResponse { .. } | SyncState::ExchangingOperations { .. }
        )
    }

    /// Check if sync is completed (either successfully or with failure)
    pub fn is_terminal(&self) -> bool {
        matches!(self, SyncState::Completed { .. } | SyncState::Failed { .. })
    }

    /// Get the peer ID if sync involves a peer
    pub fn peer_id(&self) -> Option<DeviceId> {
        match self {
            SyncState::Idle => None,
            SyncState::WaitingForResponse { peer_id, .. } => Some(*peer_id),
            SyncState::ExchangingOperations { peer_id, .. } => Some(*peer_id),
            SyncState::Completed { peer_id, .. } => Some(*peer_id),
            SyncState::Failed { peer_id, .. } => Some(*peer_id),
        }
    }
}

/// Protocol implementation for sync state machine
pub struct SyncProtocol {
    /// Current device ID
    local_device_id: DeviceId,
    /// Current protocol state
    current_state: SyncState,
    /// Protocol version supported
    protocol_version: u32,
    /// Local OpLog for synchronization
    local_oplog: OpLog,
}

impl SyncProtocol {
    /// Current protocol version
    pub const CURRENT_PROTOCOL_VERSION: u32 = 1;

    /// Create a new sync protocol instance
    pub fn new(local_device_id: DeviceId, local_oplog: OpLog) -> Self {
        Self {
            local_device_id,
            current_state: SyncState::Idle,
            protocol_version: Self::CURRENT_PROTOCOL_VERSION,
            local_oplog,
        }
    }

    /// Get current protocol state
    pub fn current_state(&self) -> &SyncState {
        &self.current_state
    }

    /// Update local OpLog
    pub fn update_local_oplog(&mut self, oplog: OpLog) {
        self.local_oplog = oplog;
    }

    /// Start synchronization with a peer
    #[allow(clippy::disallowed_methods)]
    pub fn start_sync(&mut self, peer_id: DeviceId) -> Result<SyncMessage, ProtocolError> {
        if self.current_state.is_active() {
            return Err(ProtocolError::InvalidMessageSequence {
                expected: "Idle".to_string(),
                actual: format!("{:?}", self.current_state),
            });
        }

        let local_summary = self.local_oplog.create_summary();

        self.current_state = SyncState::WaitingForResponse {
            peer_id,
            request_time: std::time::Instant::now(),
        };

        Ok(SyncMessage::SyncRequest {
            requester_id: self.local_device_id,
            local_summary,
            protocol_version: self.protocol_version,
        })
    }

    /// Handle incoming sync message
    pub fn handle_message(
        &mut self,
        message: SyncMessage,
    ) -> Result<Option<SyncMessage>, ProtocolError> {
        match (&self.current_state, &message) {
            // Handle sync request when idle
            (
                SyncState::Idle,
                SyncMessage::SyncRequest {
                    requester_id,
                    local_summary,
                    protocol_version,
                },
            ) => self.handle_sync_request(*requester_id, local_summary, *protocol_version),

            // Handle sync response when waiting
            (
                SyncState::WaitingForResponse { peer_id, .. },
                SyncMessage::SyncResponse {
                    responder_id,
                    local_summary,
                    accepted,
                    rejection_reason,
                },
            ) => {
                if responder_id != peer_id {
                    return Err(ProtocolError::InvalidMessageSequence {
                        expected: format!("Response from {}", peer_id),
                        actual: format!("Response from {}", responder_id),
                    });
                }

                if *accepted {
                    self.handle_sync_accepted(*responder_id, local_summary)
                } else {
                    self.handle_sync_rejected(*responder_id, rejection_reason.clone())
                }
            }

            // Handle operation request during exchange
            (
                SyncState::ExchangingOperations { peer_id, .. },
                SyncMessage::OperationRequest {
                    requester_id,
                    requested_cids,
                    max_operations,
                },
            ) => {
                if requester_id != peer_id {
                    return Err(ProtocolError::InvalidMessageSequence {
                        expected: format!("Request from {}", peer_id),
                        actual: format!("Request from {}", requester_id),
                    });
                }

                self.handle_operation_request(*requester_id, requested_cids, *max_operations)
            }

            // Handle operation response during exchange
            (
                SyncState::ExchangingOperations { peer_id, .. },
                SyncMessage::OperationResponse {
                    responder_id,
                    operations,
                    missing_cids,
                    has_more,
                },
            ) => {
                if responder_id != peer_id {
                    return Err(ProtocolError::InvalidMessageSequence {
                        expected: format!("Response from {}", peer_id),
                        actual: format!("Response from {}", responder_id),
                    });
                }

                self.handle_operation_response(*responder_id, operations, missing_cids, *has_more)
            }

            // Handle sync completion
            (
                _,
                SyncMessage::SyncComplete {
                    sender_id,
                    sync_summary,
                },
            ) => self.handle_sync_complete(*sender_id, sync_summary),

            // Handle sync error
            (
                _,
                SyncMessage::SyncError {
                    sender_id,
                    error,
                    terminate_session,
                },
            ) => self.handle_sync_error(*sender_id, error, *terminate_session),

            // Invalid state transitions
            _ => Err(ProtocolError::InvalidMessageSequence {
                expected: format!("Valid message for state {:?}", self.current_state),
                actual: format!("{:?}", message),
            }),
        }
    }

    /// Reset protocol to idle state
    pub fn reset(&mut self) {
        self.current_state = SyncState::Idle;
    }

    /// Check if protocol is currently synchronizing
    pub fn is_syncing(&self) -> bool {
        self.current_state.is_active()
    }

    // === Private message handlers ===

    fn handle_sync_request(
        &mut self,
        requester_id: DeviceId,
        peer_summary: &OpLogSummary,
        protocol_version: u32,
    ) -> Result<Option<SyncMessage>, ProtocolError> {
        // Check protocol version compatibility
        if protocol_version != self.protocol_version {
            return Err(ProtocolError::UnsupportedProtocolVersion {
                version: protocol_version,
            });
        }

        let local_summary = self.local_oplog.create_summary();

        // Determine what operations need to be exchanged
        let missing_from_local = local_summary.missing_cids(peer_summary);
        let missing_from_peer = peer_summary.missing_cids(&local_summary);

        // Accept sync if there's work to do
        let accepted = !missing_from_local.is_empty() || !missing_from_peer.is_empty();

        if accepted {
            self.current_state = SyncState::ExchangingOperations {
                peer_id: requester_id,
                pending_requests: missing_from_local,
                pending_sends: missing_from_peer,
            };
        }

        Ok(Some(SyncMessage::SyncResponse {
            responder_id: self.local_device_id,
            local_summary,
            accepted,
            rejection_reason: if !accepted {
                Some("No operations to synchronize".to_string())
            } else {
                None
            },
        }))
    }

    fn handle_sync_accepted(
        &mut self,
        peer_id: DeviceId,
        peer_summary: &OpLogSummary,
    ) -> Result<Option<SyncMessage>, ProtocolError> {
        let local_summary = self.local_oplog.create_summary();
        let missing_from_local = local_summary.missing_cids(peer_summary);
        let missing_from_peer = peer_summary.missing_cids(&local_summary);

        self.current_state = SyncState::ExchangingOperations {
            peer_id,
            pending_requests: missing_from_local.clone(),
            pending_sends: missing_from_peer,
        };

        // If we need operations from peer, request them
        if !missing_from_local.is_empty() {
            Ok(Some(SyncMessage::OperationRequest {
                requester_id: self.local_device_id,
                requested_cids: missing_from_local,
                max_operations: 1000, // Configure as needed
            }))
        } else {
            Ok(None)
        }
    }

    fn handle_sync_rejected(
        &mut self,
        peer_id: DeviceId,
        reason: Option<String>,
    ) -> Result<Option<SyncMessage>, ProtocolError> {
        self.current_state = SyncState::Failed {
            peer_id,
            error: ProtocolError::ResourceLimitExceeded {
                limit_type: reason.unwrap_or_else(|| "Unknown rejection".to_string()),
            },
        };
        Ok(None)
    }

    fn handle_operation_request(
        &mut self,
        _peer_id: DeviceId,
        requested_cids: &BTreeSet<Hash32>,
        max_operations: usize,
    ) -> Result<Option<SyncMessage>, ProtocolError> {
        let mut operations = Vec::new();
        let mut missing_cids = BTreeSet::new();
        let mut count = 0;

        for cid in requested_cids {
            if count >= max_operations {
                break;
            }

            if let Some(op) = self.local_oplog.get_operation(cid) {
                operations.push(op.clone());
                count += 1;
            } else {
                missing_cids.insert(*cid);
            }
        }

        let has_more = count < requested_cids.len();

        Ok(Some(SyncMessage::OperationResponse {
            responder_id: self.local_device_id,
            operations,
            missing_cids,
            has_more,
        }))
    }

    fn handle_operation_response(
        &mut self,
        peer_id: DeviceId,
        operations: &[AttestedOp],
        _missing_cids: &BTreeSet<Hash32>,
        _has_more: bool,
    ) -> Result<Option<SyncMessage>, ProtocolError> {
        // Apply received operations to local OpLog
        for op in operations {
            self.local_oplog.add_operation(op.clone());
        }

        // For simplicity, complete the sync after receiving operations
        // In a more sophisticated implementation, this might continue exchanging
        let summary = SyncSessionSummary {
            operations_sent: 0, // Would track this properly
            operations_received: operations.len(),
            duration_ms: 0, // Would track this properly
            success: true,
        };

        self.current_state = SyncState::Completed {
            peer_id,
            summary: summary.clone(),
        };

        Ok(Some(SyncMessage::SyncComplete {
            sender_id: self.local_device_id,
            sync_summary: summary,
        }))
    }

    fn handle_sync_complete(
        &mut self,
        peer_id: DeviceId,
        summary: &SyncSessionSummary,
    ) -> Result<Option<SyncMessage>, ProtocolError> {
        self.current_state = SyncState::Completed {
            peer_id,
            summary: summary.clone(),
        };
        Ok(None)
    }

    fn handle_sync_error(
        &mut self,
        peer_id: DeviceId,
        error: &ProtocolError,
        _terminate_session: bool,
    ) -> Result<Option<SyncMessage>, ProtocolError> {
        self.current_state = SyncState::Failed {
            peer_id,
            error: error.clone(),
        };
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::DeviceId;

    #[test]
    fn test_sync_message_properties() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let summary = OpLogSummary {
            version: 1,
            operation_count: 0,
            cids: BTreeSet::new(),
        };

        let request = SyncMessage::SyncRequest {
            requester_id: device_id,
            local_summary: summary,
            protocol_version: 1,
        };

        assert_eq!(request.sender_id(), device_id);
        assert!(!request.is_error());
        assert!(!request.is_completion());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn test_sync_state_properties() {
        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));

        let idle = SyncState::Idle;
        assert!(!idle.is_active());
        assert!(!idle.is_terminal());
        assert!(idle.peer_id().is_none());

        let waiting = SyncState::WaitingForResponse {
            peer_id,
            request_time: std::time::Instant::now(), // Note: disallowed method, but needed for test state
        };
        assert!(waiting.is_active());
        assert!(!waiting.is_terminal());
        assert_eq!(waiting.peer_id(), Some(peer_id));
    }

    #[test]
    fn test_protocol_creation() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let oplog = OpLog::new();
        let protocol = SyncProtocol::new(device_id, oplog);

        assert!(matches!(protocol.current_state(), SyncState::Idle));
        assert!(!protocol.is_syncing());
    }

    #[test]
    fn test_start_sync() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let oplog = OpLog::new();
        let mut protocol = SyncProtocol::new(device_id, oplog);

        let message = protocol.start_sync(peer_id).unwrap();

        assert!(matches!(message, SyncMessage::SyncRequest { .. }));
        assert!(protocol.is_syncing());
        assert_eq!(protocol.current_state().peer_id(), Some(peer_id));
    }

    #[test]
    fn test_protocol_reset() {
        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let peer_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        let oplog = OpLog::new();
        let mut protocol = SyncProtocol::new(device_id, oplog);

        // Start sync then reset
        protocol.start_sync(peer_id).unwrap();
        assert!(protocol.is_syncing());

        protocol.reset();
        assert!(!protocol.is_syncing());
        assert!(matches!(protocol.current_state(), SyncState::Idle));
    }
}