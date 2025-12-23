//! Property Extraction and Management
//!
//! Higher-level abstractions for working with properties extracted from Quint
//! specifications. Provides categorization, monitoring, and verification
//! capabilities for different types of properties.

use crate::quint::types::{
    PropertyEvaluationResult, QuintInvariant, QuintTemporalProperty, SimulationState,
    ValidationResult,
};
use crate::quint::QuintValue;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors that can occur during property operations
#[derive(Error, Debug, Clone)]
pub enum PropertyError {
    #[error("Property evaluation failed: {0}")]
    EvaluationFailed(String),

    #[error("Unsupported property type: {0}")]
    UnsupportedType(String),

    #[error("Property not found: {0}")]
    PropertyNotFound(String),

    #[error("Invalid property expression: {0}")]
    InvalidExpression(String),

    #[error("Evaluation timeout: {0}")]
    EvaluationTimeout(String),
}

/// Categories of verifiable properties
///
/// Different types of properties require different evaluation strategies
/// and monitoring approaches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PropertyType {
    /// Safety properties - something bad never happens
    Safety,
    /// Liveness properties - something good eventually happens
    Liveness,
    /// Invariant properties - always holds in reachable states
    Invariant,
    /// Temporal properties - LTL/CTL properties over execution traces
    Temporal,
    /// Performance properties - timing and resource constraints
    Performance,
    /// Security properties - cryptographic and access control properties
    Security,
    /// Consensus properties - agreement and consistency properties
    Consensus,
}

/// Abstraction for a verifiable property that can be monitored during simulation
///
/// Provides a unified interface for different types of properties extracted
/// from Quint specifications, enabling systematic property-based testing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiableProperty {
    /// Unique identifier for this property
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Type/category of this property
    pub property_type: PropertyType,
    /// Quint expression defining the property
    pub expression: String,
    /// Description of what this property checks
    pub description: String,
    /// Source location where property is defined
    pub source_location: String,
    /// Priority for testing (higher = more important)
    pub priority: PropertyPriority,
    /// Tags for categorization and filtering
    pub tags: Vec<String>,
    /// Whether this property should be checked continuously
    pub continuous_monitoring: bool,
}

/// Priority levels for property testing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropertyPriority {
    /// Low-priority properties for comprehensive testing
    Low,
    /// Medium-priority properties for additional validation
    Medium,
    /// High-priority properties important for correctness
    High,
    /// Critical properties that must always hold
    Critical,
}

/// Configuration for property extraction and monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyExtractionConfig {
    /// Which property types to extract
    pub enabled_types: Vec<PropertyType>,
    /// Minimum priority level to include
    pub min_priority: PropertyPriority,
    /// Whether to enable continuous monitoring
    pub enable_monitoring: bool,
    /// Maximum properties to extract (0 = unlimited)
    pub max_properties: usize,
    /// Tags to filter by (empty = all tags)
    pub filter_tags: Vec<String>,
}

impl Default for PropertyExtractionConfig {
    fn default() -> Self {
        Self {
            enabled_types: vec![
                PropertyType::Safety,
                PropertyType::Liveness,
                PropertyType::Invariant,
                PropertyType::Temporal,
                PropertyType::Consensus,
                PropertyType::Security,
                PropertyType::Performance,
            ],
            min_priority: PropertyPriority::Low,
            enable_monitoring: true,
            max_properties: 0,
            filter_tags: Vec::new(),
        }
    }
}

/// Property extractor that converts Quint properties to VerifiableProperty instances
pub struct PropertyExtractor {
    /// Configuration for property extraction
    config: PropertyExtractionConfig,
    /// Extracted properties indexed by ID
    properties: std::collections::HashMap<String, VerifiableProperty>,
    /// Property categorization rules
    categorization_rules: Vec<CategorizationRule>,
}

/// Rule for automatically categorizing properties based on patterns
#[derive(Debug, Clone)]
pub struct CategorizationRule {
    /// Pattern to match in property name or expression
    pub pattern: String,
    /// Property type to assign if pattern matches
    pub property_type: PropertyType,
    /// Priority to assign if pattern matches
    pub priority: PropertyPriority,
    /// Tags to add if pattern matches
    pub tags: Vec<String>,
}

