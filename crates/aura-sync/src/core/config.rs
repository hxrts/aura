//! Unified configuration for aura-sync protocols
//!
//! This module provides a centralized configuration system that consolidates
//! all timeout, retry, batch size, and peer management settings scattered
//! across the aura-sync crate into a single, coherent structure.

use aura_core::effects::RandomEffects;
use serde::{Deserialize, Serialize};
use std::env;
use std::time::Duration;

/// Master configuration for all aura-sync operations
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncConfig {
    /// Network and transport configuration
    pub network: NetworkConfig,
    /// Retry and backoff configuration
    pub retry: RetryConfig,
    /// Batch processing configuration
    pub batching: BatchConfig,
    /// Peer management configuration
    pub peer_management: PeerManagementConfig,
    /// Protocol-specific configurations
    pub protocols: ProtocolConfigs,
    /// Performance and load balancing
    pub performance: PerformanceConfig,
}

/// Network timing and connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Base interval between sync rounds (default: 30s)
    pub base_sync_interval: Duration,
    /// Minimum interval between syncs with the same peer (default: 10s)
    pub min_sync_interval: Duration,
    /// Maximum time to wait for any sync operation (default: 120s)
    pub sync_timeout: Duration,
    /// Cleanup interval for stale sessions (default: 5 minutes)
    pub cleanup_interval: Duration,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            base_sync_interval: Duration::from_secs(30),
            min_sync_interval: Duration::from_secs(10),
            sync_timeout: Duration::from_secs(120),
            cleanup_interval: Duration::from_secs(300),
        }
    }
}

/// Retry and exponential backoff configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (default: 3)
    pub max_retries: u32,
    /// Base delay for exponential backoff (default: 500ms)
    pub base_delay: Duration,
    /// Maximum delay between retries (default: 30s)
    pub max_delay: Duration,
    /// Jitter factor for randomizing delays 0.0-1.0 (default: 0.1)
    pub jitter_factor: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            jitter_factor: 0.1,
        }
    }
}

impl RetryConfig {
    /// Calculate delay for attempt number with jitter
    pub async fn delay_for_attempt<R: RandomEffects + ?Sized>(
        &self,
        attempt: u32,
        random: &R,
    ) -> Duration {
        let base_ms = self.base_delay.as_millis() as f64;
        let exponential_delay = base_ms * 2_f64.powi(attempt as i32);
        let jitter_sample = random.random_range(0, 10_000).await as f64 / 10_000.0;
        let jittered_delay = exponential_delay * (1.0 + self.jitter_factor * jitter_sample);
        let clamped_delay = jittered_delay.min(self.max_delay.as_millis() as f64);
        Duration::from_millis(clamped_delay as u64)
    }

    /// Check if should retry for given attempt number
    pub fn should_retry(&self, attempt: u32) -> bool {
        attempt < self.max_retries
    }
}

/// Batch processing and throughput configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    /// Default batch size for operations (default: 128)
    pub default_batch_size: usize,
    /// Maximum operations per synchronization round (default: 1000)
    pub max_operations_per_round: usize,
    /// Enable compression for large batches (default: true)
    pub enable_compression: bool,
    /// Minimum batch size before forcing processing (default: 10)
    pub min_batch_size: usize,
    /// Maximum time to wait before processing partial batch (default: 5s)
    pub batch_timeout: Duration,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            default_batch_size: 128,
            max_operations_per_round: 1000,
            enable_compression: true,
            min_batch_size: 10,
            batch_timeout: Duration::from_secs(5),
        }
    }
}

/// Peer selection and management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerManagementConfig {
    /// Maximum concurrent synchronization sessions (default: 5)
    pub max_concurrent_syncs: usize,
    /// Minimum priority threshold for scheduling (default: 10)
    pub min_priority_threshold: u32,
    /// Priority boost for peers with pending operations (default: 20)
    pub pending_operations_boost: u32,
    /// Penalty for peers with recent failures (default: 15)
    pub failure_penalty: u32,
    /// Time to wait before retrying failed peer (default: 5 minutes)
    pub failure_backoff_duration: Duration,
}

impl Default for PeerManagementConfig {
    fn default() -> Self {
        Self {
            max_concurrent_syncs: 5,
            min_priority_threshold: 10,
            pending_operations_boost: 20,
            failure_penalty: 15,
            failure_backoff_duration: Duration::from_secs(300),
        }
    }
}

