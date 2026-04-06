//! Authority-internal leaf types
//!
//! These types are used internally within an authority's commitment tree and
//! are never exposed externally. This maintains the authority abstraction
//! where devices are hidden implementation details.

use serde::{Deserialize, Serialize};

/// Authority-internal leaf node representation
///
/// This replaces the public LeafNode type with one that doesn't expose
/// external device identifiers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalLeafNode {
    /// Unique identifier for this leaf
    pub leaf_id: aura_core::tree::types::LeafId,

    /// Serialized public key material
    pub public_key: Vec<u8>,

    /// Optional metadata (internal use only)
    pub metadata: Option<Vec<u8>>,
}

impl LocalLeafNode {
    /// Create a new local leaf node
    pub fn new(leaf_id: aura_core::tree::types::LeafId, public_key: Vec<u8>) -> Self {
        Self {
            leaf_id,
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
