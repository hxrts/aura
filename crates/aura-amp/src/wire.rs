//! AMP wire format definitions.

use aura_core::effects::amp::AmpHeader;
use aura_core::AuraError;
use serde::{Deserialize, Serialize};

pub const AMP_WIRE_SCHEMA_VERSION: u16 = 1;

/// Simple wire format for AMP messages (header + opaque payload).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmpMessage {
    pub schema_version: u16,
    pub header: AmpHeader,
    pub payload: Vec<u8>,
}

impl AmpMessage {
    pub fn new(header: AmpHeader, payload: Vec<u8>) -> Self {
        Self {
            schema_version: AMP_WIRE_SCHEMA_VERSION,
            header,
            payload,
        }
    }
}

pub fn serialize_message(msg: &AmpMessage) -> Result<Vec<u8>, AuraError> {
    aura_core::util::serialization::to_vec(msg).map_err(|e| AuraError::serialization(e.to_string()))
}

pub fn deserialize_message(bytes: &[u8]) -> Result<AmpMessage, AuraError> {
    aura_core::util::serialization::from_slice(bytes).map_err(|e| AuraError::serialization(e.to_string()))
}
