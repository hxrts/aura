//! Configuration traits for validation, merging, and defaults

use super::*;
use crate::{AuraError, Result};

/// Trait for configuration validation
pub trait ConfigValidation {
    /// Validate configuration parameters
    fn validate(&self) -> Result<()>;
}

/// Trait for configuration merging
pub trait ConfigMerge<T> {
    /// Merge this configuration with another, preferring other's values
    fn merge_with(&mut self, other: &T);
}

/// Trait for providing configuration defaults
pub trait ConfigDefaults {
    /// Get default configuration for testing
    fn testing_defaults() -> Self;
    /// Get default configuration for production
    fn production_defaults() -> Self;
}

impl ConfigValidation for SimulationConfig {
    fn validate(&self) -> Result<()> {
        self.simulation.validate()?;
        self.property_monitoring.validate()?;
        self.performance.validate()?;
        self.network.validate()?;
        self.scenario.validate()?;
        Ok(())
    }
}

impl ConfigValidation for SimulationCoreConfig {
    fn validate(&self) -> Result<()> {
        if self.max_ticks == 0 {
            return Err(AuraError::configuration_error(
                "max_ticks must be greater than 0",
            ));
        }
        if self.max_time_ms == 0 {
            return Err(AuraError::configuration_error(
                "max_time_ms must be greater than 0",
            ));
        }
        if self.tick_duration_ms == 0 {
            return Err(AuraError::configuration_error(
                "tick_duration_ms must be greater than 0",
            ));
        }
        Ok(())
    }
}

impl ConfigValidation for PropertyMonitoringConfig {
    fn validate(&self) -> Result<()> {
        if self.max_trace_length == 0 {
            return Err(AuraError::configuration_error(
                "max_trace_length must be greater than 0",
            ));
        }
        if self.evaluation_timeout_ms == 0 {
            return Err(AuraError::configuration_error(
                "evaluation_timeout_ms must be greater than 0",
            ));
        }
        if !(0.0..=1.0).contains(&self.violation_confidence_threshold) {
            return Err(AuraError::configuration_error(
                "violation_confidence_threshold must be between 0.0 and 1.0",
            ));
        }
        Ok(())
    }
}

impl ConfigValidation for PerformanceConfig {
    fn validate(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.max_cpu_utilization) {
            return Err(AuraError::configuration_error(
                "max_cpu_utilization must be between 0.0 and 1.0",
            ));
        }
        if self.metrics_interval_ticks == 0 {
            return Err(AuraError::configuration_error(
                "metrics_interval_ticks must be greater than 0",
            ));
        }
        Ok(())
    }
}

impl ConfigValidation for NetworkConfig {
    fn validate(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.drop_rate) {
            return Err(AuraError::configuration_error(
                "drop_rate must be between 0.0 and 1.0",
            ));
        }
        if self.latency_range_ms.0 > self.latency_range_ms.1 {
            return Err(AuraError::configuration_error(
                "latency_range_ms min must be <= max",
            ));
        }
        if self.default_partition_duration == 0 {
            return Err(AuraError::configuration_error(
                "default_partition_duration must be greater than 0",
            ));
        }
        Ok(())
    }
}

impl ConfigValidation for ScenarioConfig {
    fn validate(&self) -> Result<()> {
        self.byzantine_config.validate()?;
        if let Some(expected) = self.expected_participants {
            if expected == 0 {
                return Err(AuraError::configuration_error(
                    "expected_participants must be greater than 0",
                ));
            }
        }
        Ok(())
    }
}

impl ConfigValidation for ByzantineConfig {
    fn validate(&self) -> Result<()> {
        if !(0.0..=1.0).contains(&self.max_byzantine_fraction) {
            return Err(AuraError::configuration_error(
                "max_byzantine_fraction must be between 0.0 and 1.0",
            ));
        }
        Ok(())
    }
}

impl ConfigMerge<SimulationConfig> for SimulationConfig {
    fn merge_with(&mut self, other: &SimulationConfig) {
        self.simulation.merge_with(&other.simulation);
        self.property_monitoring
            .merge_with(&other.property_monitoring);
        self.performance.merge_with(&other.performance);
        self.network.merge_with(&other.network);
        self.scenario.merge_with(&other.scenario);
    }
}

impl ConfigMerge<SimulationCoreConfig> for SimulationCoreConfig {
    fn merge_with(&mut self, other: &SimulationCoreConfig) {
        if other.max_ticks != SimulationCoreConfig::default().max_ticks {
            self.max_ticks = other.max_ticks;
        }
        if other.max_time_ms != SimulationCoreConfig::default().max_time_ms {
            self.max_time_ms = other.max_time_ms;
        }
        if other.tick_duration_ms != SimulationCoreConfig::default().tick_duration_ms {
            self.tick_duration_ms = other.tick_duration_ms;
        }
        if other.seed != SimulationCoreConfig::default().seed {
            self.seed = other.seed;
        }
        if other.scenario_name.is_some() {
            self.scenario_name = other.scenario_name.clone();
        }
        if other.debug_logging != SimulationCoreConfig::default().debug_logging {
            self.debug_logging = other.debug_logging;
        }
    }
}

