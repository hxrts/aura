//! Chaos Scenario Generation from Quint Properties
//!
//! This module implements chaos scenario generation driven by formal Quint
//! specifications. It analyzes verifiable properties to create targeted
//! test scenarios that attempt to violate specific properties.

use crate::quint::properties::{VerifiableProperty, PropertyType, PropertyPriority};
use crate::scenario::types::{Scenario, ScenarioSetup, ByzantineConditions, ByzantineStrategy, NetworkConditions, ProtocolType, ScenarioAssertion, ExpectedOutcome as ScenarioExpectedOutcome};
use thiserror::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Errors that can occur during chaos scenario generation
#[derive(Error, Debug, Clone)]
pub enum ChaosGeneratorError {
    #[error("Property analysis failed: {0}")]
    PropertyAnalysisFailed(String),
    
    #[error("Scenario generation failed: {0}")]
    ScenarioGenerationFailed(String),
    
    #[error("Unsupported property type for chaos generation: {0:?}")]
    UnsupportedPropertyType(PropertyType),
    
    #[error("Template not found: {0}")]
    TemplateNotFound(String),
    
    #[error("Invalid chaos configuration: {0}")]
    InvalidConfiguration(String),
}

/// Chaos scenario structure for property-violation testing
///
/// Represents a generated test scenario designed to violate specific
/// properties or test edge cases identified from Quint specifications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosScenario {
    /// Unique identifier for this chaos scenario
    pub id: String,
    /// Human-readable name describing the chaos scenario
    pub name: String,
    /// Property this scenario is designed to test/violate
    pub target_property: String,
    /// Type of chaos being introduced
    pub chaos_type: ChaosType,
    /// Scenario configuration derived from property analysis
    pub scenario: Scenario,
    /// Expected outcome (violation, timeout, etc.)
    pub expected_outcome: ScenarioExpectedOutcome,
    /// Metadata about scenario generation
    pub generation_metadata: GenerationMetadata,
}

/// Types of chaos that can be introduced in scenarios
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ChaosType {
    /// Byzantine participants with malicious behavior
    Byzantine,
    /// Network partitions and communication failures
    NetworkPartition,
    /// Timing attacks and delays
    TimingAttack,
    /// Resource exhaustion scenarios
    ResourceExhaustion,
    /// State corruption and invalid transitions
    StateCorruption,
    /// Cryptographic attacks and key compromise
    CryptographicAttack,
    /// Consensus disruption scenarios
    ConsensusDisruption,
}

// Use ExpectedOutcome from scenarios module
pub use crate::scenario::types::ExpectedOutcome;

/// Metadata about how a scenario was generated
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenerationMetadata {
    /// Quint property that inspired this scenario
    pub source_property: String,
    /// Template used for generation
    pub template_name: String,
    /// Generation timestamp
    pub generated_at: String,
    /// Configuration parameters used
    pub generation_params: HashMap<String, String>,
}

/// Configuration for chaos scenario generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosGenerationConfig {
    /// Maximum number of scenarios to generate per property
    pub max_scenarios_per_property: usize,
    /// Types of chaos to include in generation
    pub enabled_chaos_types: Vec<ChaosType>,
    /// Priority threshold for property selection
    pub min_property_priority: PropertyPriority,
    /// Whether to generate scenarios for satisfied properties
    pub include_satisfied_properties: bool,
    /// Network sizes to test
    pub test_network_sizes: Vec<usize>,
    /// Byzantine ratios to test (fraction of malicious nodes)
    pub byzantine_ratios: Vec<f64>,
}

impl Default for ChaosGenerationConfig {
    fn default() -> Self {
        Self {
            max_scenarios_per_property: 3,
            enabled_chaos_types: vec![
                ChaosType::Byzantine,
                ChaosType::NetworkPartition,
                ChaosType::TimingAttack,
                ChaosType::ConsensusDisruption,
            ],
            min_property_priority: PropertyPriority::Medium,
            include_satisfied_properties: true,
            test_network_sizes: vec![3, 5, 7],
            byzantine_ratios: vec![0.0, 0.33, 0.49],
        }
    }
}

