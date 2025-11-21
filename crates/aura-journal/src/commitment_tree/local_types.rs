//! Local device types for authority-internal use
//!
//! These types are used internally within an authority's commitment tree
//! and are never exposed externally. This maintains the authority abstraction
//! where devices are hidden implementation details.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Internal device identifier local to an authority
///
/// These IDs are only meaningful within a single authority context
/// and are never exposed in the public API or journal facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct LocalDeviceId(pub u32);

impl LocalDeviceId {
    /// Create a new local device ID
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Get the raw ID value
    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

impl fmt::Display for LocalDeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LocalDevice#{}", self.0)
    }
}

/// Authority-internal leaf node representation
///
/// This replaces the public LeafNode type with one that doesn't expose
/// external device identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLeafNode {
    /// Unique identifier for this leaf
    pub leaf_id: aura_core::tree::types::LeafId,

    /// Internal device reference (not exposed externally)
    pub local_device: LocalDeviceId,

    /// Serialized public key material
    pub public_key: Vec<u8>,

    /// Optional metadata (internal use only)
    pub metadata: Option<Vec<u8>>,
}

impl LocalLeafNode {
    /// Create a new local leaf node
    pub fn new(
        leaf_id: aura_core::tree::types::LeafId,
        local_device: LocalDeviceId,
        public_key: Vec<u8>,
    ) -> Self {
        Self {
            leaf_id,
            local_device,
            public_key,
            metadata: None,
        }
    }

    /// Convert to external representation (without device info)
    pub fn to_external(&self) -> ExternalLeafView {
        ExternalLeafView {
            leaf_id: self.leaf_id,
            public_key: self.public_key.clone(),
        }
    }
}

/// External view of a leaf node (no device information)
///
/// This is what gets exposed through the Authority trait API
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalLeafView {
    /// Leaf identifier (opaque to external users)
    pub leaf_id: aura_core::tree::types::LeafId,

    /// Public key material
    pub public_key: Vec<u8>,
}