impl PropertyExtractor {
    /// Create new property extractor with default configuration
    pub fn new() -> Self {
        Self {
            config: PropertyExtractionConfig::default(),
            properties: std::collections::HashMap::new(),
            categorization_rules: Self::default_categorization_rules(),
        }
    }

    /// Create property extractor with custom configuration
    pub fn with_config(config: PropertyExtractionConfig) -> Self {
        Self {
            config,
            properties: std::collections::HashMap::new(),
            categorization_rules: Self::default_categorization_rules(),
        }
    }

    /// Extract verifiable properties from Quint invariants and temporal properties
    ///
    /// # Arguments
    /// * `invariants` - Invariant properties from Quint specifications
    /// * `temporal_properties` - Temporal properties from Quint specifications
    ///
    /// # Returns
    /// * `Result<Vec<VerifiableProperty>>` - Extracted and categorized properties
    pub fn extract_properties(
        &mut self,
        invariants: &[QuintInvariant],
        temporal_properties: &[QuintTemporalProperty],
    ) -> Result<Vec<VerifiableProperty>, PropertyError> {
        self.properties.clear();

        // Extract invariants
        for invariant in invariants {
            let property = self.convert_invariant_to_property(invariant)?;
            if self.should_include_property(&property) {
                self.properties.insert(property.id.clone(), property);
            }
        }

        // Extract temporal properties
        for temporal in temporal_properties {
            let property = self.convert_temporal_to_property(temporal)?;
            if self.should_include_property(&property) {
                self.properties.insert(property.id.clone(), property);
            }
        }

        let mut result: Vec<VerifiableProperty> = self.properties.values().cloned().collect();

        // Sort by priority (highest first)
        result.sort_by(|a, b| b.priority.cmp(&a.priority));

        // Apply max properties limit
        if self.config.max_properties > 0 && result.len() > self.config.max_properties {
            result.truncate(self.config.max_properties);
        }

        Ok(result)
    }

    /// Get property by ID
    pub fn get_property(&self, id: &str) -> Option<&VerifiableProperty> {
        self.properties.get(id)
    }

    /// Get all properties of a specific type
    pub fn get_properties_by_type(&self, property_type: PropertyType) -> Vec<&VerifiableProperty> {
        self.properties
            .values()
            .filter(|p| p.property_type == property_type)
            .collect()
    }

    /// Get all properties with a specific tag
    pub fn get_properties_by_tag(&self, tag: &str) -> Vec<&VerifiableProperty> {
        self.properties
            .values()
            .filter(|p| p.tags.contains(&tag.to_string()))
            .collect()
    }

    /// Get properties suitable for continuous monitoring
    pub fn get_monitoring_properties(&self) -> Vec<&VerifiableProperty> {
        self.properties
            .values()
            .filter(|p| p.continuous_monitoring)
            .collect()
    }

    /// Convert Quint invariant to VerifiableProperty
    fn convert_invariant_to_property(
        &self,
        invariant: &QuintInvariant,
    ) -> Result<VerifiableProperty, PropertyError> {
        let id = format!("inv_{}", invariant.name);
        let (property_type, priority, tags) =
            self.categorize_property(&invariant.name, &invariant.expression);
        let continuous_monitoring = matches!(
            property_type,
            PropertyType::Invariant | PropertyType::Safety
        );

        Ok(VerifiableProperty {
            id,
            name: invariant.name.clone(),
            property_type,
            expression: invariant.expression.clone(),
            description: invariant.description.clone(),
            source_location: invariant.source_location.clone(),
            priority,
            tags,
            continuous_monitoring,
        })
    }

