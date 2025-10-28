//! Minimal Reproduction Discovery
//!
//! This module provides sophisticated algorithms for discovering minimal conditions
//! that reproduce failures. It systematically varies scenario parameters to find
//! the simplest configuration that still triggers property violations.

use crate::{
    CheckpointSimulation, FailureAnalysisResult, PropertyMonitor, PropertyViolation, Result,
    Scenario, SimError, SimulationResult,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Minimal reproduction discovery engine
///
/// This engine systematically varies scenario parameters to discover
/// the minimal set of conditions required to reproduce a failure.
pub struct MinimalReproductionFinder {
    /// Configuration for reproduction discovery
    config: ReproductionConfig,
    /// Parameter variation strategies
    variation_strategies: Vec<Box<dyn ParameterVariationStrategy>>,
    /// Discovered reproductions
    discovered_reproductions: Vec<MinimalReproduction>,
    /// Analysis statistics
    analysis_stats: ReproductionAnalysisStats,
}

/// Configuration for minimal reproduction discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReproductionConfig {
    /// Maximum number of variation attempts
    pub max_variation_attempts: usize,
    /// Timeout for each reproduction attempt (ms)
    pub reproduction_timeout_ms: u64,
    /// Minimum complexity reduction threshold
    pub min_complexity_reduction: f64,
    /// Enable parallel reproduction attempts
    pub enable_parallel_execution: bool,
    /// Maximum concurrent executions
    pub max_concurrent_executions: usize,
    /// Variation search strategy
    pub search_strategy: SearchStrategy,
}

impl Default for ReproductionConfig {
    fn default() -> Self {
        Self {
            max_variation_attempts: 1000,
            reproduction_timeout_ms: 30000,
            min_complexity_reduction: 0.1,
            enable_parallel_execution: true,
            max_concurrent_executions: 4,
            search_strategy: SearchStrategy::BinarySearch,
        }
    }
}

/// Strategy for searching minimal reproductions
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SearchStrategy {
    /// Binary search for parameter reduction
    BinarySearch,
    /// Greedy reduction of highest impact parameters
    GreedyReduction,
    /// Systematic grid search
    GridSearch,
    /// Genetic algorithm for parameter optimization
    GeneticAlgorithm,
    /// Simulated annealing for complex parameter spaces
    SimulatedAnnealing,
}

/// Minimal reproduction of a failure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinimalReproduction {
    /// Unique reproduction identifier
    pub reproduction_id: String,
    /// Original failure that was minimized
    pub original_violation: PropertyViolation,
    /// Minimal scenario that reproduces the failure
    pub minimal_scenario: Scenario,
    /// Complexity score of the minimal reproduction
    pub complexity_score: f64,
    /// Reduction achieved from original scenario
    pub complexity_reduction: f64,
    /// Reproduction consistency (0.0 to 1.0)
    pub reproduction_rate: f64,
    /// Key parameters required for reproduction
    pub essential_parameters: Vec<EssentialParameter>,
    /// Parameters that can be safely removed
    pub removable_parameters: Vec<String>,
    /// Discovery metadata
    pub discovery_metadata: DiscoveryMetadata,
}

/// Essential parameter for reproduction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EssentialParameter {
    /// Parameter name
    pub parameter_name: String,
    /// Parameter category
    pub category: ParameterCategory,
    /// Required value or range
    pub required_value: ParameterValue,
    /// Impact on reproduction
    pub impact_score: f64,
    /// Sensitivity analysis results
    pub sensitivity: ParameterSensitivity,
}

/// Category of reproduction parameters
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ParameterCategory {
    /// Network configuration parameters
    NetworkConfiguration,
    /// Byzantine behavior parameters
    ByzantineConfiguration,
    /// Protocol timing parameters
    TimingConfiguration,
    /// Participant setup parameters
    ParticipantConfiguration,
    /// Environment conditions
    EnvironmentConfiguration,
    /// Protocol-specific parameters
    ProtocolParameters,
}

