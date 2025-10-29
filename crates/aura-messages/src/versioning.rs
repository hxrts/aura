//! Message versioning for protocol evolution
//!
//! This module provides version negotiation and compatibility checking
//! for protocol messages to support seamless upgrades.

use serde::{Deserialize, Serialize};

/// Version information for a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageVersion {
    /// Major version (breaking changes)
    pub major: u16,
    /// Minor version (backward-compatible additions)
    pub minor: u16,
    /// Patch version (bug fixes)
    pub patch: u16,
}

impl MessageVersion {
    /// Current message version
    pub const CURRENT: MessageVersion = MessageVersion {
        major: 1,
        minor: 0,
        patch: 0,
    };

    /// Check if this version is compatible with another version
    pub fn is_compatible_with(&self, other: &MessageVersion) -> bool {
        // Same major version, this minor >= other minor
        self.major == other.major && self.minor >= other.minor
    }

    /// Create a new version
    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl Default for MessageVersion {
    fn default() -> Self {
        Self::CURRENT
    }
}

/// Base trait for all versioned messages
pub trait VersionedMessage {
    /// Get the version of this message
    fn version(&self) -> &MessageVersion;

    /// Check if this message is compatible with a given version
    fn is_compatible_with(&self, version: &MessageVersion) -> bool {
        self.version().is_compatible_with(version)
    }
}