/// Chaos scenario generator that creates targeted test scenarios from Quint properties
pub struct ChaosGenerator {
    /// Configuration for scenario generation
    config: ChaosGenerationConfig,
    /// Generated scenarios indexed by ID
    scenarios: HashMap<String, ChaosScenario>,
    /// Scenario templates for different chaos types
    templates: HashMap<ChaosType, ScenarioTemplate>,
}

/// Template for generating scenarios of a specific chaos type
#[derive(Debug, Clone)]
pub struct ScenarioTemplate {
    /// Base scenario configuration
    pub base_scenario: Scenario,
    /// Parameters that can be varied
    pub variable_params: Vec<VariableParameter>,
    /// Assertion patterns for this chaos type
    pub assertion_patterns: Vec<String>,
}

/// Parameter that can be varied in scenario generation
#[derive(Debug, Clone)]
pub struct VariableParameter {
    /// Name of the parameter
    pub name: String,
    /// Possible values for this parameter
    pub values: Vec<ParameterValue>,
}

/// Value for a variable parameter
#[derive(Debug, Clone)]
pub enum ParameterValue {
    /// Integer value
    Int(i64),
    /// Float value
    Float(f64),
    /// String value
    String(String),
    /// Boolean value
    Bool(bool),
}

impl ChaosGenerator {
    /// Create new chaos generator with default configuration
    pub fn new() -> Self {
        Self {
            config: ChaosGenerationConfig::default(),
            scenarios: HashMap::new(),
            templates: Self::default_templates(),
        }
    }
    
    /// Create chaos generator with custom configuration
    pub fn with_config(config: ChaosGenerationConfig) -> Self {
        Self {
            config,
            scenarios: HashMap::new(),
            templates: Self::default_templates(),
        }
    }
    
    /// Generate chaos scenarios from verifiable properties
    ///
    /// # Arguments
    /// * `properties` - Properties to analyze for chaos scenario generation
    ///
    /// # Returns
    /// * `Result<Vec<ChaosScenario>>` - Generated chaos scenarios
    pub fn generate_chaos_scenarios(
        &mut self,
        properties: &[VerifiableProperty],
    ) -> Result<Vec<ChaosScenario>, ChaosGeneratorError> {
        self.scenarios.clear();
        
        for property in properties {
            // Filter by priority
            if property.priority < self.config.min_property_priority {
                continue;
            }
            
            // Generate scenarios for this property
            let property_scenarios = self.generate_scenarios_for_property(property)?;
            
            for scenario in property_scenarios {
                self.scenarios.insert(scenario.id.clone(), scenario);
            }
        }
        
        let mut result: Vec<ChaosScenario> = self.scenarios.values().cloned().collect();
        
        // Sort by target property priority and chaos type
        result.sort_by(|a, b| {
            // Extract priority from property name (simplified approach)
            let a_priority = self.extract_property_priority(&a.target_property);
            let b_priority = self.extract_property_priority(&b.target_property);
            
            match b_priority.cmp(&a_priority) {
                std::cmp::Ordering::Equal => a.chaos_type.to_string().cmp(&b.chaos_type.to_string()),
                other => other,
            }
        });
        
        Ok(result)
    }
    
    /// Generate scenarios for a specific property
    fn generate_scenarios_for_property(
        &self,
        property: &VerifiableProperty,
    ) -> Result<Vec<ChaosScenario>, ChaosGeneratorError> {
        let mut scenarios = Vec::new();
        let chaos_types = self.select_chaos_types_for_property(property);
        
        for chaos_type in chaos_types {
            let generated = self.generate_scenario_variants(property, chaos_type)?;
            scenarios.extend(generated);
            
            // Limit scenarios per property
            if scenarios.len() >= self.config.max_scenarios_per_property {
                scenarios.truncate(self.config.max_scenarios_per_property);
                break;
            }
        }
        
        Ok(scenarios)
    }
    