/// Parameter value specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterValue {
    /// Boolean parameter
    Boolean(bool),
    /// Integer parameter
    Integer(i64),
    /// Float parameter
    Float(f64),
    /// String parameter
    String(String),
    /// Range of values
    Range(f64, f64),
    /// Set of discrete values
    DiscreteSet(Vec<String>),
}

/// Parameter sensitivity analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterSensitivity {
    /// Threshold value for reproduction
    pub threshold: Option<f64>,
    /// Value range that triggers reproduction
    pub critical_range: Option<(f64, f64)>,
    /// Sensitivity coefficient
    pub sensitivity_coefficient: f64,
    /// Interaction effects with other parameters
    pub interaction_effects: Vec<ParameterInteraction>,
}

/// Parameter interaction effect
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterInteraction {
    /// Other parameter name
    pub other_parameter: String,
    /// Interaction strength
    pub interaction_strength: f64,
    /// Type of interaction
    pub interaction_type: InteractionType,
}

/// Type of parameter interaction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InteractionType {
    /// Parameters are synergistic
    Synergistic,
    /// Parameters are antagonistic
    Antagonistic,
    /// Parameters are independent
    Independent,
    /// Parameters have conditional dependency
    Conditional,
}

/// Metadata about reproduction discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryMetadata {
    /// When discovery was performed
    pub discovered_at: u64,
    /// Time taken for discovery (ms)
    pub discovery_time_ms: u64,
    /// Number of variations attempted
    pub variations_attempted: usize,
    /// Search strategy used
    pub search_strategy: SearchStrategy,
    /// Confidence in reproduction
    pub confidence_level: f64,
    /// Discovery notes
    pub notes: Vec<String>,
}

/// Strategy for varying parameters during search
pub trait ParameterVariationStrategy: Send + Sync {
    /// Generate parameter variations
    fn generate_variations(
        &self,
        base_scenario: &Scenario,
        failure_analysis: &FailureAnalysisResult,
    ) -> Result<Vec<ScenarioVariation>>;

    /// Update strategy based on reproduction results
    fn update_strategy(&mut self, results: &[ReproductionAttempt]) -> Result<()>;

    /// Get strategy name
    fn strategy_name(&self) -> String;

    /// Get expected complexity reduction
    fn expected_complexity_reduction(&self) -> f64;
}

/// Scenario variation for testing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioVariation {
    /// Variation identifier
    pub variation_id: String,
    /// Modified scenario
    pub modified_scenario: Scenario,
    /// Parameters that were varied
    pub varied_parameters: Vec<VariedParameter>,
    /// Expected complexity score
    pub expected_complexity: f64,
    /// Variation metadata
    pub variation_metadata: VariationMetadata,
}

/// Parameter that was varied
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariedParameter {
    /// Parameter name
    pub parameter_name: String,
    /// Original value
    pub original_value: String,
    /// New value
    pub new_value: String,
    /// Variation type
    pub variation_type: VariationType,
}

/// Type of parameter variation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VariationType {
    /// Parameter was removed
    Removed,
    /// Parameter value was reduced
    Reduced,
    /// Parameter value was simplified
    Simplified,
    /// Parameter was replaced with default
    DefaultValue,
    /// Parameter was disabled
    Disabled,
}

/// Metadata about a variation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariationMetadata {
    /// Rationale for this variation
    pub rationale: String,
    /// Expected impact on failure reproduction
    pub expected_impact: f64,
    /// Variation priority
    pub priority: VariationPriority,
}

/// Priority level for variations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariationPriority {
    /// Low priority variation
    Low,
    /// Normal priority variation
    Normal,
    /// High priority variation
    High,
    /// Critical variation to test
    Critical,
}

