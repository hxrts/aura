//! Timestamp ordering utilities for B-tree indexing.
//!
//! Since `TimeStamp` doesn't implement `Ord`, we extract a comparable
//! representation for B-tree indexing.

use aura_core::time::TimeStamp;

/// Orderable key for timestamp indexing
///
/// Since `TimeStamp` doesn't implement `Ord`, we extract a comparable
/// representation for B-tree indexing. Physical timestamps use millis,
/// others use a hash-based ordering.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TimeKey {
    /// Primary sort key (milliseconds for physical, hash for others)
    pub(crate) millis: u64,
    /// Original timestamp for retrieval
    pub(crate) original: TimeStampWrapper,
}

/// Wrapper to store TimeStamp alongside the key
#[derive(Debug, Clone)]
pub(crate) struct TimeStampWrapper(pub(crate) TimeStamp);

impl PartialEq for TimeStampWrapper {
    fn eq(&self, other: &Self) -> bool {
        self.to_millis() == other.to_millis()
    }
}

impl Eq for TimeStampWrapper {}

impl PartialOrd for TimeStampWrapper {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TimeStampWrapper {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.to_millis().cmp(&other.to_millis())
    }
}

impl TimeStampWrapper {
    pub(crate) fn to_millis(&self) -> u64 {
        timestamp_to_millis(&self.0)
    }
}

/// Extract a comparable u64 from a TimeStamp for ordering purposes
pub(crate) fn timestamp_to_millis(ts: &TimeStamp) -> u64 {
    match ts {
        TimeStamp::PhysicalClock(pt) => pt.ts_ms,
        TimeStamp::LogicalClock(lt) => lt.lamport,
        TimeStamp::OrderClock(ot) => {
            // Use first 8 bytes of order clock as u64 for ordering
            u64::from_le_bytes([
                ot.0[0], ot.0[1], ot.0[2], ot.0[3], ot.0[4], ot.0[5], ot.0[6], ot.0[7],
            ])
        }
        TimeStamp::Range(rt) => rt.earliest_ms,
    }
}

impl TimeKey {
    pub(crate) fn from_timestamp(ts: TimeStamp) -> Self {
        let millis = timestamp_to_millis(&ts);
        Self {
            millis,
            original: TimeStampWrapper(ts),
        }
    }
}