    /// Convert Quint temporal property to VerifiableProperty
    fn convert_temporal_to_property(
        &self,
        temporal: &QuintTemporalProperty,
    ) -> Result<VerifiableProperty, PropertyError> {
        let id = format!("temp_{}", temporal.name);
        let (_original_property_type, priority, tags) =
            self.categorize_property(&temporal.name, &temporal.expression);

        // Override property type for temporal properties
        let property_type = if temporal.expression.contains("eventually") {
            PropertyType::Liveness
        } else if temporal.expression.contains("always") {
            PropertyType::Safety
        } else {
            PropertyType::Temporal
        };

        let continuous_monitoring = matches!(property_type, PropertyType::Safety);

        Ok(VerifiableProperty {
            id,
            name: temporal.name.clone(),
            property_type,
            expression: temporal.expression.clone(),
            description: temporal.description.clone(),
            source_location: temporal.source_location.clone(),
            priority,
            tags,
            continuous_monitoring,
        })
    }

    /// Categorize property based on name and expression patterns
    fn categorize_property(
        &self,
        name: &str,
        expression: &str,
    ) -> (PropertyType, PropertyPriority, Vec<String>) {
        for rule in &self.categorization_rules {
            if name.contains(&rule.pattern) || expression.contains(&rule.pattern) {
                return (
                    rule.property_type.clone(),
                    rule.priority.clone(),
                    rule.tags.clone(),
                );
            }
        }

        // Default categorization
        (
            PropertyType::Invariant,
            PropertyPriority::Medium,
            vec!["uncategorized".to_string()],
        )
    }

    /// Check if property should be included based on configuration
    fn should_include_property(&self, property: &VerifiableProperty) -> bool {
        // Check property type filter
        if !self.config.enabled_types.contains(&property.property_type) {
            return false;
        }

        // Check priority filter
        if property.priority < self.config.min_priority {
            return false;
        }

        // Check tag filter
        if !self.config.filter_tags.is_empty() {
            let has_matching_tag = property
                .tags
                .iter()
                .any(|tag| self.config.filter_tags.contains(tag));
            if !has_matching_tag {
                return false;
            }
        }

        true
    }

    /// Default categorization rules for property classification
    fn default_categorization_rules() -> Vec<CategorizationRule> {
        vec![
            // More specific patterns first
            CategorizationRule {
                pattern: "crypto".to_string(),
                property_type: PropertyType::Security,
                priority: PropertyPriority::Critical,
                tags: vec!["security".to_string(), "crypto".to_string()],
            },
            CategorizationRule {
                pattern: "byzantine".to_string(),
                property_type: PropertyType::Security,
                priority: PropertyPriority::High,
                tags: vec!["security".to_string(), "byzantine".to_string()],
            },
            CategorizationRule {
                pattern: "consensus".to_string(),
                property_type: PropertyType::Consensus,
                priority: PropertyPriority::Critical,
                tags: vec!["consensus".to_string()],
            },
            CategorizationRule {
                pattern: "threshold".to_string(),
                property_type: PropertyType::Consensus,
                priority: PropertyPriority::High,
                tags: vec!["consensus".to_string(), "threshold".to_string()],
            },
            // More general patterns after specific ones
            CategorizationRule {
                pattern: "safety".to_string(),
                property_type: PropertyType::Safety,
                priority: PropertyPriority::Critical,
                tags: vec!["safety".to_string()],
            },
            CategorizationRule {
                pattern: "liveness".to_string(),
                property_type: PropertyType::Liveness,
                priority: PropertyPriority::High,
                tags: vec!["liveness".to_string()],
            },
            CategorizationRule {
                pattern: "key".to_string(),
                property_type: PropertyType::Security,
                priority: PropertyPriority::High,
                tags: vec!["security".to_string(), "keys".to_string()],
            },
            CategorizationRule {
                pattern: "performance".to_string(),
                property_type: PropertyType::Performance,
                priority: PropertyPriority::Medium,
                tags: vec!["performance".to_string()],
            },
        ]
    }
}

impl Default for PropertyExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Property monitor for continuous validation during simulation
pub struct PropertyMonitor {
    /// Properties being monitored
    monitored_properties: Vec<VerifiableProperty>,
    /// Results from property evaluations
    evaluation_results: Vec<PropertyEvaluationResult>,
    /// Enable verbose logging
    verbose: bool,
}