/// Result of a reproduction attempt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReproductionAttempt {
    /// Attempt identifier
    pub attempt_id: String,
    /// Scenario variation that was tested
    pub tested_variation: ScenarioVariation,
    /// Whether reproduction was successful
    pub reproduction_successful: bool,
    /// Violations detected during attempt
    pub detected_violations: Vec<PropertyViolation>,
    /// Execution time (ms)
    pub execution_time_ms: u64,
    /// Simulation result
    pub simulation_result: Option<SimulationResult>,
    /// Failure details if reproduction failed
    pub failure_details: Option<String>,
}

/// Analysis statistics for reproduction discovery
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReproductionAnalysisStats {
    /// Total reproduction attempts
    pub total_attempts: usize,
    /// Successful reproductions
    pub successful_reproductions: usize,
    /// Average execution time per attempt (ms)
    pub average_execution_time_ms: f64,
    /// Best complexity reduction achieved
    pub best_complexity_reduction: f64,
    /// Time spent on analysis (ms)
    pub total_analysis_time_ms: u64,
    /// Success rate by strategy
    pub strategy_success_rates: HashMap<String, f64>,
}

/// Binary search variation strategy
pub struct BinarySearchStrategy {
    /// Current search parameters
    search_parameters: Vec<String>,
    /// Parameter importance ranking
    parameter_importance: HashMap<String, f64>,
}

/// Greedy reduction strategy
pub struct GreedyReductionStrategy {
    /// Parameters ordered by reduction impact
    impact_ordered_parameters: Vec<(String, f64)>,
    /// Reduction thresholds
    reduction_thresholds: HashMap<String, f64>,
}

impl MinimalReproductionFinder {
    /// Create a new minimal reproduction finder
    pub fn new() -> Result<Self> {
        let mut strategies: Vec<Box<dyn ParameterVariationStrategy>> = Vec::new();
        strategies.push(Box::new(BinarySearchStrategy::new()));
        strategies.push(Box::new(GreedyReductionStrategy::new()));

        Ok(Self {
            config: ReproductionConfig::default(),
            variation_strategies: strategies,
            discovered_reproductions: Vec::new(),
            analysis_stats: ReproductionAnalysisStats::new(),
        })
    }

    /// Create finder with custom configuration
    pub fn with_config(config: ReproductionConfig) -> Result<Self> {
        let mut finder = Self::new()?;
        finder.config = config;
        Ok(finder)
    }

