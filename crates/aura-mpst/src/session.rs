//! Session type system types
//!
//! Types for session type signatures used by handler interfaces.
//! Moved from aura-core as these represent session type system concerns.

use serde::{Deserialize, Serialize};

/// Local session type for handler interfaces
///
/// Represents the type signature of session protocols used by handlers.
/// This is a placeholder type for compatibility with handler implementations.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalSessionType {
    /// Protocol name
    pub protocol: String,
    /// Session parameters
    pub params: Vec<u8>,
}

impl LocalSessionType {
    /// Create a new local session type
    pub fn new(protocol: String, params: Vec<u8>) -> Self {
        Self { protocol, params }
    }

    /// Get the protocol name
    pub fn protocol(&self) -> &str {
        &self.protocol
    }

    /// Get the session parameters
    pub fn params(&self) -> &[u8] {
        &self.params
    }
}
