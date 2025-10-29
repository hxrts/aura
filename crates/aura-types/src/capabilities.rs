//! Capability system types
//!
//! This module provides types for the capability-based access control system
//! used throughout the Aura platform.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Capability identifier for access control
///
/// Unique identifier for capabilities within the capability system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CapabilityId(pub [u8; 32]);

impl CapabilityId {
    /// Create a new capability ID
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create from a byte slice
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, crate::TypeError> {
        if bytes.len() == 32 {
            let mut array = [0u8; 32];
            array.copy_from_slice(bytes);
            Ok(Self(array))
        } else {
            Err(crate::TypeError::InvalidIdentifier(format!(
                "CapabilityId must be exactly 32 bytes, got {}",
                bytes.len()
            )))
        }
    }

    /// Create from blake3 hash
    pub fn from_blake3_hash(hash: &blake3::Hash) -> Self {
        Self(*hash.as_bytes())
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Get as hex string (alias for to_hex)
    pub fn as_hex(&self) -> String {
        self.to_hex()
    }

    /// Create from hex string
    pub fn from_hex(hex_str: &str) -> Result<Self, hex::FromHexError> {
        let bytes = hex::decode(hex_str)?;
        if bytes.len() == 32 {
            let mut array = [0u8; 32];
            array.copy_from_slice(&bytes);
            Ok(Self(array))
        } else {
            Err(hex::FromHexError::InvalidStringLength)
        }
    }

    /// Generate a random capability ID
    #[allow(clippy::disallowed_methods)]
    pub fn random() -> Self {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(blake3::hash(uuid::Uuid::new_v4().as_bytes()).as_bytes());
        Self(bytes)
    }

    /// Generate a deterministic capability ID from a parent chain
    ///
    /// Creates a capability ID based on a parent capability, subject identifier,
    /// and scope. This allows for deterministic derivation of child capabilities.
    pub fn from_chain(
        parent_id: Option<&CapabilityId>,
        subject_id: &[u8],
        scope_data: &[u8],
    ) -> Self {
        let mut hasher = blake3::Hasher::new();

        if let Some(parent) = parent_id {
            hasher.update(&parent.0);
        }

        hasher.update(subject_id);
        hasher.update(scope_data);

        Self(hasher.finalize().into())
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "capability-{}", self.to_hex())
    }
}

impl From<[u8; 32]> for CapabilityId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<blake3::Hash> for CapabilityId {
    fn from(hash: blake3::Hash) -> Self {
        Self::from_blake3_hash(&hash)
    }
}

impl From<CapabilityId> for [u8; 32] {
    fn from(capability_id: CapabilityId) -> Self {
        capability_id.0
    }
}

/// Capability scope enumeration
///
/// Defines the scope of access that a capability grants.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityScope {
    /// Read-only access
    Read,
    /// Write access (includes read)
    Write,
    /// Execute access for operations
    Execute,
    /// Administrative access (full control)
    Admin,
    /// Custom scope with specific permissions
    Custom(String),
}

impl fmt::Display for CapabilityScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilityScope::Read => write!(f, "read"),
            CapabilityScope::Write => write!(f, "write"),
            CapabilityScope::Execute => write!(f, "execute"),
            CapabilityScope::Admin => write!(f, "admin"),
            CapabilityScope::Custom(custom) => write!(f, "custom:{}", custom),
        }
    }
}

impl CapabilityScope {
    /// Check if this scope includes read access
    pub fn includes_read(&self) -> bool {
        matches!(
            self,
            CapabilityScope::Read | CapabilityScope::Write | CapabilityScope::Admin
        )
    }

    /// Check if this scope includes write access
    pub fn includes_write(&self) -> bool {
        matches!(self, CapabilityScope::Write | CapabilityScope::Admin)
    }

    /// Check if this scope includes execute access
    pub fn includes_execute(&self) -> bool {
        matches!(self, CapabilityScope::Execute | CapabilityScope::Admin)
    }

    /// Check if this scope includes admin access
    pub fn includes_admin(&self) -> bool {
        matches!(self, CapabilityScope::Admin)
    }
}

/// Capability resource type
///
/// Identifies the type of resource that a capability controls access to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityResource {
    /// Storage chunk access
    Chunk(crate::content::ChunkId),
    /// Session access
    Session(crate::identifiers::SessionId),
    /// Account access
    Account(crate::identifiers::AccountId),
    /// Device access
    Device(crate::identifiers::DeviceId),
    /// Protocol access
    Protocol(crate::protocols::ProtocolType),
    /// Custom resource type
    Custom {
        /// Type of the custom resource
        resource_type: String,
        /// Identifier of the specific resource
        resource_id: String,
    },
}

impl fmt::Display for CapabilityResource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilityResource::Chunk(chunk_id) => write!(f, "chunk:{}", chunk_id),
            CapabilityResource::Session(session_id) => write!(f, "session:{}", session_id),
            CapabilityResource::Account(account_id) => write!(f, "account:{}", account_id.0),
            CapabilityResource::Device(device_id) => write!(f, "device:{}", device_id.0),
            CapabilityResource::Protocol(protocol_type) => write!(f, "protocol:{}", protocol_type),
            CapabilityResource::Custom {
                resource_type,
                resource_id,
            } => {
                write!(f, "{}:{}", resource_type, resource_id)
            }
        }
    }
}

/// Capability expiration time
///
/// Defines when a capability expires.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CapabilityExpiration {
    /// Never expires
    Never,
    /// Expires at a specific timestamp (Unix timestamp)
    Timestamp(u64),
    /// Expires after a duration (seconds from creation)
    Duration(u64),
    /// Expires at the end of a session
    SessionEnd(crate::identifiers::SessionId),
}

impl fmt::Display for CapabilityExpiration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CapabilityExpiration::Never => write!(f, "never"),
            CapabilityExpiration::Timestamp(timestamp) => write!(f, "timestamp:{}", timestamp),
            CapabilityExpiration::Duration(seconds) => write!(f, "duration:{}s", seconds),
            CapabilityExpiration::SessionEnd(session_id) => write!(f, "session-end:{}", session_id),
        }
    }
}

impl CapabilityExpiration {
    /// Check if the capability has expired at the given timestamp
    pub fn has_expired(&self, current_timestamp: u64) -> bool {
        match self {
            CapabilityExpiration::Never => false,
            CapabilityExpiration::Timestamp(expiry) => current_timestamp >= *expiry,
            CapabilityExpiration::Duration(_) => {
                // Duration-based expiration needs creation time to determine expiry
                // This should be handled by the capability management system
                false
            }
            CapabilityExpiration::SessionEnd(_) => {
                // Session-based expiration needs session state to determine expiry
                // This should be handled by the capability management system
                false
            }
        }
    }
}