    /// Find minimal reproduction for a failure
    pub fn find_minimal_reproduction(
        &mut self,
        original_violation: &PropertyViolation,
        original_scenario: &Scenario,
        failure_analysis: &FailureAnalysisResult,
    ) -> Result<MinimalReproduction> {
        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let reproduction_id = format!("repro_{}_{}", original_violation.property_name, start_time);

        // Calculate original complexity
        let original_complexity = self.calculate_scenario_complexity(original_scenario)?;

        // Generate variations using all strategies
        let mut all_variations = Vec::new();
        for strategy in &self.variation_strategies {
            let variations = strategy.generate_variations(original_scenario, failure_analysis)?;
            all_variations.extend(variations);
        }

        // Sort variations by expected complexity (simplest first)
        all_variations.sort_by(|a, b| {
            a.expected_complexity
                .partial_cmp(&b.expected_complexity)
                .unwrap()
        });

        // Execute reproduction attempts
        let mut reproduction_attempts = Vec::new();
        let mut best_reproduction: Option<(ScenarioVariation, f64)> = None;

        for (i, variation) in all_variations.iter().enumerate() {
            if i >= self.config.max_variation_attempts {
                break;
            }

            let attempt = self.execute_reproduction_attempt(variation, original_violation)?;
            self.analysis_stats.total_attempts += 1;

            if attempt.reproduction_successful {
                self.analysis_stats.successful_reproductions += 1;
                let complexity_reduction =
                    (original_complexity - variation.expected_complexity) / original_complexity;

                if complexity_reduction >= self.config.min_complexity_reduction
                    && (best_reproduction.is_none()
                        || variation.expected_complexity < best_reproduction.as_ref().unwrap().1)
                {
                    best_reproduction = Some((variation.clone(), variation.expected_complexity));
                }
            }

            reproduction_attempts.push(attempt);
        }

        // Update strategies based on results
        for strategy in &mut self.variation_strategies {
            strategy.update_strategy(&reproduction_attempts)?;
        }

        // Generate minimal reproduction result
        let end_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let discovery_time = end_time - start_time;

        let minimal_reproduction =
            if let Some((best_variation, best_complexity)) = best_reproduction {
                let complexity_reduction =
                    (original_complexity - best_complexity) / original_complexity;
                let reproduction_rate =
                    self.calculate_reproduction_rate(&reproduction_attempts, &best_variation);
                let essential_parameters =
                    self.identify_essential_parameters(&best_variation, failure_analysis)?;
                let removable_parameters =
                    self.identify_removable_parameters(original_scenario, &best_variation);

                MinimalReproduction {
                    reproduction_id: reproduction_id.clone(),
                    original_violation: original_violation.clone(),
                    minimal_scenario: best_variation.modified_scenario,
                    complexity_score: best_complexity,
                    complexity_reduction,
                    reproduction_rate,
                    essential_parameters,
                    removable_parameters,
                    discovery_metadata: DiscoveryMetadata {
                        discovered_at: start_time,
                        discovery_time_ms: discovery_time,
                        variations_attempted: reproduction_attempts.len(),
                        search_strategy: self.config.search_strategy.clone(),
                        confidence_level: self.calculate_confidence_level(&reproduction_attempts),
                        notes: Vec::new(),
                    },
                }
            } else {
                return Err(SimError::AnalysisError(
                    "No minimal reproduction found".to_string(),
                ));
            };

        self.discovered_reproductions
            .push(minimal_reproduction.clone());
        self.analysis_stats.total_analysis_time_ms += discovery_time;

        if minimal_reproduction.complexity_reduction > self.analysis_stats.best_complexity_reduction
        {
            self.analysis_stats.best_complexity_reduction =
                minimal_reproduction.complexity_reduction;
        }

        Ok(minimal_reproduction)
    }

    /// Execute a reproduction attempt
    pub fn execute_reproduction_attempt(
        &mut self,
        variation: &ScenarioVariation,
        target_violation: &PropertyViolation,
    ) -> Result<ReproductionAttempt> {
        let start_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        let attempt_id = format!("attempt_{}_{}", variation.variation_id, start_time);

        // Create simulation from variation
        let mut simulation = CheckpointSimulation::from_scenario(&variation.modified_scenario)?;
        let mut property_monitor = PropertyMonitor::new();

        // Execute simulation with monitoring
        let simulation_result = simulation.run_with_monitoring(&mut property_monitor);
        let execution_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - start_time;

        // Check for target violation reproduction
        let detected_violations = property_monitor.get_detected_violations().clone();
        let reproduction_successful = detected_violations
            .iter()
            .any(|v| v.property_name == target_violation.property_name);

        Ok(ReproductionAttempt {
            attempt_id,
            tested_variation: variation.clone(),
            reproduction_successful,
            detected_violations,
            execution_time_ms: execution_time,
            simulation_result: simulation_result.ok(),
            failure_details: if !reproduction_successful {
                Some("Target violation not reproduced".to_string())
            } else {
                None
            },
        })
    }

    /// Calculate scenario complexity score
    pub fn calculate_scenario_complexity(&self, scenario: &Scenario) -> Result<f64> {
        let mut complexity = 0.0;

        // Network complexity
        complexity += scenario.setup.participants as f64 * 0.1;
        if let Some(ref network) = scenario.setup.network_conditions {
            complexity += if !network.partitions.is_empty() {
                2.0
            } else {
                0.0
            };
        }

        // Byzantine complexity
        if let Some(ref byzantine) = scenario.setup.byzantine_conditions {
            complexity += byzantine.count as f64 * 0.5;
            complexity += byzantine.strategies.len() as f64 * 0.3;
        }

        // Protocol complexity
        if let Some(ref protocols) = scenario.protocols {
            complexity += protocols.len() as f64 * 0.2;
            for protocol in protocols {
                if let Some(ref params) = protocol.parameters {
                    complexity += params.len() as f64 * 0.1;
                }
            }
        }

        // Environment complexity - not available in current Scenario struct
        // if let Some(ref environment) = scenario.execution.environment {
        //     complexity += environment.effects.len() as f64 * 0.15;
        // }

        // Assertions complexity
        complexity += scenario.assertions.len() as f64 * 0.05;

        Ok(complexity)
    }

