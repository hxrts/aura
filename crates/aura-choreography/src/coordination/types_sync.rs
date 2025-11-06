//! Synchronization protocol configuration and types

use serde::{Deserialize, Serialize};

/// Configuration for state synchronization choreographies
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncConfig {
    /// Maximum message size for sync operations (bytes)
    pub max_message_size: usize,

    /// Timeout for coordinator heartbeats (milliseconds)
    pub coordinator_timeout_ms: u64,

    /// Maximum number of concurrent sync operations
    pub max_concurrent_syncs: usize,

    /// Enable privacy-preserving timing obfuscation
    pub enable_timing_obfuscation: bool,

    /// Cover traffic interval (milliseconds)
    pub cover_traffic_interval_ms: u64,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            max_message_size: 10 * 1024 * 1024, // 10MB
            coordinator_timeout_ms: 30_000,     // 30 seconds
            max_concurrent_syncs: 10,
            enable_timing_obfuscation: true,
            cover_traffic_interval_ms: 5_000, // 5 seconds
        }
    }
}

impl SyncConfig {
    /// Create a new sync configuration with custom values
    pub fn new(
        max_message_size: usize,
        coordinator_timeout_ms: u64,
        max_concurrent_syncs: usize,
        enable_timing_obfuscation: bool,
        cover_traffic_interval_ms: u64,
    ) -> Self {
        Self {
            max_message_size,
            coordinator_timeout_ms,
            max_concurrent_syncs,
            enable_timing_obfuscation,
            cover_traffic_interval_ms,
        }
    }

    /// Create a configuration optimized for testing (faster timeouts, no obfuscation)
    pub fn for_testing() -> Self {
        Self {
            max_message_size: 1024 * 1024, // 1MB
            coordinator_timeout_ms: 1_000, // 1 second
            max_concurrent_syncs: 5,
            enable_timing_obfuscation: false,
            cover_traffic_interval_ms: 100, // 100ms
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.max_message_size, 10 * 1024 * 1024);
        assert_eq!(config.coordinator_timeout_ms, 30_000);
        assert_eq!(config.max_concurrent_syncs, 10);
        assert!(config.enable_timing_obfuscation);
        assert_eq!(config.cover_traffic_interval_ms, 5_000);
    }

    #[test]
    fn test_sync_config_for_testing() {
        let config = SyncConfig::for_testing();
        assert_eq!(config.max_message_size, 1024 * 1024);
        assert_eq!(config.coordinator_timeout_ms, 1_000);
        assert_eq!(config.max_concurrent_syncs, 5);
        assert!(!config.enable_timing_obfuscation);
        assert_eq!(config.cover_traffic_interval_ms, 100);
    }
}
