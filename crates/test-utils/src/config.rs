//! Standardized test configuration patterns
//!
//! This module provides consistent configuration patterns and defaults
//! for testing across the Aura codebase.

use std::time::Duration;

/// Standard test configuration
#[derive(Debug, Clone)]
pub struct TestConfig {
    /// Default timeout for async tests
    pub default_timeout: Duration,
    /// Base port for network tests
    pub base_network_port: u16,
    /// Enable verbose logging
    pub verbose_logging: bool,
    /// Random seed for deterministic tests
    pub random_seed: u64,
    /// Number of test iterations for property-based tests
    pub test_iterations: usize,
    /// Enable slow tests
    pub run_slow_tests: bool,
}

impl TestConfig {
    /// Create a new test configuration with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a test configuration for fast tests
    pub fn fast() -> Self {
        Self {
            default_timeout: Duration::from_secs(5),
            base_network_port: 9000,
            verbose_logging: false,
            random_seed: 42,
            test_iterations: 10,
            run_slow_tests: false,
        }
    }

    /// Create a test configuration for comprehensive tests
    pub fn comprehensive() -> Self {
        Self {
            default_timeout: Duration::from_secs(30),
            base_network_port: 9000,
            verbose_logging: true,
            random_seed: 42,
            test_iterations: 100,
            run_slow_tests: true,
        }
    }

    /// Create a test configuration for CI environments
    pub fn ci() -> Self {
        Self {
            default_timeout: Duration::from_secs(60),
            base_network_port: 9000,
            verbose_logging: true,
            random_seed: 42,
            test_iterations: 50,
            run_slow_tests: false,
        }
    }

    /// Set the timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Set the base network port
    pub fn with_base_port(mut self, port: u16) -> Self {
        self.base_network_port = port;
        self
    }

    /// Enable or disable verbose logging
    pub fn with_verbose_logging(mut self, verbose: bool) -> Self {
        self.verbose_logging = verbose;
        self
    }

    /// Set the random seed
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.random_seed = seed;
        self
    }

    /// Set the number of test iterations
    pub fn with_iterations(mut self, iterations: usize) -> Self {
        self.test_iterations = iterations;
        self
    }

    /// Enable or disable slow tests
    pub fn with_slow_tests(mut self, enabled: bool) -> Self {
        self.run_slow_tests = enabled;
        self
    }
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            default_timeout: Duration::from_secs(10),
            base_network_port: 9000,
            verbose_logging: false,
            random_seed: 42,
            test_iterations: 20,
            run_slow_tests: false,
        }
    }
}

/// Protocol test configuration
#[derive(Debug, Clone)]
pub struct ProtocolTestConfig {
    /// Number of devices in the protocol test
    pub device_count: usize,
    /// Threshold for threshold protocols
    pub threshold: u16,
    /// Number of protocol rounds
    pub rounds: usize,
    /// Simulate message loss probability (0.0 to 1.0)
    pub message_loss_rate: f64,
    /// Simulate network latency in milliseconds
    pub network_latency_ms: u64,
}

impl ProtocolTestConfig {
    /// Create a new protocol test configuration
    pub fn new(device_count: usize, threshold: u16) -> Self {
        Self {
            device_count,
            threshold,
            rounds: 1,
            message_loss_rate: 0.0,
            network_latency_ms: 0,
        }
    }

    /// Create configuration for a simple protocol test (no failures)
    pub fn simple(device_count: usize) -> Self {
        Self::new(device_count, (device_count as u16 + 1) / 2)
    }

    /// Create configuration for a resilient protocol test (with some failures)
    pub fn resilient(device_count: usize) -> Self {
        Self {
            device_count,
            threshold: (device_count as u16 + 1) / 2,
            rounds: 3,
            message_loss_rate: 0.1,
            network_latency_ms: 10,
        }
    }

    /// Create configuration for a stress test (high failure rate)
    pub fn stress_test(device_count: usize) -> Self {
        Self {
            device_count,
            threshold: (device_count as u16 + 1) / 2,
            rounds: 10,
            message_loss_rate: 0.3,
            network_latency_ms: 50,
        }
    }

    /// Set number of rounds
    pub fn with_rounds(mut self, rounds: usize) -> Self {
        self.rounds = rounds;
        self
    }

    /// Set message loss rate
    pub fn with_message_loss(mut self, rate: f64) -> Self {
        self.message_loss_rate = rate.max(0.0).min(1.0);
        self
    }

    /// Set network latency
    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.network_latency_ms = latency_ms;
        self
    }
}

impl Default for ProtocolTestConfig {
    fn default() -> Self {
        Self::new(3, 2)
    }
}

/// Storage test configuration
#[derive(Debug, Clone)]
pub struct StorageTestConfig {
    /// Number of storage devices
    pub device_count: usize,
    /// File size for storage tests
    pub file_size: u32,
    /// Number of files to test
    pub file_count: u32,
    /// Storage quota limit
    pub quota_limit: u64,
    /// Enable replication
    pub enable_replication: bool,
    /// Replication factor
    pub replication_factor: u16,
}

impl StorageTestConfig {
    /// Create a new storage test configuration
    pub fn new(device_count: usize) -> Self {
        Self {
            device_count,
            file_size: 1024, // 1KB
            file_count: 10,
            quota_limit: 1024 * 1024, // 1MB
            enable_replication: false,
            replication_factor: 1,
        }
    }