/// Protocol-specific configuration grouping
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProtocolConfigs {
    /// OTA upgrade protocol configuration
    pub ota_upgrade: OTAConfig,
    /// Receipt verification configuration
    pub verification: VerificationConfig,
    /// Anti-entropy protocol configuration
    pub anti_entropy: AntiEntropyConfig,
}

/// OTA (Over-The-Air) upgrade protocol configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OTAConfig {
    /// Timeout for soft fork proposals (default: 1 hour)
    pub soft_fork_timeout: Duration,
    /// Timeout for hard fork proposals (default: 1 week)
    pub hard_fork_timeout: Duration,
    /// Default readiness threshold (default: 2)
    pub default_threshold: u32,
    /// Maximum session duration (default: 24 hours)
    pub max_session_duration: Duration,
    /// Enable automatic validation (default: true)
    pub enable_auto_validation: bool,
}

impl Default for OTAConfig {
    fn default() -> Self {
        Self {
            soft_fork_timeout: Duration::from_secs(3600), // 1 hour
            hard_fork_timeout: Duration::from_secs(7 * 24 * 3600), // 1 week
            default_threshold: 2,
            max_session_duration: Duration::from_secs(24 * 3600), // 24 hours
            enable_auto_validation: true,
        }
    }
}

/// Receipt verification configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationConfig {
    /// Timeout for receipt verification (default: 30s)
    pub verification_timeout: Duration,
    /// Required confirmations for verification (default: 2)
    pub required_confirmations: usize,
    /// Maximum verification attempts (default: 3)
    pub max_verification_attempts: u32,
}

impl Default for VerificationConfig {
    fn default() -> Self {
        Self {
            verification_timeout: Duration::from_secs(30),
            required_confirmations: 2,
            max_verification_attempts: 3,
        }
    }
}

/// Anti-entropy protocol configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyConfig {
    /// Minimum time between anti-entropy rounds (default: 60s)
    pub min_sync_interval: Duration,
    /// Digest comparison timeout (default: 10s)
    pub digest_timeout: Duration,
    /// Maximum digest entries per message (default: 1000)
    pub max_digest_entries: usize,
}

impl Default for AntiEntropyConfig {
    fn default() -> Self {
        Self {
            min_sync_interval: Duration::from_secs(60),
            digest_timeout: Duration::from_secs(10),
            max_digest_entries: 1000,
        }
    }
}

/// Performance tuning and resource management configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Maximum CPU usage percentage 0-100 (default: 80)
    pub max_cpu_usage: u32,
    /// Maximum network bandwidth usage in bytes/sec (default: 10MB/s)
    pub max_network_bandwidth: u64,
    /// Enable adaptive scheduling based on system load (default: true)
    pub adaptive_scheduling: bool,
    /// Memory limit for batching operations in bytes (default: 100MB)
    pub memory_limit: usize,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            max_cpu_usage: 80,
            max_network_bandwidth: 10 * 1024 * 1024, // 10 MB/s
            adaptive_scheduling: true,
            memory_limit: 100 * 1024 * 1024, // 100 MB
        }
    }
}

impl PerformanceConfig {
    /// Validate performance configuration values
    pub fn validate(&self) -> Result<(), String> {
        if self.max_cpu_usage > 100 {
            return Err("max_cpu_usage must be <= 100".to_string());
        }
        if self.max_network_bandwidth == 0 {
            return Err("max_network_bandwidth must be > 0".to_string());
        }
        if self.memory_limit < 1024 * 1024 {
            // 1MB minimum
            return Err("memory_limit must be >= 1MB".to_string());
        }
        Ok(())
    }
}

