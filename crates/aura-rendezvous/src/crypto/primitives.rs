//! Lightweight rendezvous crypto stand-ins until aura-crypto exposes full primitives.

use aura_core::{relationships::RelationshipId, AuraResult};
use serde::{Deserialize, Serialize};

/// Minimal blind signature representation.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BlindSignature {
    bytes: Vec<u8>,
}

impl BlindSignature {
    /// Create a blind signature from bytes.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Access raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Relationship-bound secret brand tag used for channel derivation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretBrand {
    relationship: RelationshipId,
    tag: [u8; 32],
}

impl SecretBrand {
    /// Construct a brand from a relationship identifier.
    pub fn new(relationship_id: RelationshipId) -> AuraResult<Self> {
        let mut tag = [0u8; 32];
        tag.copy_from_slice(relationship_id.as_bytes());
        Ok(Self {
            relationship: relationship_id,
            tag,
        })
    }

    /// Return bytes used for deriving channel identifiers.
    pub fn to_bytes(&self) -> AuraResult<Vec<u8>> {
        Ok(self.tag.to_vec())
    }

    /// Associated relationship identifier.
    pub fn relationship(&self) -> RelationshipId {
        self.relationship.clone()
    }
}

/// Minimal unlinkable credential container.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnlinkableCredential {
    bytes: Vec<u8>,
}

impl UnlinkableCredential {
    /// Create a credential from bytes.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Generate an empty credential.
    pub fn empty() -> Self {
        Self { bytes: Vec::new() }
    }

    /// Get credential as bytes.
    pub fn to_bytes(&self) -> &[u8] {
        &self.bytes
    }
}