    /// Select appropriate chaos types for a property
    fn select_chaos_types_for_property(&self, property: &VerifiableProperty) -> Vec<ChaosType> {
        let mut chaos_types = Vec::new();
        
        // Select chaos types based on property type and tags
        match property.property_type {
            PropertyType::Safety => {
                if property.tags.contains(&"crypto".to_string()) {
                    chaos_types.push(ChaosType::CryptographicAttack);
                }
                chaos_types.push(ChaosType::Byzantine);
                chaos_types.push(ChaosType::StateCorruption);
            }
            PropertyType::Liveness => {
                chaos_types.push(ChaosType::NetworkPartition);
                chaos_types.push(ChaosType::TimingAttack);
                chaos_types.push(ChaosType::Byzantine);
            }
            PropertyType::Consensus => {
                chaos_types.push(ChaosType::ConsensusDisruption);
                chaos_types.push(ChaosType::Byzantine);
                chaos_types.push(ChaosType::NetworkPartition);
            }
            PropertyType::Security => {
                chaos_types.push(ChaosType::CryptographicAttack);
                chaos_types.push(ChaosType::Byzantine);
            }
            PropertyType::Performance => {
                chaos_types.push(ChaosType::ResourceExhaustion);
                chaos_types.push(ChaosType::TimingAttack);
            }
            PropertyType::Invariant => {
                chaos_types.push(ChaosType::StateCorruption);
                chaos_types.push(ChaosType::Byzantine);
            }
            PropertyType::Temporal => {
                chaos_types.push(ChaosType::TimingAttack);
                chaos_types.push(ChaosType::NetworkPartition);
            }
        }
        
        // Filter by enabled chaos types
        chaos_types.retain(|ct| self.config.enabled_chaos_types.contains(ct));
        
        chaos_types
    }
    
    /// Generate scenario variants for a property and chaos type
    fn generate_scenario_variants(
        &self,
        property: &VerifiableProperty,
        chaos_type: ChaosType,
    ) -> Result<Vec<ChaosScenario>, ChaosGeneratorError> {
        let template = self.templates.get(&chaos_type)
            .ok_or_else(|| ChaosGeneratorError::TemplateNotFound(format!("{:?}", chaos_type)))?;
        
        let mut scenarios = Vec::new();
        
        // Generate variants by varying network size and byzantine ratio
        for &network_size in &self.config.test_network_sizes {
            for &byzantine_ratio in &self.config.byzantine_ratios {
                let scenario = self.create_scenario_from_template(
                    property,
                    chaos_type.clone(),
                    template,
                    network_size,
                    byzantine_ratio,
                )?;
                scenarios.push(scenario);
            }
        }
        
        Ok(scenarios)
    }
    
    /// Create a scenario from a template with specific parameters
    fn create_scenario_from_template(
        &self,
        property: &VerifiableProperty,
        chaos_type: ChaosType,
        template: &ScenarioTemplate,
        network_size: usize,
        byzantine_ratio: f64,
    ) -> Result<ChaosScenario, ChaosGeneratorError> {
        let byzantine_count = (network_size as f64 * byzantine_ratio).floor() as usize;
        
        let scenario_id = format!("chaos_{}_{}_n{}_b{}", 
                                 property.id, 
                                 chaos_type.to_string().to_lowercase(),
                                 network_size,
                                 byzantine_count);
        
        let scenario_name = format!("Chaos Test: {} - {} (n={}, byzantine={})",
                                   property.name,
                                   chaos_type.to_string(),
                                   network_size,
                                   byzantine_count);
        
        // Create scenario based on template
        let mut scenario = template.base_scenario.clone();
        scenario.name = scenario_name.clone();
        scenario.description = format!("Generated chaos scenario to test property '{}' using {} chaos",
                                     property.name, chaos_type.to_string());
        
        // Configure participants
        scenario.setup.participants = network_size;
        scenario.setup.threshold = (network_size * 2 / 3) + 1; // 2/3 + 1 threshold
        
        // Configure byzantine behavior
        if byzantine_count > 0 {
            scenario.byzantine = Some(ByzantineConditions {
                count: byzantine_count,
                participants: (0..byzantine_count).collect(),
                strategies: vec![ByzantineStrategy {
                    strategy_type: self.select_byzantine_strategy(&chaos_type, property),
                    description: Some(format!("Chaos testing strategy for {}", chaos_type.to_string())),
                    abort_after: None,
                }],
            });
        }
        
        // Configure network based on chaos type
        scenario.network = Some(self.create_network_config(&chaos_type));
        
        // Add property-specific assertions
        scenario.assertions = self.create_assertions(property, &chaos_type);
        
        // Set expected outcome
        scenario.expected_outcome = self.determine_expected_outcome(property, &chaos_type, byzantine_ratio);
        
        let generation_metadata = GenerationMetadata {
            source_property: property.id.clone(),
            template_name: format!("{:?}", chaos_type),
            generated_at: chrono::Utc::now().to_rfc3339(),
            generation_params: vec![
                ("network_size".to_string(), network_size.to_string()),
                ("byzantine_ratio".to_string(), byzantine_ratio.to_string()),
                ("byzantine_count".to_string(), byzantine_count.to_string()),
            ].into_iter().collect(),
        };
        
        Ok(ChaosScenario {
            id: scenario_id,
            name: scenario_name,
            target_property: property.id.clone(),
            chaos_type,
            expected_outcome: scenario.expected_outcome.clone(),
            scenario,
            generation_metadata,
        })
    }
    
