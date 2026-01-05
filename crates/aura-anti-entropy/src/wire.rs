//! Anti-entropy wire format helpers.

use super::effects::SyncError;
use aura_core::identifiers::AuthorityId;
use aura_core::time::{OrderTime, PhysicalTime};
use aura_core::tree::AttestedOp;
use serde::{Deserialize, Serialize};

pub const SYNC_WIRE_SCHEMA_VERSION: u16 = 2;

/// Acknowledgment for a received fact.
///
/// Sent by the receiver to confirm fact receipt when `ack_requested` is true.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FactAck {
    /// Identifier of the fact being acknowledged (OrderTime)
    pub fact_id: OrderTime,
    /// Authority sending the acknowledgment
    pub acknowledger: AuthorityId,
    /// Timestamp when the acknowledgment was created
    pub acked_at: PhysicalTime,
}

impl FactAck {
    /// Create a new fact acknowledgment.
    pub fn new(fact_id: OrderTime, acknowledger: AuthorityId, acked_at: PhysicalTime) -> Self {
        Self {
            fact_id,
            acknowledger,
            acked_at,
        }
    }
}

/// Wrapper for transmitting operations with acknowledgment request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpWithAckRequest {
    /// The attested operation
    pub op: AttestedOp,
    /// Whether the sender requests an acknowledgment
    pub ack_requested: bool,
}

impl OpWithAckRequest {
    /// Create a new operation wrapper without ack request.
    pub fn new(op: AttestedOp) -> Self {
        Self {
            op,
            ack_requested: false,
        }
    }

    /// Create a new operation wrapper with ack request.
    pub fn with_ack_request(op: AttestedOp) -> Self {
        Self {
            op,
            ack_requested: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncWirePayload {
    /// Legacy operation without ack tracking (v1 compat)
    Op(AttestedOp),
    /// Operation with optional ack request (v2)
    OpWithAck(OpWithAckRequest),
    /// Acknowledgment for a received fact (v2)
    Ack(FactAck),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncWireMessage {
    pub schema_version: u16,
    pub payload: SyncWirePayload,
}

impl SyncWireMessage {
    /// Create a legacy operation message (v1 compat).
    pub fn op(op: AttestedOp) -> Self {
        Self {
            schema_version: SYNC_WIRE_SCHEMA_VERSION,
            payload: SyncWirePayload::Op(op),
        }
    }

    /// Create an operation message with ack request.
    pub fn op_with_ack_request(op: AttestedOp) -> Self {
        Self {
            schema_version: SYNC_WIRE_SCHEMA_VERSION,
            payload: SyncWirePayload::OpWithAck(OpWithAckRequest::with_ack_request(op)),
        }
    }

    /// Create an operation message without ack request.
    pub fn op_with_ack(op: AttestedOp, ack_requested: bool) -> Self {
        Self {
            schema_version: SYNC_WIRE_SCHEMA_VERSION,
            payload: SyncWirePayload::OpWithAck(OpWithAckRequest { op, ack_requested }),
        }
    }

    /// Create an acknowledgment message.
    pub fn ack(ack: FactAck) -> Self {
        Self {
            schema_version: SYNC_WIRE_SCHEMA_VERSION,
            payload: SyncWirePayload::Ack(ack),
        }
    }

    /// Check if this message is an ack request.
    pub fn is_ack_requested(&self) -> bool {
        match &self.payload {
            SyncWirePayload::Op(_) => false,
            SyncWirePayload::OpWithAck(wrap) => wrap.ack_requested,
            SyncWirePayload::Ack(_) => false,
        }
    }

    /// Extract the operation if this is an Op or OpWithAck message.
    pub fn operation(&self) -> Option<&AttestedOp> {
        match &self.payload {
            SyncWirePayload::Op(op) => Some(op),
            SyncWirePayload::OpWithAck(wrap) => Some(&wrap.op),
            SyncWirePayload::Ack(_) => None,
        }
    }

    /// Extract the ack if this is an Ack message.
    pub fn get_ack(&self) -> Option<&FactAck> {
        match &self.payload {
            SyncWirePayload::Ack(a) => Some(a),
            _ => None,
        }
    }
}

pub fn serialize_message(msg: &SyncWireMessage) -> Result<Vec<u8>, SyncError> {
    aura_core::util::serialization::to_vec(msg).map_err(|e| SyncError::NetworkError(e.to_string()))
}

pub fn deserialize_message(bytes: &[u8]) -> Result<SyncWireMessage, SyncError> {
    aura_core::util::serialization::from_slice(bytes)
        .map_err(|e| SyncError::NetworkError(e.to_string()))
}
