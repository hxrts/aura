//! Consistent serialization for all message types
//!
//! This module provides standardized serialization/deserialization
//! functions for all wire format messages.

use crate::{MessageError, MessageResult};
use serde::{Deserialize, Serialize};

/// Serialize a message to CBOR format (preferred for wire protocol)
pub fn serialize_cbor<T: Serialize>(msg: &T) -> MessageResult<Vec<u8>> {
    serde_cbor::to_vec(msg).map_err(|e| MessageError::SerializationFailed(e.to_string()))
}

/// Deserialize a message from CBOR format
pub fn deserialize_cbor<T: for<'de> Deserialize<'de>>(data: &[u8]) -> MessageResult<T> {
    serde_cbor::from_slice(data).map_err(|e| MessageError::DeserializationFailed(e.to_string()))
}

/// Serialize a message to JSON format (for debugging/development)
pub fn serialize_json<T: Serialize>(msg: &T) -> MessageResult<String> {
    serde_json::to_string(msg).map_err(|e| MessageError::SerializationFailed(e.to_string()))
}

/// Deserialize a message from JSON format
pub fn deserialize_json<T: for<'de> Deserialize<'de>>(data: &str) -> MessageResult<T> {
    serde_json::from_str(data).map_err(|e| MessageError::DeserializationFailed(e.to_string()))
}

/// Serialize a message to compact binary format
pub fn serialize_bincode<T: Serialize>(msg: &T) -> MessageResult<Vec<u8>> {
    bincode::serialize(msg).map_err(|e| MessageError::SerializationFailed(e.to_string()))
}

/// Deserialize a message from compact binary format
pub fn deserialize_bincode<T: for<'de> Deserialize<'de>>(data: &[u8]) -> MessageResult<T> {
    bincode::deserialize(data).map_err(|e| MessageError::DeserializationFailed(e.to_string()))
}

/// Trait for types that can be serialized consistently
pub trait WireSerializable: Serialize + for<'de> Deserialize<'de> {
    /// Serialize to wire format (CBOR)
    fn to_wire(&self) -> MessageResult<Vec<u8>> {
        serialize_cbor(self)
    }

    /// Deserialize from wire format (CBOR)
    fn from_wire(data: &[u8]) -> MessageResult<Self> {
        deserialize_cbor(data)
    }

    /// Serialize to JSON for debugging
    fn to_json(&self) -> MessageResult<String> {
        serialize_json(self)
    }

    /// Deserialize from JSON
    fn from_json(data: &str) -> MessageResult<Self> {
        deserialize_json(data)
    }
}