impl SyncConfig {
    /// Create a configuration optimized for testing
    pub fn for_testing() -> Self {
        Self {
            network: NetworkConfig {
                base_sync_interval: Duration::from_millis(100),
                min_sync_interval: Duration::from_millis(50),
                sync_timeout: Duration::from_secs(5),
                cleanup_interval: Duration::from_secs(10),
            },
            retry: RetryConfig {
                max_retries: 2,
                base_delay: Duration::from_millis(10),
                max_delay: Duration::from_millis(100),
                jitter_factor: 0.0, // No jitter in tests for predictability
            },
            batching: BatchConfig {
                default_batch_size: 10,
                max_operations_per_round: 50,
                enable_compression: false, // Faster for tests
                min_batch_size: 1,
                batch_timeout: Duration::from_millis(100),
            },
            performance: PerformanceConfig {
                max_cpu_usage: 100, // No limits in tests
                max_network_bandwidth: u64::MAX,
                adaptive_scheduling: false, // Predictable behavior
                memory_limit: usize::MAX,
            },
            ..Self::default()
        }
    }

    /// Create a configuration optimized for production
    pub fn for_production() -> Self {
        Self {
            network: NetworkConfig {
                base_sync_interval: Duration::from_secs(60), // Less frequent in prod
                sync_timeout: Duration::from_secs(300),      // Longer timeout
                ..NetworkConfig::default()
            },
            retry: RetryConfig {
                max_retries: 5,                     // More retries in production
                max_delay: Duration::from_secs(60), // Longer max delay
                ..RetryConfig::default()
            },
            performance: PerformanceConfig {
                max_cpu_usage: 60, // Conservative CPU usage
                adaptive_scheduling: true,
                ..PerformanceConfig::default()
            },
            ..Self::default()
        }
    }

    /// Validate the entire configuration
    pub fn validate(&self) -> Result<(), String> {
        // Validate performance config
        self.performance.validate()?;

        // Validate network timeouts make sense
        if self.network.min_sync_interval >= self.network.base_sync_interval {
            return Err("min_sync_interval must be less than base_sync_interval".to_string());
        }

        // Validate retry config
        if self.retry.jitter_factor < 0.0 || self.retry.jitter_factor > 1.0 {
            return Err("jitter_factor must be between 0.0 and 1.0".to_string());
        }

        // Validate batch config
        if self.batching.min_batch_size > self.batching.default_batch_size {
            return Err("min_batch_size must be <= default_batch_size".to_string());
        }

        Ok(())
    }

