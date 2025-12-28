//! Anti-entropy wire format helpers.

use super::effects::SyncError;
use aura_core::tree::AttestedOp;
use serde::{Deserialize, Serialize};

pub const SYNC_WIRE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncWirePayload {
    Op(AttestedOp),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncWireMessage {
    pub schema_version: u16,
    pub payload: SyncWirePayload,
}

impl SyncWireMessage {
    pub fn op(op: AttestedOp) -> Self {
        Self {
            schema_version: SYNC_WIRE_SCHEMA_VERSION,
            payload: SyncWirePayload::Op(op),
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
