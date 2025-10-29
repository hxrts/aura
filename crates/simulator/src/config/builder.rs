//! Configuration builder for type-safe construction

use super::*;
use crate::Result;

/// Type-safe builder for simulation configuration
pub struct ConfigBuilder {
    config: SimulationConfig,
}

impl ConfigBuilder {
    /// Create a new configuration builder with defaults
    pub fn new() -> Self {
        Self {
            config: SimulationConfig::default(),
        }
    }

    /// Set the random seed for deterministic execution
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.config.simulation.seed = seed;
        self
    }

    /// Set maximum simulation ticks
    pub fn with_max_ticks(mut self, max_ticks: u64) -> Self {
        self.config.simulation.max_ticks = max_ticks;
        self
    }

    /// Set maximum simulation time in milliseconds
    pub fn with_max_time_ms(mut self, max_time_ms: u64) -> Self {
        self.config.simulation.max_time_ms = max_time_ms;
        self
    }

    /// Set tick duration in milliseconds
    pub fn with_tick_duration_ms(mut self, tick_duration_ms: u64) -> Self {
        self.config.simulation.tick_duration_ms = tick_duration_ms;
        self
    }

    /// Set scenario name
    pub fn with_scenario_name<S: Into<String>>(mut self, name: S) -> Self {
        self.config.simulation.scenario_name = Some(name.into());
        self
    }

    /// Enable debug logging
    pub fn with_debug_logging(mut self, enabled: bool) -> Self {
        self.config.simulation.debug_logging = enabled;
        self
    }

    /// Set maximum trace length for property monitoring
    pub fn with_max_trace_length(mut self, max_trace_length: usize) -> Self {
        self.config.property_monitoring.max_trace_length = max_trace_length;
        self
    }

    /// Set property evaluation timeout
    pub fn with_evaluation_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.config.property_monitoring.evaluation_timeout_ms = timeout_ms;
        self
    }

    /// Enable or disable parallel property evaluation
    pub fn with_parallel_evaluation(mut self, enabled: bool) -> Self {
        self.config.property_monitoring.parallel_evaluation = enabled;
        self
    }

    /// Add properties to monitor
    pub fn with_properties<I, S>(mut self, properties: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.config.property_monitoring.properties =
            properties.into_iter().map(|s| s.into()).collect();
        self
    }

    /// Set violation confidence threshold
    pub fn with_violation_threshold(mut self, threshold: f64) -> Self {
        self.config
            .property_monitoring
            .violation_confidence_threshold = threshold;
        self
    }

    /// Enable stopping on first violation
    pub fn with_stop_on_violation(mut self, enabled: bool) -> Self {
        self.config.property_monitoring.stop_on_violation = enabled;
        self
    }

    /// Set checkpoint interval
    pub fn with_checkpoint_interval(mut self, interval_ticks: u64) -> Self {
        self.config.performance.checkpoint_interval_ticks = interval_ticks;
        self
    }

    /// Set maximum number of checkpoints
    pub fn with_max_checkpoints(mut self, max_checkpoints: usize) -> Self {
        self.config.performance.max_checkpoints = max_checkpoints;
        self
    }

    /// Enable performance monitoring
    pub fn with_performance_monitoring(mut self, enabled: bool) -> Self {
        self.config.performance.enable_monitoring = enabled;
        self
    }

    /// Set network drop rate
    pub fn with_drop_rate(mut self, drop_rate: f64) -> Self {
        self.config.network.drop_rate = drop_rate;
        self
    }

    /// Set network latency range
    pub fn with_latency_range_ms(mut self, min_ms: u64, max_ms: u64) -> Self {
        self.config.network.latency_range_ms = (min_ms, max_ms);
        self
    }

    /// Set network jitter
    pub fn with_jitter_ms(mut self, jitter_ms: u64) -> Self {
        self.config.network.jitter_ms = jitter_ms;
        self
    }

    /// Enable network partitions
    pub fn with_network_partitions(mut self, enabled: bool) -> Self {
        self.config.network.enable_partitions = enabled;
        self
    }

    /// Set expected participants count
    pub fn with_expected_participants(mut self, count: usize) -> Self {
        self.config.scenario.expected_participants = Some(count);
        self
    }

    /// Add protocols to execute
    pub fn with_protocols<I, S>(mut self, protocols: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.config.scenario.protocols = protocols.into_iter().map(|s| s.into()).collect();
        self
    }

    /// Set Byzantine participants fraction
    pub fn with_byzantine_fraction(mut self, fraction: f64) -> Self {
        self.config.scenario.byzantine_config.max_byzantine_fraction = fraction;
        self
    }

    /// Add Byzantine strategies
    pub fn with_byzantine_strategies<I, S>(mut self, strategies: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.config.scenario.byzantine_config.default_strategies =
            strategies.into_iter().map(|s| s.into()).collect();
        self
    }

    /// Add scenario parameters
    pub fn with_scenario_parameters<I, K, V>(mut self, parameters: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.config.scenario.parameters = parameters
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        self
    }

    /// Build the configuration with validation
    pub fn build(self) -> Result<SimulationConfig> {
        self.config.validate()?;
        Ok(self.config)
    }

    /// Build the configuration without validation (for testing)
    pub fn build_unchecked(self) -> SimulationConfig {
        self.config
    }
}