    /// Load configuration from environment variables with fallbacks
    pub fn from_env() -> Self {
        let mut config = Self::default();

        // Network
        config.network.base_sync_interval = duration_secs(
            "AURA_SYNC_BASE_SYNC_INTERVAL_SECS",
            config.network.base_sync_interval,
        );
        config.network.min_sync_interval = duration_secs(
            "AURA_SYNC_MIN_SYNC_INTERVAL_SECS",
            config.network.min_sync_interval,
        );
        config.network.sync_timeout =
            duration_secs("AURA_SYNC_TIMEOUT_SECS", config.network.sync_timeout);
        config.network.cleanup_interval = duration_secs(
            "AURA_SYNC_CLEANUP_INTERVAL_SECS",
            config.network.cleanup_interval,
        );

        // Retry
        config.retry.max_retries =
            parse_u32("AURA_SYNC_RETRY_MAX_RETRIES", config.retry.max_retries);
        config.retry.base_delay =
            duration_millis("AURA_SYNC_RETRY_BASE_DELAY_MS", config.retry.base_delay);
        config.retry.max_delay =
            duration_millis("AURA_SYNC_RETRY_MAX_DELAY_MS", config.retry.max_delay);
        config.retry.jitter_factor =
            parse_f64("AURA_SYNC_RETRY_JITTER", config.retry.jitter_factor);

        // Batching
        config.batching.default_batch_size = parse_usize(
            "AURA_SYNC_DEFAULT_BATCH_SIZE",
            config.batching.default_batch_size,
        );
        config.batching.max_operations_per_round = parse_usize(
            "AURA_SYNC_MAX_OPS_PER_ROUND",
            config.batching.max_operations_per_round,
        );
        config.batching.enable_compression = parse_bool(
            "AURA_SYNC_ENABLE_COMPRESSION",
            config.batching.enable_compression,
        );
        config.batching.min_batch_size =
            parse_usize("AURA_SYNC_MIN_BATCH_SIZE", config.batching.min_batch_size);
        config.batching.batch_timeout =
            duration_millis("AURA_SYNC_BATCH_TIMEOUT_MS", config.batching.batch_timeout);

        // Peer management
        config.peer_management.max_concurrent_syncs = parse_usize(
            "AURA_SYNC_MAX_CONCURRENT_SYNCS",
            config.peer_management.max_concurrent_syncs,
        );
        config.peer_management.min_priority_threshold = parse_u32(
            "AURA_SYNC_MIN_PRIORITY_THRESHOLD",
            config.peer_management.min_priority_threshold,
        );
        config.peer_management.pending_operations_boost = parse_u32(
            "AURA_SYNC_PENDING_OPS_BOOST",
            config.peer_management.pending_operations_boost,
        );
        config.peer_management.failure_penalty = parse_u32(
            "AURA_SYNC_FAILURE_PENALTY",
            config.peer_management.failure_penalty,
        );
        config.peer_management.failure_backoff_duration = duration_secs(
            "AURA_SYNC_FAILURE_BACKOFF_SECS",
            config.peer_management.failure_backoff_duration,
        );

        // Protocols
        config.protocols.anti_entropy.min_sync_interval = duration_secs(
            "AURA_SYNC_ANTI_ENTROPY_MIN_INTERVAL_SECS",
            config.protocols.anti_entropy.min_sync_interval,
        );
        config.protocols.anti_entropy.digest_timeout = duration_secs(
            "AURA_SYNC_ANTI_ENTROPY_DIGEST_TIMEOUT_SECS",
            config.protocols.anti_entropy.digest_timeout,
        );
        config.protocols.anti_entropy.max_digest_entries = parse_usize(
            "AURA_SYNC_ANTI_ENTROPY_MAX_DIGEST_ENTRIES",
            config.protocols.anti_entropy.max_digest_entries,
        );

        config.protocols.verification.verification_timeout = duration_secs(
            "AURA_SYNC_VERIFICATION_TIMEOUT_SECS",
            config.protocols.verification.verification_timeout,
        );
        config.protocols.verification.required_confirmations = parse_usize(
            "AURA_SYNC_VERIFICATION_CONFIRMATIONS",
            config.protocols.verification.required_confirmations,
        );
        config.protocols.verification.max_verification_attempts = parse_u32(
            "AURA_SYNC_VERIFICATION_MAX_ATTEMPTS",
            config.protocols.verification.max_verification_attempts,
        );

        config.protocols.ota_upgrade.soft_fork_timeout = duration_secs(
            "AURA_SYNC_OTA_SOFT_FORK_TIMEOUT_SECS",
            config.protocols.ota_upgrade.soft_fork_timeout,
        );
        config.protocols.ota_upgrade.hard_fork_timeout = duration_secs(
            "AURA_SYNC_OTA_HARD_FORK_TIMEOUT_SECS",
            config.protocols.ota_upgrade.hard_fork_timeout,
        );
        config.protocols.ota_upgrade.default_threshold = parse_u32(
            "AURA_SYNC_OTA_DEFAULT_THRESHOLD",
            config.protocols.ota_upgrade.default_threshold,
        );
        config.protocols.ota_upgrade.max_session_duration = duration_secs(
            "AURA_SYNC_OTA_MAX_SESSION_DURATION_SECS",
            config.protocols.ota_upgrade.max_session_duration,
        );
        config.protocols.ota_upgrade.enable_auto_validation = parse_bool(
            "AURA_SYNC_OTA_ENABLE_AUTO_VALIDATION",
            config.protocols.ota_upgrade.enable_auto_validation,
        );

        // Performance
        config.performance.max_cpu_usage =
            parse_u32("AURA_SYNC_MAX_CPU_USAGE", config.performance.max_cpu_usage);
        config.performance.max_network_bandwidth = parse_u64(
            "AURA_SYNC_MAX_NETWORK_BANDWIDTH",
            config.performance.max_network_bandwidth,
        );
        config.performance.adaptive_scheduling = parse_bool(
            "AURA_SYNC_ADAPTIVE_SCHEDULING",
            config.performance.adaptive_scheduling,
        );
        config.performance.memory_limit = parse_usize(
            "AURA_SYNC_MEMORY_LIMIT_BYTES",
            config.performance.memory_limit,
        );

        config
    }

    /// Get a configuration builder for custom setups
    pub fn builder() -> SyncConfigBuilder {
        SyncConfigBuilder::new()
    }
}

/// Builder pattern for creating custom configurations
pub struct SyncConfigBuilder {
    config: SyncConfig,
}