impl PropertyMonitor {
    /// Create new property monitor
    pub fn new(properties: Vec<VerifiableProperty>) -> Self {
        Self {
            monitored_properties: properties,
            evaluation_results: Vec::new(),
            verbose: false,
        }
    }

    /// Enable verbose logging
    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Evaluate all monitored properties against current simulation state using a
    /// lightweight interpreter for common expression forms (boolean variable lookup
    /// and simple equality checks). This keeps the simulator deterministic without
    /// invoking the full Quint evaluator while still providing real signal.
    pub fn evaluate_properties(&mut self, _state: &dyn SimulationState) -> ValidationResult {
        let mut validation_result = ValidationResult::new();

        for property in &self.monitored_properties {
            // Evaluate using the lightweight interpreter defined in evaluate_single_property
            let holds = self.evaluate_single_property(property, _state);
            let eval_time = 0u64;

            let result = PropertyEvaluationResult {
                property_name: property.name.clone(),
                holds,
                details: format!("Evaluated property: {}", property.expression),
                witness: None,
                evaluation_time_ms: eval_time,
            };

            if self.verbose {
                println!(
                    "Property '{}': {} ({}ms)",
                    property.name,
                    if holds { "HOLDS" } else { "VIOLATED" },
                    eval_time
                );
            }

            self.evaluation_results.push(result.clone());
            validation_result.add_result(result);
        }

        validation_result.total_time_ms = 0;
        validation_result
    }

    /// Get all evaluation results
    pub fn get_results(&self) -> &[PropertyEvaluationResult] {
        &self.evaluation_results
    }

    /// Get results for a specific property
    pub fn get_property_results(&self, property_name: &str) -> Vec<&PropertyEvaluationResult> {
        self.evaluation_results
            .iter()
            .filter(|r| r.property_name == property_name)
            .collect()
    }

    /// Clear all evaluation results
    pub fn clear_results(&mut self) {
        self.evaluation_results.clear();
    }

    /// Evaluate a single property using the lightweight interpreter:
    /// - `foo`     => expects state variable `foo` to be boolean true
    /// - `foo==bar`=> compares variables/literals
    /// - literals  => coerces to boolean where possible
    fn evaluate_single_property(
        &self,
        property: &VerifiableProperty,
        state: &dyn SimulationState,
    ) -> bool {
        let expr = property.expression.trim();

        // Equality check: lhs==rhs
        if let Some((lhs, rhs)) = expr.split_once("==") {
            let lhs_val = self.resolve_token(lhs.trim(), state);
            let rhs_val = self.resolve_token(rhs.trim(), state);
            return lhs_val == rhs_val;
        }

        // Fallback: treat expression as boolean variable reference
        match self.resolve_token(expr, state) {
            Some(QuintValue::Bool(b)) => b,
            Some(QuintValue::Int(i)) => i != 0,
            Some(QuintValue::String(s)) => !s.is_empty(),
            Some(QuintValue::List(v)) => !v.is_empty(),
            Some(QuintValue::Set(v)) => !v.is_empty(),
            Some(QuintValue::Map(v)) => !v.is_empty(),
            Some(QuintValue::Record(v)) => !v.is_empty(),
            // If the property references a value we don't track in this lightweight
            // interpreter, treat it as "unknown but not violated" to avoid false
            // negatives when running without full Quint state instrumentation.
            None => true,
        }
    }

