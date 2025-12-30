//! Relationship and context types
//!
//! This module provides types for managing relationships between entities
//! and contextual information across the Aura platform.

use serde::{Deserialize, Serialize};
use std::fmt;

use crate::crypto::hash;

/// Relationship identifier for entity relationships
///
/// Unique identifier for relationships between entities in the system.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RelationshipId(pub [u8; 32]);

impl RelationshipId {
    /// Create a new relationship ID
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
                "RelationshipId must be exactly 32 bytes, got {}",
                bytes.len()
            )))
        }
    }

    /// Create from a 32-byte hash (typically Blake3)
    pub fn from_hash(hash: &[u8; 32]) -> Self {
        Self(*hash)
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
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

    /// Create deterministic relationship ID from two entity IDs
    pub fn from_entities(entity1: &[u8], entity2: &[u8]) -> Self {
        let mut inputs = [entity1, entity2];
        inputs.sort();
        let mut h = hash::hasher();
        h.update(inputs[0]);
        h.update(inputs[1]);
        Self(h.finalize())
    }
}

impl fmt::Display for RelationshipId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "relationship-{}", self.to_hex())
    }
}

impl From<[u8; 32]> for RelationshipId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<RelationshipId> for [u8; 32] {
    fn from(relationship_id: RelationshipId) -> Self {
        relationship_id.0
    }
}

/// Relationship type enumeration
///
/// Defines the type of relationship between entities.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationshipType {
    /// Guardian relationship (device <-> guardian)
    Guardian,
    /// Group membership relationship
    GroupMember,
    /// Trust relationship between entities
    Trust,
    /// Delegation relationship (authority delegation)
    Delegation,
    /// Capability grant relationship
    CapabilityGrant,
    /// Communication channel relationship
    CommunicationChannel,
    /// Custom relationship type
    Custom(String),
}

impl fmt::Display for RelationshipType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelationshipType::Guardian => write!(f, "guardian"),
            RelationshipType::GroupMember => write!(f, "group-member"),
            RelationshipType::Trust => write!(f, "trust"),
            RelationshipType::Delegation => write!(f, "delegation"),
            RelationshipType::CapabilityGrant => write!(f, "capability-grant"),
            RelationshipType::CommunicationChannel => write!(f, "communication-channel"),
            RelationshipType::Custom(custom) => write!(f, "custom:{custom}"),
        }
    }
}

/// Relationship status enumeration
///
/// Indicates the current status of a relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelationshipStatus {
    /// Relationship is active and valid
    Active,
    /// Relationship is pending approval
    Pending,
    /// Relationship has been suspended
    Suspended,
    /// Relationship has been revoked
    Revoked,
    /// Relationship has expired
    Expired,
}

impl fmt::Display for RelationshipStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RelationshipStatus::Active => write!(f, "active"),
            RelationshipStatus::Pending => write!(f, "pending"),
            RelationshipStatus::Suspended => write!(f, "suspended"),
            RelationshipStatus::Revoked => write!(f, "revoked"),
            RelationshipStatus::Expired => write!(f, "expired"),
        }
    }
}

// ContextId has been moved to identifiers.rs to avoid duplication and use UUID-based implementation

/// Trust level enumeration
///
/// Indicates the level of trust in a relationship or entity.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
pub enum TrustLevel {
    /// No trust - entity is untrusted
    #[default]
    None,
    /// Low trust - limited interactions allowed
    Low,
    /// Medium trust - normal interactions allowed
    Medium,
    /// High trust - extended interactions allowed
    High,
    /// Full trust - all interactions allowed
    Full,
}

impl fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TrustLevel::None => write!(f, "none"),
            TrustLevel::Low => write!(f, "low"),
            TrustLevel::Medium => write!(f, "medium"),
            TrustLevel::High => write!(f, "high"),
            TrustLevel::Full => write!(f, "full"),
        }
    }
}

impl TrustLevel {
    /// Check if this trust level meets or exceeds a required level
    pub fn meets_requirement(&self, required: TrustLevel) -> bool {
        *self >= required
    }

    /// Get all trust levels in order
    pub fn all() -> &'static [TrustLevel] {
        &[
            TrustLevel::None,
            TrustLevel::Low,
            TrustLevel::Medium,
            TrustLevel::High,
            TrustLevel::Full,
        ]
    }
}