    /// Select appropriate byzantine strategy for chaos type and property
    fn select_byzantine_strategy(&self, chaos_type: &ChaosType, property: &VerifiableProperty) -> String {
        match chaos_type {
            ChaosType::Byzantine => {
                if property.tags.contains(&"consensus".to_string()) {
                    "equivocation".to_string()
                } else if property.tags.contains(&"crypto".to_string()) {
                    "key_compromise".to_string()
                } else {
                    "arbitrary".to_string()
                }
            }
            ChaosType::ConsensusDisruption => "consensus_attack".to_string(),
            ChaosType::CryptographicAttack => "signature_forgery".to_string(),
            ChaosType::StateCorruption => "state_manipulation".to_string(),
            _ => "silent".to_string(),
        }
    }
    
    /// Create chaos-specific parameters
    fn create_chaos_params(&self, chaos_type: &ChaosType, _property: &VerifiableProperty) -> HashMap<String, String> {
        let mut params = HashMap::new();
        
        match chaos_type {
            ChaosType::TimingAttack => {
                params.insert("delay_ms".to_string(), "1000".to_string());
                params.insert("jitter_ms".to_string(), "500".to_string());
            }
            ChaosType::NetworkPartition => {
                params.insert("partition_duration_ms".to_string(), "5000".to_string());
                params.insert("partition_type".to_string(), "split_brain".to_string());
            }
            ChaosType::ResourceExhaustion => {
                params.insert("memory_limit_mb".to_string(), "100".to_string());
                params.insert("cpu_limit_percent".to_string(), "50".to_string());
            }
            _ => {}
        }
        
        params
    }
    
    /// Create network configuration for chaos type
    fn create_network_config(&self, chaos_type: &ChaosType) -> NetworkConditions {
        match chaos_type {
            ChaosType::NetworkPartition => NetworkConditions {
                latency_range: [100, 500],
                drop_rate: 0.1,
                partitions: vec![vec![0, 1], vec![2, 3]], // Example partition
            },
            ChaosType::TimingAttack => NetworkConditions {
                latency_range: [50, 2000],
                drop_rate: 0.05,
                partitions: vec![],
            },
            _ => NetworkConditions {
                latency_range: [10, 100],
                drop_rate: 0.01,
                partitions: vec![],
            },
        }
    }
    