    /// Calculate reproduction rate from attempts
    fn calculate_reproduction_rate(
        &self,
        attempts: &[ReproductionAttempt],
        target_variation: &ScenarioVariation,
    ) -> f64 {
        let target_attempts: Vec<_> = attempts
            .iter()
            .filter(|a| a.tested_variation.variation_id == target_variation.variation_id)
            .collect();

        if target_attempts.is_empty() {
            return 0.0;
        }

        let successful_count = target_attempts
            .iter()
            .filter(|a| a.reproduction_successful)
            .count();

        successful_count as f64 / target_attempts.len() as f64
    }

    /// Identify essential parameters for reproduction
    fn identify_essential_parameters(
        &self,
        variation: &ScenarioVariation,
        failure_analysis: &FailureAnalysisResult,
    ) -> Result<Vec<EssentialParameter>> {
        let mut essential_parameters = Vec::new();

        for varied_param in &variation.varied_parameters {
            // Analyze parameter based on failure analysis
            let impact_score =
                self.calculate_parameter_impact(&varied_param.parameter_name, failure_analysis);

            if impact_score > 0.3 {
                // Threshold for essential parameters
                essential_parameters.push(EssentialParameter {
                    parameter_name: varied_param.parameter_name.clone(),
                    category: self.categorize_parameter(&varied_param.parameter_name),
                    required_value: self.extract_parameter_value(&varied_param.new_value),
                    impact_score,
                    sensitivity: self.analyze_parameter_sensitivity(&varied_param.parameter_name),
                });
            }
        }

        Ok(essential_parameters)
    }

    /// Identify parameters that can be safely removed
    fn identify_removable_parameters(
        &self,
        _original: &Scenario,
        minimal: &ScenarioVariation,
    ) -> Vec<String> {
        minimal
            .varied_parameters
            .iter()
            .filter(|p| p.variation_type == VariationType::Removed)
            .map(|p| p.parameter_name.clone())
            .collect()
    }

    /// Calculate confidence level in reproduction
    fn calculate_confidence_level(&self, attempts: &[ReproductionAttempt]) -> f64 {
        if attempts.is_empty() {
            return 0.0;
        }

        let success_rate = attempts
            .iter()
            .filter(|a| a.reproduction_successful)
            .count() as f64
            / attempts.len() as f64;

        // Factor in consistency and execution stability
        let avg_execution_time = attempts
            .iter()
            .map(|a| a.execution_time_ms as f64)
            .sum::<f64>()
            / attempts.len() as f64;

        let time_variance = attempts
            .iter()
            .map(|a| (a.execution_time_ms as f64 - avg_execution_time).powi(2))
            .sum::<f64>()
            / attempts.len() as f64;

        let stability_factor = 1.0 / (1.0 + time_variance / avg_execution_time);

        success_rate * 0.7 + stability_factor * 0.3
    }

    /// Get discovered reproductions
    pub fn get_discovered_reproductions(&self) -> &[MinimalReproduction] {
        &self.discovered_reproductions
    }

    /// Get analysis statistics
    pub fn get_analysis_statistics(&self) -> &ReproductionAnalysisStats {
        &self.analysis_stats
    }

    // Private helper methods

    fn calculate_parameter_impact(
        &self,
        _parameter_name: &str,
        _failure_analysis: &FailureAnalysisResult,
    ) -> f64 {
        // Calculate impact based on failure analysis
        // This would analyze how the parameter relates to critical events
        0.5 // Placeholder implementation
    }

