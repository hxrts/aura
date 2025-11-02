//! Unified configuration framework for simulation components
//!
//! This module provides a hierarchical configuration system that eliminates
//! duplication across simulation components while providing type-safe construction
//! and validation.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod builder;
pub mod traits;

pub use builder::ConfigBuilder;
pub use traits::{ConfigDefaults, ConfigMerge, ConfigValidation};

/// Unified simulation configuration with hierarchical composition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct SimulationConfig {
    /// Core simulation parameters
    pub simulation: SimulationCoreConfig,
    /// Property monitoring configuration
    pub property_monitoring: PropertyMonitoringConfig,
    /// Performance and resource limits
    pub performance: PerformanceConfig,
    /// Network simulation configuration
    pub network: NetworkConfig,
    /// Scenario-specific configuration
    pub scenario: ScenarioConfig,
}

/// Core simulation execution parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationCoreConfig {
    /// Maximum simulation ticks
    pub max_ticks: u64,
    /// Maximum simulation time in milliseconds
    pub max_time_ms: u64,
    /// Time advancement per tick in milliseconds
    pub tick_duration_ms: u64,
    /// Random seed for deterministic execution
    pub seed: u64,
    /// Scenario name if loaded from file
    pub scenario_name: Option<String>,
    /// Whether to enable debug logging
    pub debug_logging: bool,
}

/// Property monitoring and evaluation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyMonitoringConfig {
    /// Maximum execution trace length
    pub max_trace_length: usize,
    /// Property evaluation timeout in milliseconds
    pub evaluation_timeout_ms: u64,
    /// Whether to enable parallel property evaluation
    pub parallel_evaluation: bool,
    /// Properties to monitor during simulation
    pub properties: Vec<String>,
    /// Minimum confidence threshold for violations
    pub violation_confidence_threshold: f64,
    /// Whether to stop on first violation
    pub stop_on_violation: bool,
}

/// Performance limits and resource configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Maximum memory usage in bytes (0 = unlimited)
    pub max_memory_bytes: u64,
    /// Maximum CPU utilization (0.0 to 1.0)
    pub max_cpu_utilization: f64,
    /// Checkpoint creation interval (0 = disabled)
    pub checkpoint_interval_ticks: u64,
    /// Maximum number of checkpoints to keep
    pub max_checkpoints: usize,
    /// Whether to enable performance monitoring
    pub enable_monitoring: bool,
    /// Metrics collection interval in ticks
    pub metrics_interval_ticks: u64,
}

/// Network simulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Message drop probability (0.0 to 1.0)
    pub drop_rate: f64,
    /// Base latency range (min_ms, max_ms)
    pub latency_range_ms: (u64, u64),
    /// Maximum additional jitter in milliseconds
    pub jitter_ms: u64,
    /// Bandwidth limits per participant (bytes per tick)
    pub bandwidth_limits: HashMap<String, u64>,
    /// Whether to enable network partitions
    pub enable_partitions: bool,
    /// Default partition duration in ticks
    pub default_partition_duration: u64,
}

/// Scenario-specific configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct ScenarioConfig {
    /// Scenario file path if loaded
    pub scenario_file: Option<String>,
    /// Custom scenario parameters
    pub parameters: HashMap<String, String>,
    /// Expected participants count
    pub expected_participants: Option<usize>,
    /// Protocol types to execute
    pub protocols: Vec<String>,
    /// Byzantine participant configuration
    pub byzantine_config: ByzantineConfig,
}

/// Byzantine adversary configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ByzantineConfig {
    /// Maximum fraction of byzantine participants (0.0 to 1.0)
    pub max_byzantine_fraction: f64,
    /// Default byzantine strategies to enable
    pub default_strategies: Vec<String>,
    /// Strategy-specific parameters
    pub strategy_parameters: HashMap<String, HashMap<String, String>>,
    /// Whether to enable adaptive byzantine behavior
    pub adaptive_behavior: bool,
}


impl Default for SimulationCoreConfig {
    fn default() -> Self {
        Self {
            max_ticks: 10000,
            max_time_ms: 60000,    // 60 seconds
            tick_duration_ms: 100, // 100ms per tick
            seed: 42,
            scenario_name: None,
            debug_logging: false,
        }
    }
}

impl Default for PropertyMonitoringConfig {
    fn default() -> Self {
        Self {
            max_trace_length: 1000,
            evaluation_timeout_ms: 5000, // 5 seconds
            parallel_evaluation: true,
            properties: Vec::new(),
            violation_confidence_threshold: 0.8,
            stop_on_violation: false,
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            max_memory_bytes: 0,      // Unlimited
            max_cpu_utilization: 0.8, // 80%
            checkpoint_interval_ticks: 100,
            max_checkpoints: 100,
            enable_monitoring: true,
            metrics_interval_ticks: 10,
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            drop_rate: 0.0,
            latency_range_ms: (10, 100),
            jitter_ms: 10,
            bandwidth_limits: HashMap::new(),
            enable_partitions: false,
            default_partition_duration: 100,
        }
    }
}


impl Default for ByzantineConfig {
    fn default() -> Self {
        Self {
            max_byzantine_fraction: 0.33, // Standard 1/3 threshold
            default_strategies: Vec::new(),
            strategy_parameters: HashMap::new(),
            adaptive_behavior: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_creation() {
        let config = SimulationConfig::default();
        assert_eq!(config.simulation.max_ticks, 10000);
        assert_eq!(config.simulation.max_time_ms, 60000);
        assert_eq!(config.simulation.tick_duration_ms, 100);
        assert_eq!(config.simulation.seed, 42);
        assert!(!config.simulation.debug_logging);
    }

    #[test]
    fn test_property_monitoring_defaults() {
        let config = PropertyMonitoringConfig::default();
        assert_eq!(config.max_trace_length, 1000);
        assert_eq!(config.evaluation_timeout_ms, 5000);
        assert!(config.parallel_evaluation);
        assert_eq!(config.violation_confidence_threshold, 0.8);
        assert!(!config.stop_on_violation);
    }

    #[test]
    fn test_network_config_defaults() {
        let config = NetworkConfig::default();
        assert_eq!(config.drop_rate, 0.0);
        assert_eq!(config.latency_range_ms, (10, 100));
        assert_eq!(config.jitter_ms, 10);
        assert!(!config.enable_partitions);
    }

    #[test]
    fn test_byzantine_config_defaults() {
        let config = ByzantineConfig::default();
        assert_eq!(config.max_byzantine_fraction, 0.33);
        assert!(config.default_strategies.is_empty());
        assert!(!config.adaptive_behavior);
    }
}