    /// Create assertions for property and chaos type
    fn create_assertions(&self, property: &VerifiableProperty, chaos_type: &ChaosType) -> Vec<ScenarioAssertion> {
        let mut assertions = Vec::new();
        
        // Add property-specific assertion
        assertions.push(ScenarioAssertion {
            assertion_type: "property_monitor".to_string(),
            honest_participants: None,
            expected_detected: None,
            expected_property: Some(property.id.clone()),
            timeout_multiplier: None,
        });
        
        // Add chaos-specific assertions
        match chaos_type {
            ChaosType::Byzantine => {
                assertions.push(ScenarioAssertion {
                    assertion_type: "byzantine_detection".to_string(),
                    honest_participants: None,
                    expected_detected: Some((0..property.priority.clone() as usize).collect()), // Simplified
                    expected_property: None,
                    timeout_multiplier: None,
                });
            }
            ChaosType::NetworkPartition => {
                assertions.push(ScenarioAssertion {
                    assertion_type: "network_recovery".to_string(),
                    honest_participants: None,
                    expected_detected: None,
                    expected_property: None,
                    timeout_multiplier: Some(2.0),
                });
            }
            _ => {}
        }
        
        assertions
    }
    
    /// Determine expected outcome for property and chaos type
    fn determine_expected_outcome(&self, property: &VerifiableProperty, chaos_type: &ChaosType, byzantine_ratio: f64) -> ScenarioExpectedOutcome {
        match (property.property_type.clone(), chaos_type, byzantine_ratio) {
            (PropertyType::Safety, ChaosType::Byzantine, ratio) if ratio > 0.33 => {
                ScenarioExpectedOutcome::SafetyViolationPrevented
            }
            (PropertyType::Liveness, ChaosType::NetworkPartition, _) => {
                ScenarioExpectedOutcome::Failure
            }
            (PropertyType::Consensus, ChaosType::ConsensusDisruption, ratio) if ratio > 0.33 => {
                ScenarioExpectedOutcome::Failure
            }
            _ => {
                if property.priority == PropertyPriority::Critical && byzantine_ratio <= 0.33 {
                    ScenarioExpectedOutcome::Success
                } else {
                    ScenarioExpectedOutcome::HonestMajoritySuccess
                }
            }
        }
    }
    
    /// Extract property priority from property name (simplified heuristic)
    fn extract_property_priority(&self, _property_name: &str) -> PropertyPriority {
        // Simplified approach - in practice would look up actual property
        PropertyPriority::Medium
    }
    
    /// Get generated scenario by ID
    pub fn get_scenario(&self, id: &str) -> Option<&ChaosScenario> {
        self.scenarios.get(id)
    }
    
    /// Get all scenarios for a specific property
    pub fn get_scenarios_for_property(&self, property_id: &str) -> Vec<&ChaosScenario> {
        self.scenarios
            .values()
            .filter(|s| s.target_property == property_id)
            .collect()
    }
    
    /// Get scenarios by chaos type
    pub fn get_scenarios_by_chaos_type(&self, chaos_type: ChaosType) -> Vec<&ChaosScenario> {
        self.scenarios
            .values()
            .filter(|s| s.chaos_type == chaos_type)
            .collect()
    }
    
    /// Default scenario templates for different chaos types
    fn default_templates() -> HashMap<ChaosType, ScenarioTemplate> {
        let mut templates = HashMap::new();
        
        // Byzantine chaos template
        templates.insert(ChaosType::Byzantine, ScenarioTemplate {
            base_scenario: Self::create_base_byzantine_scenario(),
            variable_params: vec![
                VariableParameter {
                    name: "byzantine_count".to_string(),
                    values: vec![
                        ParameterValue::Int(1),
                        ParameterValue::Int(2),
                    ],
                },
            ],
            assertion_patterns: vec![
                "byzantine_behavior_detected".to_string(),
                "safety_maintained".to_string(),
            ],
        });
        
        // Network partition template
        templates.insert(ChaosType::NetworkPartition, ScenarioTemplate {
            base_scenario: Self::create_base_partition_scenario(),
            variable_params: vec![
                VariableParameter {
                    name: "partition_duration".to_string(),
                    values: vec![
                        ParameterValue::Int(1000),
                        ParameterValue::Int(5000),
                    ],
                },
            ],
            assertion_patterns: vec![
                "partition_recovery".to_string(),
                "liveness_maintained".to_string(),
            ],
        });
        
        // Add other chaos type templates...
        templates.insert(ChaosType::TimingAttack, ScenarioTemplate {
            base_scenario: Self::create_base_timing_scenario(),
            variable_params: vec![],
            assertion_patterns: vec!["timing_resilience".to_string()],
        });
        
        templates.insert(ChaosType::ResourceExhaustion, ScenarioTemplate {
            base_scenario: Self::create_base_resource_scenario(),
            variable_params: vec![],
            assertion_patterns: vec!["resource_management".to_string()],
        });
        
        templates.insert(ChaosType::StateCorruption, ScenarioTemplate {
            base_scenario: Self::create_base_corruption_scenario(),
            variable_params: vec![],
            assertion_patterns: vec!["state_integrity".to_string()],
        });
        
        templates.insert(ChaosType::CryptographicAttack, ScenarioTemplate {
            base_scenario: Self::create_base_crypto_scenario(),
            variable_params: vec![],
            assertion_patterns: vec!["crypto_security".to_string()],
        });
        
        templates.insert(ChaosType::ConsensusDisruption, ScenarioTemplate {
            base_scenario: Self::create_base_consensus_scenario(),
            variable_params: vec![],
            assertion_patterns: vec!["consensus_integrity".to_string()],
        });
        
        templates
    }
    