impl ConfigMerge<PropertyMonitoringConfig> for PropertyMonitoringConfig {
    fn merge_with(&mut self, other: &PropertyMonitoringConfig) {
        if other.max_trace_length != PropertyMonitoringConfig::default().max_trace_length {
            self.max_trace_length = other.max_trace_length;
        }
        if other.evaluation_timeout_ms != PropertyMonitoringConfig::default().evaluation_timeout_ms
        {
            self.evaluation_timeout_ms = other.evaluation_timeout_ms;
        }
        if other.parallel_evaluation != PropertyMonitoringConfig::default().parallel_evaluation {
            self.parallel_evaluation = other.parallel_evaluation;
        }
        if !other.properties.is_empty() {
            self.properties = other.properties.clone();
        }
        if other.violation_confidence_threshold
            != PropertyMonitoringConfig::default().violation_confidence_threshold
        {
            self.violation_confidence_threshold = other.violation_confidence_threshold;
        }
        if other.stop_on_violation != PropertyMonitoringConfig::default().stop_on_violation {
            self.stop_on_violation = other.stop_on_violation;
        }
    }
}

impl ConfigMerge<PerformanceConfig> for PerformanceConfig {
    fn merge_with(&mut self, other: &PerformanceConfig) {
        if other.max_memory_bytes != PerformanceConfig::default().max_memory_bytes {
            self.max_memory_bytes = other.max_memory_bytes;
        }
        if other.max_cpu_utilization != PerformanceConfig::default().max_cpu_utilization {
            self.max_cpu_utilization = other.max_cpu_utilization;
        }
        if other.checkpoint_interval_ticks != PerformanceConfig::default().checkpoint_interval_ticks
        {
            self.checkpoint_interval_ticks = other.checkpoint_interval_ticks;
        }
        if other.max_checkpoints != PerformanceConfig::default().max_checkpoints {
            self.max_checkpoints = other.max_checkpoints;
        }
        if other.enable_monitoring != PerformanceConfig::default().enable_monitoring {
            self.enable_monitoring = other.enable_monitoring;
        }
        if other.metrics_interval_ticks != PerformanceConfig::default().metrics_interval_ticks {
            self.metrics_interval_ticks = other.metrics_interval_ticks;
        }
    }
}

impl ConfigMerge<NetworkConfig> for NetworkConfig {
    fn merge_with(&mut self, other: &NetworkConfig) {
        if other.drop_rate != NetworkConfig::default().drop_rate {
            self.drop_rate = other.drop_rate;
        }
        if other.latency_range_ms != NetworkConfig::default().latency_range_ms {
            self.latency_range_ms = other.latency_range_ms;
        }
        if other.jitter_ms != NetworkConfig::default().jitter_ms {
            self.jitter_ms = other.jitter_ms;
        }
        if !other.bandwidth_limits.is_empty() {
            self.bandwidth_limits = other.bandwidth_limits.clone();
        }
        if other.enable_partitions != NetworkConfig::default().enable_partitions {
            self.enable_partitions = other.enable_partitions;
        }
        if other.default_partition_duration != NetworkConfig::default().default_partition_duration {
            self.default_partition_duration = other.default_partition_duration;
        }
    }
}

impl ConfigMerge<ScenarioConfig> for ScenarioConfig {
    fn merge_with(&mut self, other: &ScenarioConfig) {
        if other.scenario_file.is_some() {
            self.scenario_file = other.scenario_file.clone();
        }
        if !other.parameters.is_empty() {
            self.parameters = other.parameters.clone();
        }
        if other.expected_participants.is_some() {
            self.expected_participants = other.expected_participants;
        }
        if !other.protocols.is_empty() {
            self.protocols = other.protocols.clone();
        }
        self.byzantine_config.merge_with(&other.byzantine_config);
    }
}

impl ConfigMerge<ByzantineConfig> for ByzantineConfig {
    fn merge_with(&mut self, other: &ByzantineConfig) {
        if other.max_byzantine_fraction != ByzantineConfig::default().max_byzantine_fraction {
            self.max_byzantine_fraction = other.max_byzantine_fraction;
        }
        if !other.default_strategies.is_empty() {
            self.default_strategies = other.default_strategies.clone();
        }
        if !other.strategy_parameters.is_empty() {
            self.strategy_parameters = other.strategy_parameters.clone();
        }
        if other.adaptive_behavior != ByzantineConfig::default().adaptive_behavior {
            self.adaptive_behavior = other.adaptive_behavior;
        }
    }
}

