//! Content addressing types
//!
//! This module provides the foundational content identifier (Cid) type that is used
//! across multiple crates including aura-journal and aura-store for content addressing.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Content identifier for journal and ledger content
///
/// Used for content addressing within the journal system and storage operations.
/// This is a foundation type shared across multiple crates to avoid circular dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Cid(pub String);

impl Cid {
    /// Create a new content identifier
    pub fn new(cid: impl Into<String>) -> Self {
        Self(cid.into())
    }

    /// Get the CID string
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Create from blake3 hash
    pub fn from_hash(hash: &[u8; 32]) -> Self {
        Self(format!("blake3-{}", hex::encode(hash)))
    }
}

impl fmt::Display for Cid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for Cid {
    fn from(cid: String) -> Self {
        Self(cid)
    }
}

impl From<&str> for Cid {
    fn from(cid: &str) -> Self {
        Self(cid.to_string())
    }
}

impl From<[u8; 32]> for Cid {
    fn from(hash: [u8; 32]) -> Self {
        Self::from_hash(&hash)
    }
}

impl From<Vec<u8>> for Cid {
    fn from(bytes: Vec<u8>) -> Self {
        Self(hex::encode(&bytes))
    }
}

impl From<&[u8]> for Cid {
    fn from(bytes: &[u8]) -> Self {
        Self(hex::encode(bytes))
    }
}