    /// Create base byzantine scenario template
    fn create_base_byzantine_scenario() -> Scenario {
        Scenario {
            name: "Byzantine Chaos Template".to_string(),
            description: "Template for byzantine participant chaos testing".to_string(),
            extends: None,
            setup: ScenarioSetup {
                participants: 3,
                threshold: 2,
                seed: 42,
            },
            network: None,
            byzantine: None,
            phases: None,
            protocols: None,
            assertions: Vec::new(),
            expected_outcome: ScenarioExpectedOutcome::Success,
            quint_source: None,
        }
    }
    
    /// Create base network partition scenario template
    fn create_base_partition_scenario() -> Scenario {
        Scenario {
            name: "Network Partition Template".to_string(),
            description: "Template for network partition chaos testing".to_string(),
            extends: None,
            setup: ScenarioSetup {
                participants: 5,
                threshold: 3,
                seed: 42,
            },
            network: Some(NetworkConditions {
                latency_range: [100, 1000],
                drop_rate: 0.1,
                partitions: vec![vec![0, 1], vec![2, 3, 4]],
            }),
            byzantine: None,
            phases: None,
            protocols: None,
            assertions: Vec::new(),
            expected_outcome: ScenarioExpectedOutcome::Success,
            quint_source: None,
        }
    }
    
    /// Create other base scenario templates (simplified implementations)
    fn create_base_timing_scenario() -> Scenario {
        Self::create_base_byzantine_scenario() // Simplified
    }
    
    fn create_base_resource_scenario() -> Scenario {
        Self::create_base_byzantine_scenario() // Simplified
    }
    
    fn create_base_corruption_scenario() -> Scenario {
        Self::create_base_byzantine_scenario() // Simplified
    }
    
    fn create_base_crypto_scenario() -> Scenario {
        Self::create_base_byzantine_scenario() // Simplified
    }
    
    fn create_base_consensus_scenario() -> Scenario {
        Self::create_base_byzantine_scenario() // Simplified
    }
}

