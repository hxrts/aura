//! Time and causality tracking primitives
//!
//! This module provides Lamport timestamps for causal ordering in distributed systems
//! and time utility functions.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Lamport timestamp for causal ordering
///
/// Used for Last-Writer-Wins conflict resolution. Higher timestamps win.
/// Provides happens-before relationships in distributed systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
pub struct LamportTimestamp(pub u64);

impl LamportTimestamp {
    /// Create a new timestamp with value 0
    pub fn zero() -> Self {
        Self(0)
    }

    /// Create a new timestamp with a specific value
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Get the inner value
    pub fn value(&self) -> u64 {
        self.0
    }

    /// Increment timestamp (for local events)
    pub fn increment(&mut self) {
        self.0 += 1;
    }

    /// Merge with another timestamp (take maximum) and increment
    ///
    /// Used when receiving messages to update local clock:
    /// local_clock = max(local_clock, message_clock) + 1
    pub fn merge_and_increment(&mut self, other: LamportTimestamp) {
        self.0 = self.0.max(other.0) + 1;
    }

    /// Merge with another timestamp (take maximum)
    ///
    /// Used for comparing clocks without incrementing.
    pub fn merge(&mut self, other: LamportTimestamp) {
        self.0 = self.0.max(other.0);
    }

    /// Create a new timestamp that is the maximum of two timestamps
    pub fn max(self, other: LamportTimestamp) -> LamportTimestamp {
        LamportTimestamp(self.0.max(other.0))
    }
}

impl Default for LamportTimestamp {
    fn default() -> Self {
        Self::zero()
    }
}

impl From<u64> for LamportTimestamp {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<LamportTimestamp> for u64 {
    fn from(timestamp: LamportTimestamp) -> u64 {
        timestamp.0
    }
}

impl std::fmt::Display for LamportTimestamp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// Time utility functions

/// Get current Unix timestamp in seconds
///
/// Uses system time. In production with effects system, use TimeEffects instead.
#[allow(clippy::disallowed_methods)]
pub fn current_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Get current Unix timestamp in milliseconds
///
/// Uses system time. In production with effects system, use TimeEffects instead.
#[allow(clippy::disallowed_methods)]
pub fn current_unix_timestamp_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Get current SystemTime
///
/// Uses system time. In production with effects system, use TimeEffects instead.
#[allow(clippy::disallowed_methods)]
pub fn current_system_time() -> SystemTime {
    SystemTime::now()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lamport_timestamp_creation() {
        let ts = LamportTimestamp::new(42);
        assert_eq!(ts.value(), 42);
        assert_eq!(ts.0, 42);
    }

    #[test]
    fn test_lamport_timestamp_zero() {
        let ts = LamportTimestamp::zero();
        assert_eq!(ts.value(), 0);
    }

    #[test]
    fn test_lamport_timestamp_increment() {
        let mut ts = LamportTimestamp::new(10);
        ts.increment();
        assert_eq!(ts.value(), 11);
        ts.increment();
        assert_eq!(ts.value(), 12);
    }

    #[test]
    fn test_lamport_timestamp_merge() {
        let mut ts1 = LamportTimestamp::new(10);
        let ts2 = LamportTimestamp::new(20);

        ts1.merge(ts2);
        assert_eq!(ts1.value(), 20);

        // Merging with lower value should not change
        let ts3 = LamportTimestamp::new(15);
        ts1.merge(ts3);
        assert_eq!(ts1.value(), 20);
    }

    #[test]
    fn test_lamport_timestamp_merge_and_increment() {
        let mut local = LamportTimestamp::new(10);
        let remote = LamportTimestamp::new(20);

        local.merge_and_increment(remote);
        assert_eq!(local.value(), 21); // max(10, 20) + 1
    }

    #[test]
    fn test_lamport_timestamp_ordering() {
        let ts1 = LamportTimestamp::new(10);
        let ts2 = LamportTimestamp::new(20);
        let ts3 = LamportTimestamp::new(10);

        assert!(ts1 < ts2);
        assert!(ts2 > ts1);
        assert_eq!(ts1, ts3);
    }

    #[test]
    fn test_lamport_timestamp_max() {
        let ts1 = LamportTimestamp::new(10);
        let ts2 = LamportTimestamp::new(20);

        let max_ts = ts1.max(ts2);
        assert_eq!(max_ts.value(), 20);
    }

    #[test]
    fn test_lamport_timestamp_conversions() {
        let ts = LamportTimestamp::from(42u64);
        assert_eq!(ts.value(), 42);

        let value: u64 = ts.into();
        assert_eq!(value, 42);
    }

    #[test]
    fn test_lamport_timestamp_display() {
        let ts = LamportTimestamp::new(123);
        assert_eq!(format!("{}", ts), "123");
    }
}
