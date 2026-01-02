//! Session type system types
//!
//! Types for session type signatures used by handler interfaces.
//! Moved from aura-core as these represent session type system concerns.

use crate::ids::SessionTypeId;
use serde::{Deserialize, Serialize};

/// Local session type for handler interfaces
///
/// Represents the type signature of session protocols used by handlers.
/// Maintains compatibility with rumpsteak projections while keeping Aura-specific
/// metadata (protocol name + serialized params) available for runtime selection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalSessionType {
    /// Protocol name
    pub protocol: SessionTypeId,
    /// Session parameters
    pub params: Vec<u8>,
}

impl LocalSessionType {
    /// Create a new local session type
    pub fn new(protocol: impl Into<SessionTypeId>, params: Vec<u8>) -> Self {
        Self {
            protocol: protocol.into(),
            params,
        }
    }

    /// Get the protocol name
    pub fn protocol(&self) -> &str {
        self.protocol.as_str()
    }

    /// Get the protocol identifier
    pub fn protocol_id(&self) -> &SessionTypeId {
        &self.protocol
    }

    /// Get the session parameters
    pub fn params(&self) -> &[u8] {
        &self.params
    }
}