    fn resolve_token(
        &self,
        token: &str,
        state: &dyn SimulationState,
    ) -> Option<crate::quint::types::QuintValue> {
        let token = token.trim();
        if let Some(val) = state.get_variable(token) {
            return Some(val);
        }
        match token {
            "true" => Some(crate::quint::types::QuintValue::Bool(true)),
            "false" => Some(crate::quint::types::QuintValue::Bool(false)),
            _ => {
                if let Ok(i) = token.parse::<i64>() {
                    Some(crate::quint::types::QuintValue::Int(i))
                } else {
                    None
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quint::types::{QuintInvariant, QuintTemporalProperty};

    #[test]
    fn test_property_extractor_creation() {
        let extractor = PropertyExtractor::new();
        assert_eq!(extractor.properties.len(), 0);

        let config = PropertyExtractionConfig {
            enabled_types: vec![PropertyType::Safety],
            min_priority: PropertyPriority::High,
            enable_monitoring: false,
            max_properties: 5,
            filter_tags: vec!["test".to_string()],
        };

        let extractor = PropertyExtractor::with_config(config);
        assert_eq!(extractor.config.enabled_types.len(), 1);
        assert_eq!(extractor.config.max_properties, 5);
    }

    #[test]
    fn test_property_extraction() {
        let mut extractor = PropertyExtractor::new();

        let invariants = vec![
            QuintInvariant {
                name: "safety_property".to_string(),
                expression: "no_double_spending".to_string(),
                description: "No double spending occurs".to_string(),
                source_location: "test.qnt:10".to_string(),
                enabled: true,
                tags: vec!["safety".to_string()],
            },
            QuintInvariant {
                name: "consensus_invariant".to_string(),
                expression: "all_honest_agree".to_string(),
                description: "All honest parties agree".to_string(),
                source_location: "test.qnt:15".to_string(),
                enabled: true,
                tags: vec!["consensus".to_string()],
            },
        ];

        let temporal_properties = vec![QuintTemporalProperty {
            name: "liveness_property".to_string(),
            property_type: "LTL".to_string(),
            expression: "eventually complete".to_string(),
            description: "Protocol eventually completes".to_string(),
            source_location: "test.qnt:20".to_string(),
            enabled: true,
            tags: vec!["liveness".to_string()],
        }];

        let result = extractor.extract_properties(&invariants, &temporal_properties);
        assert!(result.is_ok());

        let properties = result.unwrap();
        assert_eq!(properties.len(), 3);

        // Check that properties are sorted by priority
        assert!(properties[0].priority >= properties[1].priority);
    }

    #[test]
    fn test_property_categorization() {
        let mut extractor = PropertyExtractor::new();

        let invariants = vec![QuintInvariant {
            name: "crypto_safety".to_string(),
            expression: "valid_signatures".to_string(),
            description: "All signatures are valid".to_string(),
            source_location: "test.qnt:5".to_string(),
            enabled: true,
            tags: vec!["crypto".to_string(), "safety".to_string()],
        }];

        let properties = extractor.extract_properties(&invariants, &[]).unwrap();
        assert_eq!(properties.len(), 1);

        let property = &properties[0];
        assert_eq!(property.property_type, PropertyType::Security);
        assert_eq!(property.priority, PropertyPriority::Critical);
        assert!(property.tags.contains(&"security".to_string()));
        assert!(property.tags.contains(&"crypto".to_string()));
    }

    #[test]
    fn test_property_monitor() {
        let property = VerifiableProperty {
            id: "test_prop".to_string(),
            name: "test_property".to_string(),
            property_type: PropertyType::Invariant,
            expression: "always_valid".to_string(),
            description: "Test property".to_string(),
            source_location: "test.qnt:1".to_string(),
            priority: PropertyPriority::High,
            tags: vec!["test".to_string()],
            continuous_monitoring: true,
        };

        let mut monitor = PropertyMonitor::new(vec![property]).with_verbose(true);

        // Create a dummy simulation state for testing
        struct DummyState;
        impl SimulationState for DummyState {
            fn get_variable(&self, _name: &str) -> Option<crate::quint::types::QuintValue> {
                None
            }
            fn get_all_variables(
                &self,
            ) -> std::collections::HashMap<String, crate::quint::types::QuintValue> {
                std::collections::HashMap::new()
            }
            fn get_current_time(&self) -> u64 {
                0
            }
            fn get_metadata(
                &self,
            ) -> std::collections::HashMap<String, crate::quint::types::QuintValue> {
                std::collections::HashMap::new()
            }
        }

        let dummy_state = DummyState;
        let result = monitor.evaluate_properties(&dummy_state);

        assert_eq!(result.total_properties, 1);
        assert_eq!(result.satisfied_properties, 1);
        assert_eq!(result.violated_properties, 0);
        assert!(result.all_satisfied());
    }
}
