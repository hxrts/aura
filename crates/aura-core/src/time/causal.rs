//! Causal metadata carried by facts whose reduction must be independent of
//! arrival order (docs/105_journal.md, "Order-independent reduction").
//!
//! These are plain data types. The generic reduction rules over them (tagged
//! observed-remove sets, multi-value registers, causal display order) live in
//! `aura-journal::causal_reduction`.

use super::{LogicalTime, VectorClock};
use crate::types::identifiers::DeviceId;
use serde::{Deserialize, Serialize};

/// Unique identity of one tagged add or register write.
///
/// It is never carried on the wire: readers derive it from the fact's
/// canonical encoding, which includes the fact's author and the writer's
/// freshly advanced logical clock. A peer therefore cannot reuse another
/// fact's tag to revoke or shadow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CausalTag(pub [u8; 32]);

impl CausalTag {
    /// Content-derived tag over a domain type id and canonical payload bytes.
    #[must_use]
    pub fn from_content(type_id: &str, payload: &[u8]) -> Self {
        let mut bytes = Vec::with_capacity(type_id.len() + 1 + payload.len());
        bytes.extend_from_slice(type_id.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(payload);
        Self(crate::crypto::hash::hash(&bytes))
    }
}

/// Wire form of a `LogicalTime`: entries are a sorted list rather than a map,
/// so every codec (including DAG-CBOR, which requires string map keys)
/// encodes it canonically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalClock {
    /// Lamport scalar of the writer's clock.
    pub lamport: u64,
    /// Vector entries sorted by device.
    pub vector: Vec<(DeviceId, u64)>,
}

impl CausalClock {
    /// Encode a logical time.
    #[must_use]
    pub fn from_logical(time: &LogicalTime) -> Self {
        let mut vector: Vec<(DeviceId, u64)> = time
            .vector
            .iter()
            .map(|(device, counter)| (*device, *counter))
            .collect();
        vector.sort();
        vector.dedup_by(|a, b| a.0 == b.0);
        Self {
            lamport: time.lamport,
            vector,
        }
    }

    /// Decode into a logical time.
    #[must_use]
    pub fn to_logical(&self) -> LogicalTime {
        let mut vector = VectorClock::new();
        for (device, counter) in &self.vector {
            vector.insert(*device, *counter);
        }
        LogicalTime {
            vector,
            lamport: self.lamport,
        }
    }

    fn counter(&self, device: &DeviceId) -> u64 {
        self.vector
            .binary_search_by(|(entry, _)| entry.cmp(device))
            .map(|index| self.vector[index].1)
            .unwrap_or(0)
    }

    /// Sum of vector entries. Strictly increases along happens-before, so
    /// ordering by it is a linear extension of causality.
    #[must_use]
    pub fn depth(&self) -> u64 {
        self.vector
            .iter()
            .fold(0u64, |sum, (_, counter)| sum.saturating_add(*counter))
    }

    /// Strict happens-before on the vector component.
    #[must_use]
    pub fn happens_before(&self, other: &Self) -> bool {
        let dominated = self
            .vector
            .iter()
            .all(|(device, counter)| *counter <= other.counter(device));
        dominated && self.vector_differs(other)
    }

    fn vector_differs(&self, other: &Self) -> bool {
        let trimmed = |clock: &Self| {
            clock
                .vector
                .iter()
                .filter(|(_, counter)| *counter > 0)
                .copied()
                .collect::<Vec<_>>()
        };
        trimmed(self) != trimmed(other)
    }
}

/// Causal metadata of one fact, stamped by its writer before commit. The
/// fact's own tag is not part of it; see `CausalTag`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CausalMetadata {
    /// Tags of adds this reversal removes: those the writer observed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub revokes: Vec<CausalTag>,
    /// Tags of register writes this write replaces: those the writer observed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<CausalTag>,
    /// The writer's logical clock after observing everything it references.
    pub clock: CausalClock,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(byte: u8) -> DeviceId {
        DeviceId(uuid::Uuid::from_bytes([byte; 16]))
    }

    #[test]
    fn clock_round_trips_and_orders_causally() {
        let mut vector = VectorClock::new();
        vector.insert(device(2), 3);
        vector.insert(device(1), 1);
        let earlier = CausalClock::from_logical(&LogicalTime { vector, lamport: 4 });
        assert_eq!(earlier.to_logical().lamport, 4);
        assert_eq!(CausalClock::from_logical(&earlier.to_logical()), earlier);

        let mut later = earlier.clone();
        later.vector[0].1 = 2;
        assert!(earlier.happens_before(&later));
        assert!(!later.happens_before(&earlier));
        assert!(earlier.depth() < later.depth());

        let concurrent = CausalClock {
            lamport: 1,
            vector: vec![(device(3), 1)],
        };
        assert!(!earlier.happens_before(&concurrent));
        assert!(!concurrent.happens_before(&earlier));
    }

    #[test]
    fn metadata_round_trips_through_dag_cbor_with_multi_device_clock() {
        let meta = CausalMetadata {
            revokes: vec![CausalTag([1; 32])],
            supersedes: Vec::new(),
            clock: CausalClock {
                lamport: 9,
                vector: vec![(device(1), 2), (device(2), 5)],
            },
        };
        let bytes = crate::util::serialization::to_vec(&meta).unwrap();
        let decoded: CausalMetadata = crate::util::serialization::from_slice(&bytes).unwrap();
        assert_eq!(decoded, meta);
    }
}