impl SyncConfigBuilder {
    /// Create a new configuration builder with default settings
    pub fn new() -> Self {
        Self {
            config: SyncConfig::default(),
        }
    }

    /// Set network configuration
    pub fn network(mut self, network: NetworkConfig) -> Self {
        self.config.network = network;
        self
    }

    /// Set retry configuration
    pub fn retry(mut self, retry: RetryConfig) -> Self {
        self.config.retry = retry;
        self
    }

    /// Set batching configuration
    pub fn batching(mut self, batching: BatchConfig) -> Self {
        self.config.batching = batching;
        self
    }

    /// Set peer management configuration
    pub fn peer_management(mut self, peer_management: PeerManagementConfig) -> Self {
        self.config.peer_management = peer_management;
        self
    }

    /// Set protocol configurations
    pub fn protocols(mut self, protocols: ProtocolConfigs) -> Self {
        self.config.protocols = protocols;
        self
    }

    /// Set performance configuration
    pub fn performance(mut self, performance: PerformanceConfig) -> Self {
        self.config.performance = performance;
        self
    }

    /// Build and validate the configuration
    pub fn build(self) -> Result<SyncConfig, String> {
        self.config.validate()?;
        Ok(self.config)
    }
}

fn parse_bool(key: &str, default: bool) -> bool {
    env::var(key)
        .ok()
        .as_deref()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(default)
}

fn parse_u32(key: &str, default: u32) -> u32 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(default)
}

fn parse_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

fn parse_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(default)
}

fn parse_f64(key: &str, default: f64) -> f64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(default)
}

fn duration_secs(key: &str, default: Duration) -> Duration {
    env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(default)
}

fn duration_millis(key: &str, default: Duration) -> Duration {
    env::var(key)
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(default)
}

impl Default for SyncConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_valid() {
        let config = SyncConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_testing_config() {
        let config = SyncConfig::for_testing();
        assert!(config.validate().is_ok());
        assert_eq!(config.retry.jitter_factor, 0.0);
        assert!(!config.batching.enable_compression);
    }

    #[test]
    fn test_production_config() {
        let config = SyncConfig::for_production();
        assert!(config.validate().is_ok());
        assert_eq!(config.retry.max_retries, 5);
        assert_eq!(config.performance.max_cpu_usage, 60);
    }

    #[tokio::test]
    async fn test_retry_config_delay_calculation() {
        let retry_config = RetryConfig::default();
        let fixture = aura_testkit::create_test_fixture()
            .await
            .unwrap_or_else(|_| panic!("Failed to create test fixture"));
        let device_id = fixture.device_id();
        let composer = aura_testkit::foundation::TestEffectComposer::new(
            aura_core::effects::ExecutionMode::Testing,
            device_id,
        );
        let handler = composer
            .build_mock_handler()
            .unwrap_or_else(|err| panic!("build mock handler for retry tests: {err}"));

        let delay1 = retry_config.delay_for_attempt(0, handler.as_ref()).await;
        let delay2 = retry_config.delay_for_attempt(1, handler.as_ref()).await;

        // Second attempt should have longer delay (exponential backoff)
        assert!(delay2 > delay1);

        // Should not exceed max delay
        let delay_long = retry_config.delay_for_attempt(10, handler.as_ref()).await;
        assert!(delay_long <= retry_config.max_delay);
    }

    #[test]
    fn test_config_validation() {
        let mut config = SyncConfig::default();

        // Test invalid CPU usage
        config.performance.max_cpu_usage = 150;
        assert!(config.validate().is_err());

        config.performance.max_cpu_usage = 80;

        // Test invalid jitter factor
        config.retry.jitter_factor = 2.0;
        assert!(config.validate().is_err());

        config.retry.jitter_factor = 0.1;
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_builder() {
        let config = SyncConfig::builder()
            .retry(RetryConfig {
                max_retries: 10,
                ..RetryConfig::default()
            })
            .build()
            .unwrap();

        assert_eq!(config.retry.max_retries, 10);
    }

    #[test]
    fn test_builder_validation() {
        let result = SyncConfig::builder()
            .performance(PerformanceConfig {
                max_cpu_usage: 150, // Invalid
                ..PerformanceConfig::default()
            })
            .build();

        assert!(result.is_err());
    }
}
