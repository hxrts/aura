//! Transport identifiers.
//!
//! Strongly typed identifiers for message IDs and sequencing.

use aura_core::hash::hasher;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

/// Unique identifier for a transport message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(Uuid);

impl MessageId {
    /// Create a new deterministic message identifier.
    pub fn new() -> Self {
        Self::from_uuid(Self::generate_uuid())
    }

    /// Wrap an existing UUID as a message identifier.
    pub fn from_uuid(uuid: Uuid) -> Self {
        Self(uuid)
    }

    /// Get the inner UUID.
    pub fn as_uuid(&self) -> Uuid {
        self.0
    }

    fn message_counter() -> &'static AtomicU64 {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        &COUNTER
    }

    fn generate_uuid() -> Uuid {
        let counter = Self::message_counter().fetch_add(1, Ordering::SeqCst);
        let mut h = hasher();
        h.update(b"aura-message-id");
        h.update(&counter.to_le_bytes());
        let digest = h.finalize();
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&digest[..16]);
        Uuid::from_bytes(uuid_bytes)
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

/// Sequence number for ordered message streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SequenceNumber(u64);

impl SequenceNumber {
    /// Starting sequence number.
    pub const ZERO: Self = Self(0);

    /// Create a new sequence number.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Extract the raw sequence value.
    pub fn value(self) -> u64 {
        self.0
    }

    /// Return the next sequence number.
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

impl Default for SequenceNumber {
    fn default() -> Self {
        Self::ZERO
    }
}

impl From<u64> for SequenceNumber {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<u32> for SequenceNumber {
    fn from(value: u32) -> Self {
        Self(value as u64)
    }
}