impl ConfigDefaults for SimulationConfig {
    fn testing_defaults() -> Self {
        Self {
            simulation: SimulationCoreConfig {
                max_ticks: 1000,
                max_time_ms: 10000,   // 10 seconds for tests
                tick_duration_ms: 10, // Fast ticks for tests
                seed: 42,
                scenario_name: Some("test_scenario".to_string()),
                debug_logging: true,
            },
            property_monitoring: PropertyMonitoringConfig {
                max_trace_length: 100,
                evaluation_timeout_ms: 1000, // 1 second for tests
                parallel_evaluation: false,  // Deterministic for tests
                properties: vec!["validCounts".to_string(), "sessionLimit".to_string()],
                violation_confidence_threshold: 0.7,
                stop_on_violation: true,
            },
            performance: PerformanceConfig {
                max_memory_bytes: 100_000_000, // 100MB limit for tests
                max_cpu_utilization: 1.0,      // No CPU limits in tests
                checkpoint_interval_ticks: 10,
                max_checkpoints: 10,
                enable_monitoring: false, // Reduce overhead in tests
                metrics_interval_ticks: 5,
            },
            network: NetworkConfig {
                drop_rate: 0.0,           // No drops in tests by default
                latency_range_ms: (1, 5), // Low latency for tests
                jitter_ms: 0,             // No jitter for deterministic tests
                bandwidth_limits: HashMap::new(),
                enable_partitions: false,
                default_partition_duration: 10,
            },
            scenario: ScenarioConfig {
                scenario_file: None,
                parameters: HashMap::new(),
                expected_participants: Some(3), // Small test group
                protocols: vec!["test_protocol".to_string()],
                byzantine_config: ByzantineConfig {
                    max_byzantine_fraction: 0.0, // No byzantine in basic tests
                    default_strategies: Vec::new(),
                    strategy_parameters: HashMap::new(),
                    adaptive_behavior: false,
                },
            },
        }
    }

    fn production_defaults() -> Self {
        Self {
            simulation: SimulationCoreConfig {
                max_ticks: 100000,
                max_time_ms: 600000, // 10 minutes
                tick_duration_ms: 100,
                seed: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("System time before UNIX epoch")
                    .as_secs(),
                scenario_name: None,
                debug_logging: false,
            },
            property_monitoring: PropertyMonitoringConfig {
                max_trace_length: 10000,
                evaluation_timeout_ms: 30000, // 30 seconds
                parallel_evaluation: true,
                properties: Vec::new(),
                violation_confidence_threshold: 0.9,
                stop_on_violation: false,
            },
            performance: PerformanceConfig {
                max_memory_bytes: 0, // Unlimited
                max_cpu_utilization: 0.8,
                checkpoint_interval_ticks: 1000,
                max_checkpoints: 50,
                enable_monitoring: true,
                metrics_interval_ticks: 100,
            },
            network: NetworkConfig {
                drop_rate: 0.01,             // 1% drop rate for realism
                latency_range_ms: (50, 500), // Realistic internet latency
                jitter_ms: 50,
                bandwidth_limits: HashMap::new(),
                enable_partitions: true,
                default_partition_duration: 1000,
            },
            scenario: ScenarioConfig {
                scenario_file: None,
                parameters: HashMap::new(),
                expected_participants: None,
                protocols: Vec::new(),
                byzantine_config: ByzantineConfig {
                    max_byzantine_fraction: 0.33,
                    default_strategies: Vec::new(),
                    strategy_parameters: HashMap::new(),
                    adaptive_behavior: true,
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation() {
        let config = SimulationConfig::default();
        assert!(config.validate().is_ok());

        let mut invalid_config = SimulationConfig::default();
        invalid_config.simulation.max_ticks = 0;
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_config_merging() {
        let mut base = SimulationConfig::default();
        let override_config = SimulationConfig {
            simulation: SimulationCoreConfig {
                seed: 999,
                debug_logging: true,
                ..Default::default()
            },
            ..Default::default()
        };

        base.merge_with(&override_config);
        assert_eq!(base.simulation.seed, 999);
        assert!(base.simulation.debug_logging);
        // Other values should remain default
        assert_eq!(base.simulation.max_ticks, 10000);
    }

    #[test]
    fn test_testing_defaults() {
        let config = SimulationConfig::testing_defaults();
        assert_eq!(config.simulation.max_ticks, 1000);
        assert_eq!(config.simulation.tick_duration_ms, 10);
        assert!(config.simulation.debug_logging);
        assert_eq!(config.property_monitoring.max_trace_length, 100);
        assert!(!config.property_monitoring.parallel_evaluation);
        assert_eq!(config.network.drop_rate, 0.0);
    }

    #[test]
    fn test_production_defaults() {
        let config = SimulationConfig::production_defaults();
        assert_eq!(config.simulation.max_ticks, 100000);
        assert_eq!(config.simulation.tick_duration_ms, 100);
        assert!(!config.simulation.debug_logging);
        assert_eq!(config.property_monitoring.max_trace_length, 10000);
        assert!(config.property_monitoring.parallel_evaluation);
        assert_eq!(config.network.drop_rate, 0.01);
    }
}