impl Default for ConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builder_basic_construction() {
        let config = ConfigBuilder::new()
            .with_seed(123)
            .with_max_ticks(5000)
            .with_scenario_name("test_scenario")
            .build_unchecked();

        assert_eq!(config.simulation.seed, 123);
        assert_eq!(config.simulation.max_ticks, 5000);
        assert_eq!(
            config.simulation.scenario_name,
            Some("test_scenario".to_string())
        );
    }

    #[test]
    fn test_builder_property_monitoring() {
        let config = ConfigBuilder::new()
            .with_max_trace_length(2000)
            .with_evaluation_timeout_ms(10000)
            .with_properties(vec!["prop1", "prop2"])
            .with_violation_threshold(0.9)
            .build_unchecked();

        assert_eq!(config.property_monitoring.max_trace_length, 2000);
        assert_eq!(config.property_monitoring.evaluation_timeout_ms, 10000);
        assert_eq!(
            config.property_monitoring.properties,
            vec!["prop1", "prop2"]
        );
        assert_eq!(
            config.property_monitoring.violation_confidence_threshold,
            0.9
        );
    }

    #[test]
    fn test_builder_network_config() {
        let config = ConfigBuilder::new()
            .with_drop_rate(0.1)
            .with_latency_range_ms(50, 200)
            .with_jitter_ms(20)
            .with_network_partitions(true)
            .build_unchecked();

        assert_eq!(config.network.drop_rate, 0.1);
        assert_eq!(config.network.latency_range_ms, (50, 200));
        assert_eq!(config.network.jitter_ms, 20);
        assert!(config.network.enable_partitions);
    }

    #[test]
    fn test_builder_byzantine_config() {
        let config = ConfigBuilder::new()
            .with_byzantine_fraction(0.25)
            .with_byzantine_strategies(vec!["drop_messages", "delay_messages"])
            .build_unchecked();

        assert_eq!(
            config.scenario.byzantine_config.max_byzantine_fraction,
            0.25
        );
        assert_eq!(
            config.scenario.byzantine_config.default_strategies,
            vec!["drop_messages", "delay_messages"]
        );
    }

    #[test]
    fn test_builder_scenario_parameters() {
        let params = vec![("key1", "value1"), ("key2", "value2")];
        let config = ConfigBuilder::new()
            .with_scenario_parameters(params)
            .with_expected_participants(5)
            .with_protocols(vec!["dkg", "signing"])
            .build_unchecked();

        assert_eq!(config.scenario.parameters.len(), 2);
        assert_eq!(
            config.scenario.parameters.get("key1"),
            Some(&"value1".to_string())
        );
        assert_eq!(config.scenario.expected_participants, Some(5));
        assert_eq!(config.scenario.protocols, vec!["dkg", "signing"]);
    }
}