    /// Create configuration for small-scale storage test
    pub fn small() -> Self {
        Self {
            device_count: 2,
            file_size: 512, // 512B
            file_count: 5,
            quota_limit: 512 * 1024, // 512KB
            enable_replication: false,
            replication_factor: 1,
        }
    }

    /// Create configuration for medium-scale storage test
    pub fn medium() -> Self {
        Self {
            device_count: 3,
            file_size: 10 * 1024, // 10KB
            file_count: 50,
            quota_limit: 10 * 1024 * 1024, // 10MB
            enable_replication: true,
            replication_factor: 2,
        }
    }

    /// Create configuration for large-scale storage test
    pub fn large() -> Self {
        Self {
            device_count: 5,
            file_size: 100 * 1024, // 100KB
            file_count: 100,
            quota_limit: 100 * 1024 * 1024, // 100MB
            enable_replication: true,
            replication_factor: 3,
        }
    }

    /// Set file size
    pub fn with_file_size(mut self, size: u32) -> Self {
        self.file_size = size;
        self
    }

    /// Set number of files
    pub fn with_file_count(mut self, count: u32) -> Self {
        self.file_count = count;
        self
    }

    /// Set quota limit
    pub fn with_quota(mut self, quota: u64) -> Self {
        self.quota_limit = quota;
        self
    }

    /// Enable replication
    pub fn with_replication(mut self, factor: u16) -> Self {
        self.enable_replication = true;
        self.replication_factor = factor;
        self
    }
}

impl Default for StorageTestConfig {
    fn default() -> Self {
        Self::new(2)
    }
}

/// Common test configuration helpers
/// Helper functions for creating test configurations
pub mod config_helpers {
    use super::*;

    /// Get default test configuration
    pub fn default_config() -> TestConfig {
        TestConfig::default()
    }

    /// Get fast test configuration (for quick CI checks)
    pub fn fast_config() -> TestConfig {
        TestConfig::fast()
    }

    /// Get comprehensive test configuration (for thorough testing)
    pub fn comprehensive_config() -> TestConfig {
        TestConfig::comprehensive()
    }

    /// Get CI environment configuration
    pub fn ci_config() -> TestConfig {
        TestConfig::ci()
    }

    /// Get default protocol test configuration
    pub fn default_protocol_config() -> ProtocolTestConfig {
        ProtocolTestConfig::default()
    }

    /// Get simple protocol test configuration (no failures)
    pub fn simple_protocol_config(device_count: usize) -> ProtocolTestConfig {
        ProtocolTestConfig::simple(device_count)
    }

    /// Get resilient protocol test configuration
    pub fn resilient_protocol_config(device_count: usize) -> ProtocolTestConfig {
        ProtocolTestConfig::resilient(device_count)
    }

    /// Get default storage test configuration
    pub fn default_storage_config() -> StorageTestConfig {
        StorageTestConfig::default()
    }

    /// Get small storage test configuration
    pub fn small_storage_config() -> StorageTestConfig {
        StorageTestConfig::small()
    }

    /// Get medium storage test configuration
    pub fn medium_storage_config() -> StorageTestConfig {
        StorageTestConfig::medium()
    }

    /// Get large storage test configuration
    pub fn large_storage_config() -> StorageTestConfig {
        StorageTestConfig::large()
    }

    /// Determine test config based on environment
    pub fn config_for_environment() -> TestConfig {
        if cfg!(test) {
            if std::env::var("CI").is_ok() {
                TestConfig::ci()
            } else {
                TestConfig::default()
            }
        } else {
            TestConfig::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = TestConfig::default();
        assert!(config.default_timeout > Duration::from_secs(0));
        assert!(config.test_iterations > 0);
    }

    #[test]
    fn test_config_presets() {
        let fast = TestConfig::fast();
        let comprehensive = TestConfig::comprehensive();

        assert!(fast.default_timeout < comprehensive.default_timeout);
        assert!(fast.test_iterations < comprehensive.test_iterations);
    }

    #[test]
    fn test_protocol_config() {
        let config = ProtocolTestConfig::simple(3);
        assert_eq!(config.device_count, 3);
        assert_eq!(config.message_loss_rate, 0.0);
    }

    #[test]
    fn test_protocol_config_presets() {
        let simple = ProtocolTestConfig::simple(3);
        let resilient = ProtocolTestConfig::resilient(3);
        let stress = ProtocolTestConfig::stress_test(3);

        assert_eq!(simple.message_loss_rate, 0.0);
        assert!(resilient.message_loss_rate > 0.0);
        assert!(stress.message_loss_rate > resilient.message_loss_rate);
    }

    #[test]
    fn test_storage_config() {
        let small = StorageTestConfig::small();
        let medium = StorageTestConfig::medium();
        let large = StorageTestConfig::large();

        assert!(small.file_size < medium.file_size);
        assert!(medium.file_size < large.file_size);
        assert!(small.quota_limit < medium.quota_limit);
        assert!(medium.quota_limit < large.quota_limit);
    }

    #[test]
    fn test_config_builders() {
        let config = TestConfig::default()
            .with_timeout(Duration::from_secs(20))
            .with_seed(123)
            .with_iterations(50);

        assert_eq!(config.default_timeout, Duration::from_secs(20));
        assert_eq!(config.random_seed, 123);
        assert_eq!(config.test_iterations, 50);
    }
}
