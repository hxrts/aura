//! Coordinator utilities for distributed protocol coordination
//!
//! This module provides types and utilities for temporary coordinator selection
//! and failure detection in distributed protocols.

use serde::{Deserialize, Serialize};

/// Heartbeat message for coordinator liveness detection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Heartbeat {
    /// Current epoch
    pub epoch: u64,
    /// Timestamp when heartbeat was sent
    pub timestamp: u64,
}

/// Configuration for coordinator timeout detection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoordinatorTimeout {
    /// Timeout threshold in milliseconds
    pub timeout_ms: u64,
}

impl Default for CoordinatorTimeout {
    fn default() -> Self {
        Self {
            timeout_ms: 30_000, // 30 seconds
        }
    }
}

impl CoordinatorTimeout {
    /// Create a new coordinator timeout configuration
    pub fn new(timeout_ms: u64) -> Self {
        Self { timeout_ms }
    }

    /// Check if the timeout has been exceeded
    pub fn is_exceeded(&self, elapsed_ms: u64) -> bool {
        elapsed_ms > self.timeout_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_heartbeat_creation() {
        let heartbeat = Heartbeat {
            epoch: 42,
            timestamp: 1000,
        };
        assert_eq!(heartbeat.epoch, 42);
        assert_eq!(heartbeat.timestamp, 1000);
    }

    #[test]
    fn test_coordinator_timeout_default() {
        let timeout = CoordinatorTimeout::default();
        assert_eq!(timeout.timeout_ms, 30_000);
    }

    #[test]
    fn test_coordinator_timeout_is_exceeded() {
        let timeout = CoordinatorTimeout::new(5000);
        assert!(!timeout.is_exceeded(4999));
        assert!(!timeout.is_exceeded(5000));
        assert!(timeout.is_exceeded(5001));
    }
}