impl Default for ChaosGenerator {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ChaosType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChaosType::Byzantine => write!(f, "Byzantine"),
            ChaosType::NetworkPartition => write!(f, "NetworkPartition"),
            ChaosType::TimingAttack => write!(f, "TimingAttack"),
            ChaosType::ResourceExhaustion => write!(f, "ResourceExhaustion"),
            ChaosType::StateCorruption => write!(f, "StateCorruption"),
            ChaosType::CryptographicAttack => write!(f, "CryptographicAttack"),
            ChaosType::ConsensusDisruption => write!(f, "ConsensusDisruption"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quint::properties::{VerifiableProperty, PropertyType, PropertyPriority};
    
    #[test]
    fn test_chaos_generator_creation() {
        let generator = ChaosGenerator::new();
        assert_eq!(generator.scenarios.len(), 0);
        assert_eq!(generator.templates.len(), 7); // All chaos types
        
        let config = ChaosGenerationConfig {
            max_scenarios_per_property: 2,
            enabled_chaos_types: vec![ChaosType::Byzantine],
            min_property_priority: PropertyPriority::High,
            include_satisfied_properties: false,
            test_network_sizes: vec![3],
            byzantine_ratios: vec![0.33],
        };
        
        let generator = ChaosGenerator::with_config(config);
        assert_eq!(generator.config.max_scenarios_per_property, 2);
    }
    
    #[test]
    fn test_chaos_scenario_generation() {
        let mut generator = ChaosGenerator::new();
        
        let properties = vec![
            VerifiableProperty {
                id: "safety_prop".to_string(),
                name: "Safety Property".to_string(),
                property_type: PropertyType::Safety,
                expression: "no_double_spending".to_string(),
                description: "No double spending occurs".to_string(),
                source_location: "test.qnt:10".to_string(),
                priority: PropertyPriority::Critical,
                tags: vec!["safety".to_string()],
                continuous_monitoring: true,
            },
        ];
        
        let result = generator.generate_chaos_scenarios(&properties);
        assert!(result.is_ok());
        
        let scenarios = result.unwrap();
        assert!(!scenarios.is_empty());
        
        // Should generate scenarios for safety property
        let safety_scenarios: Vec<_> = scenarios.iter()
            .filter(|s| s.target_property == "safety_prop")
            .collect();
        assert!(!safety_scenarios.is_empty());
    }
    
    #[test]
    fn test_chaos_type_selection() {
        let generator = ChaosGenerator::new();
        
        let safety_property = VerifiableProperty {
            id: "safety_test".to_string(),
            name: "Safety Test".to_string(),
            property_type: PropertyType::Safety,
            expression: "always_safe".to_string(),
            description: "Test safety property".to_string(),
            source_location: "test.qnt:1".to_string(),
            priority: PropertyPriority::High,
            tags: vec!["safety".to_string()],
            continuous_monitoring: true,
        };
        
        let chaos_types = generator.select_chaos_types_for_property(&safety_property);
        assert!(chaos_types.contains(&ChaosType::Byzantine));
        assert!(chaos_types.contains(&ChaosType::StateCorruption));
        
        let consensus_property = VerifiableProperty {
            id: "consensus_test".to_string(),
            name: "Consensus Test".to_string(),
            property_type: PropertyType::Consensus,
            expression: "agreement".to_string(),
            description: "Test consensus property".to_string(),
            source_location: "test.qnt:1".to_string(),
            priority: PropertyPriority::Critical,
            tags: vec!["consensus".to_string()],
            continuous_monitoring: true,
        };
        
        let consensus_chaos_types = generator.select_chaos_types_for_property(&consensus_property);
        assert!(consensus_chaos_types.contains(&ChaosType::ConsensusDisruption));
        assert!(consensus_chaos_types.contains(&ChaosType::Byzantine));
    }
    
    #[test]
    fn test_expected_outcome_determination() {
        let generator = ChaosGenerator::new();
        
        let safety_property = VerifiableProperty {
            id: "safety_test".to_string(),
            name: "Safety Test".to_string(),
            property_type: PropertyType::Safety,
            expression: "always_safe".to_string(),
            description: "Test safety property".to_string(),
            source_location: "test.qnt:1".to_string(),
            priority: PropertyPriority::Critical,
            tags: vec!["safety".to_string()],
            continuous_monitoring: true,
        };
        
        // High byzantine ratio should expect safety violation prevented or failure
        let outcome = generator.determine_expected_outcome(&safety_property, &ChaosType::Byzantine, 0.5);
        match outcome {
            ExpectedOutcome::SafetyViolationPrevented => {}
            _ => panic!("Expected safety violation prevented for high byzantine ratio"),
        }
        
        // Low byzantine ratio should maintain safety
        let outcome = generator.determine_expected_outcome(&safety_property, &ChaosType::Byzantine, 0.1);
        match outcome {
            ExpectedOutcome::Success => {}
            _ => panic!("Expected success for low byzantine ratio"),
        }
    }
}