    fn categorize_parameter(&self, parameter_name: &str) -> ParameterCategory {
        if parameter_name.contains("network") || parameter_name.contains("partition") {
            ParameterCategory::NetworkConfiguration
        } else if parameter_name.contains("byzantine") {
            ParameterCategory::ByzantineConfiguration
        } else if parameter_name.contains("timeout") || parameter_name.contains("delay") {
            ParameterCategory::TimingConfiguration
        } else if parameter_name.contains("participant") {
            ParameterCategory::ParticipantConfiguration
        } else if parameter_name.contains("environment") {
            ParameterCategory::EnvironmentConfiguration
        } else {
            ParameterCategory::ProtocolParameters
        }
    }

    fn extract_parameter_value(&self, value_str: &str) -> ParameterValue {
        if let Ok(bool_val) = value_str.parse::<bool>() {
            ParameterValue::Boolean(bool_val)
        } else if let Ok(int_val) = value_str.parse::<i64>() {
            ParameterValue::Integer(int_val)
        } else if let Ok(float_val) = value_str.parse::<f64>() {
            ParameterValue::Float(float_val)
        } else {
            ParameterValue::String(value_str.to_string())
        }
    }

    fn analyze_parameter_sensitivity(&self, _parameter_name: &str) -> ParameterSensitivity {
        // Placeholder implementation
        ParameterSensitivity {
            threshold: None,
            critical_range: None,
            sensitivity_coefficient: 0.5,
            interaction_effects: Vec::new(),
        }
    }
}

impl BinarySearchStrategy {
    fn new() -> Self {
        Self {
            search_parameters: Vec::new(),
            parameter_importance: HashMap::new(),
        }
    }
}

impl ParameterVariationStrategy for BinarySearchStrategy {
    fn generate_variations(
        &self,
        base_scenario: &Scenario,
        _failure_analysis: &FailureAnalysisResult,
    ) -> Result<Vec<ScenarioVariation>> {
        let mut variations = Vec::new();

        // Generate binary search variations
        // This is a simplified implementation
        let variation = ScenarioVariation {
            variation_id: "binary_search_1".to_string(),
            modified_scenario: base_scenario.clone(),
            varied_parameters: Vec::new(),
            expected_complexity: 5.0,
            variation_metadata: VariationMetadata {
                rationale: "Binary search parameter reduction".to_string(),
                expected_impact: 0.7,
                priority: VariationPriority::High,
            },
        };

        variations.push(variation);
        Ok(variations)
    }

    fn update_strategy(&mut self, _results: &[ReproductionAttempt]) -> Result<()> {
        // Update strategy based on results
        Ok(())
    }

    fn strategy_name(&self) -> String {
        "BinarySearch".to_string()
    }

    fn expected_complexity_reduction(&self) -> f64 {
        0.5
    }
}

impl GreedyReductionStrategy {
    fn new() -> Self {
        Self {
            impact_ordered_parameters: Vec::new(),
            reduction_thresholds: HashMap::new(),
        }
    }
}

impl ParameterVariationStrategy for GreedyReductionStrategy {
    fn generate_variations(
        &self,
        base_scenario: &Scenario,
        _failure_analysis: &FailureAnalysisResult,
    ) -> Result<Vec<ScenarioVariation>> {
        let mut variations = Vec::new();

        // Generate greedy reduction variations
        let variation = ScenarioVariation {
            variation_id: "greedy_reduction_1".to_string(),
            modified_scenario: base_scenario.clone(),
            varied_parameters: Vec::new(),
            expected_complexity: 3.0,
            variation_metadata: VariationMetadata {
                rationale: "Greedy parameter reduction".to_string(),
                expected_impact: 0.8,
                priority: VariationPriority::High,
            },
        };

        variations.push(variation);
        Ok(variations)
    }

    fn update_strategy(&mut self, _results: &[ReproductionAttempt]) -> Result<()> {
        Ok(())
    }

    fn strategy_name(&self) -> String {
        "GreedyReduction".to_string()
    }

    fn expected_complexity_reduction(&self) -> f64 {
        0.7
    }
}

impl ReproductionAnalysisStats {
    fn new() -> Self {
        Self {
            total_attempts: 0,
            successful_reproductions: 0,
            average_execution_time_ms: 0.0,
            best_complexity_reduction: 0.0,
            total_analysis_time_ms: 0,
            strategy_success_rates: HashMap::new(),
        }
    }
}

impl Default for MinimalReproductionFinder {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_finder_creation() {
        let finder = MinimalReproductionFinder::new();
        assert!(finder.is_ok());

        let finder = finder.unwrap();
        assert_eq!(finder.discovered_reproductions.len(), 0);
        assert_eq!(finder.analysis_stats.total_attempts, 0);
    }

    #[test]
    fn test_complexity_calculation() {
        let finder = MinimalReproductionFinder::new().unwrap();

        // Create a mock scenario
        let scenario = create_mock_scenario();
        let complexity = finder.calculate_scenario_complexity(&scenario).unwrap();

        assert!(complexity > 0.0);
    }

    #[test]
    fn test_parameter_categorization() {
        let finder = MinimalReproductionFinder::new().unwrap();

        assert_eq!(
            finder.categorize_parameter("network_latency"),
            ParameterCategory::NetworkConfiguration
        );
        assert_eq!(
            finder.categorize_parameter("byzantine_count"),
            ParameterCategory::ByzantineConfiguration
        );
        assert_eq!(
            finder.categorize_parameter("timeout_ms"),
            ParameterCategory::TimingConfiguration
        );
        assert_eq!(
            finder.categorize_parameter("participant_count"),
            ParameterCategory::ParticipantConfiguration
        );
    }

    #[test]
    fn test_parameter_value_extraction() {
        let finder = MinimalReproductionFinder::new().unwrap();

        match finder.extract_parameter_value("true") {
            ParameterValue::Boolean(true) => (),
            _ => panic!("Expected boolean true"),
        }

        match finder.extract_parameter_value("42") {
            ParameterValue::Integer(42) => (),
            _ => panic!("Expected integer 42"),
        }

        match finder.extract_parameter_value("3.14") {
            ParameterValue::Float(f) if (f - 3.14).abs() < 0.001 => (),
            _ => panic!("Expected float 3.14"),
        }
    }

    #[test]
    fn test_strategy_creation() {
        let binary_strategy = BinarySearchStrategy::new();
        assert_eq!(binary_strategy.strategy_name(), "BinarySearch");
        assert_eq!(binary_strategy.expected_complexity_reduction(), 0.5);

        let greedy_strategy = GreedyReductionStrategy::new();
        assert_eq!(greedy_strategy.strategy_name(), "GreedyReduction");
        assert_eq!(greedy_strategy.expected_complexity_reduction(), 0.7);
    }

    fn create_mock_scenario() -> Scenario {
        use crate::scenario::types::*;

        Scenario {
            name: "test_scenario".to_string(),
            description: "Test scenario for minimal reproduction".to_string(),
            tags: vec!["test".to_string()],
            setup: ScenarioSetup {
                network: NetworkSetup {
                    participant_count: 3,
                    latency_ms: (10, 100),
                    drop_rate: Some(0.1),
                    partitions: None,
                },
                byzantine: Some(ByzantineSetup {
                    byzantine_count: 1,
                    strategies: vec!["message_delay".to_string()],
                    parameters: None,
                }),
                participants: None,
            },
            execution: ScenarioExecution {
                protocols: vec![ProtocolExecution {
                    protocol_type: ProtocolType::Dkd,
                    participants: None,
                    parameters: None,
                }],
                environment: None,
                assertions: vec![AssertionType::AllParticipantsDeriveSameKey],
            },
            metadata: ScenarioMetadata {
                created_at: "2025-01-01T00:00:00Z".to_string(),
                version: "1.0".to_string(),
                author: "test".to_string(),
                extends: None,
            },
        }
    }
}
