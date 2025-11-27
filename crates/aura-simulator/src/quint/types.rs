//! Simulation-Specific Quint Type Definitions
//!
//! Data structures for simulation-specific aspects of Quint integration,
//! including chaos generation, Byzantine mapping, and ITF trace conversion.
//!
//! Core Quint types (Property, PropertySpec, EvaluationResult) have been moved
//! to aura-core and aura-quint for proper architectural separation. The
//! simulator keeps lightweight mirrors here and provides adapters at the bridge
//! layer to avoid leaking runtime dependencies back into the core crates.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during Quint operations (simulation-specific)
#[derive(Error, Debug, Clone)]
pub enum QuintError {
    /// Failed to parse Quint specification file
    #[error("Parse error: {0}")]
    ParseError(String),

    /// Invalid property expression in specification
    #[error("Invalid property expression: {0}")]
    InvalidProperty(String),

    /// Type checking failed for Quint specification
    #[error("Type checking error: {0}")]
    TypeCheck(String),

    /// Error during property evaluation against simulation state
    #[error("Evaluation error: {0}")]
    Evaluation(String),

    /// Attempted to use unsupported Quint feature
    #[error("Unsupported feature: {0}")]
    UnsupportedFeature(String),
}

/// Complete Quint specification loaded from a .qnt file (simulation-specific extension)
///
/// Represents a parsed Quint module with simulation-specific enhancements for
/// chaos generation and Byzantine analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintSpec {
    /// Name of the specification (typically the module name)
    pub name: String,
    /// Path to the source .qnt file
    pub file_path: PathBuf,
    /// Name of the Quint module
    pub module_name: String,
    /// Version of the specification
    pub version: String,
    /// Description of the specification
    pub description: String,
    /// Modules defined in this specification
    pub modules: Vec<QuintModule>,
    /// Metadata for the specification
    pub metadata: HashMap<String, String>,
    /// Invariant properties defined in this specification
    pub invariants: Vec<QuintInvariant>,
    /// Temporal logic properties (LTL/CTL) defined in this specification
    pub temporal_properties: Vec<QuintTemporalProperty>,
    /// Safety properties defined in this specification
    pub safety_properties: Vec<QuintSafetyProperty>,
    /// State variables defined in the module
    pub state_variables: Vec<QuintStateVariable>,
    /// Actions/transitions defined in the module
    pub actions: Vec<QuintAction>,
}

/// Parsed Quint module with definitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintModule {
    pub name: String,
    pub definitions: Vec<QuintDefinition>,
}

/// Quint definition kinds captured during parsing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuintDefinition {
    Invariant(QuintInvariant),
    Temporal(QuintTemporalProperty),
    Safety(QuintSafetyProperty),
    StateVar(QuintStateVariable),
    Action(QuintAction),
}

/// Invariant property from a Quint specification (simulation-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintInvariant {
    /// Name of the invariant
    pub name: String,
    /// Quint expression defining the invariant
    pub expression: String,
    /// Human-readable description of the property
    pub description: String,
    /// Source location (file:line) where this invariant is defined
    pub source_location: String,
    /// Whether this invariant is enabled for checking
    pub enabled: bool,
    /// Tags associated with this invariant
    pub tags: Vec<String>,
}

/// Temporal logic property from a Quint specification (simulation-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintTemporalProperty {
    /// Name of the temporal property
    pub name: String,
    /// Type of temporal logic (LTL, CTL, etc.)
    pub property_type: String,
    /// Quint expression defining the temporal property
    pub expression: String,
    /// Human-readable description of the property
    pub description: String,
    /// Source location (file:line) where this property is defined
    pub source_location: String,
    /// Whether this property is enabled for checking
    pub enabled: bool,
    /// Tags associated with this property
    pub tags: Vec<String>,
}

/// State variable definition from a Quint specification (simulation-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintStateVariable {
    /// Name of the state variable
    pub name: String,
    /// Quint type of the variable
    pub variable_type: String,
    /// Initial value expression (if specified)
    pub initial_value: Option<String>,
    /// Description of the variable's purpose
    pub description: String,
}

/// Action/transition definition from a Quint specification (simulation-specific)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintAction {
    /// Name of the action
    pub name: String,
    /// Parameters of the action
    pub parameters: Vec<QuintParameter>,
    /// Precondition that must hold for the action to be enabled
    pub precondition: Option<String>,
    /// Effect of the action on the state
    pub effect: String,
    /// Description of what the action does
    pub description: String,
}

/// Parameter of a Quint action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintParameter {
    /// Name of the parameter
    pub name: String,
    /// Quint type of the parameter
    pub parameter_type: String,
    /// Default value (if any)
    pub default_value: Option<String>,
}

/// Abstraction of simulation state for Quint evaluation
///
/// This trait allows the Quint bridge to evaluate properties against
/// simulation state without being tightly coupled to specific simulation
/// implementations.
pub trait SimulationState {
    /// Get the value of a state variable by name
    fn get_variable(&self, name: &str) -> Option<QuintValue>;

    /// Get all state variables as a map
    fn get_all_variables(&self) -> std::collections::HashMap<String, QuintValue>;

    /// Get the current time/step in the simulation
    fn get_current_time(&self) -> u64;

    /// Get simulation metadata (participant count, etc.)
    fn get_metadata(&self) -> std::collections::HashMap<String, QuintValue>;
}

/// Value type for Quint evaluations (simulation-specific)
///
/// Represents values that can be passed between the simulation and Quint evaluator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QuintValue {
    /// Boolean value
    Bool(bool),
    /// Integer value
    Int(i64),
    /// String value
    String(String),
    /// List of values
    List(Vec<QuintValue>),
    /// Set of values
    Set(std::collections::HashSet<QuintValue>),
    /// Map of values
    Map(std::collections::HashMap<String, QuintValue>),
    /// Record/struct with named fields
    Record(std::collections::HashMap<String, QuintValue>),
}

impl std::hash::Hash for QuintValue {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            QuintValue::Bool(b) => {
                0u8.hash(state);
                b.hash(state);
            }
            QuintValue::Int(i) => {
                1u8.hash(state);
                i.hash(state);
            }
            QuintValue::String(s) => {
                2u8.hash(state);
                s.hash(state);
            }
            QuintValue::List(l) => {
                3u8.hash(state);
                l.hash(state);
            }
            QuintValue::Set(s) => {
                4u8.hash(state);
                // Note: HashSet doesn't implement Hash, so we sort for consistency
                let mut items: Vec<_> = s.iter().collect();
                items.sort_by(|a, b| format!("{:?}", a).cmp(&format!("{:?}", b)));
                items.hash(state);
            }
            QuintValue::Map(m) => {
                5u8.hash(state);
                // Sort keys for consistent hashing
                let mut items: Vec<_> = m.iter().collect();
                items.sort_by_key(|(k, _)| *k);
                items.hash(state);
            }
            QuintValue::Record(r) => {
                6u8.hash(state);
                // Sort keys for consistent hashing
                let mut items: Vec<_> = r.iter().collect();
                items.sort_by_key(|(k, _)| *k);
                items.hash(state);
            }
        }
    }
}

impl std::cmp::PartialEq for QuintValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (QuintValue::Bool(a), QuintValue::Bool(b)) => a == b,
            (QuintValue::Int(a), QuintValue::Int(b)) => a == b,
            (QuintValue::String(a), QuintValue::String(b)) => a == b,
            (QuintValue::List(a), QuintValue::List(b)) => a == b,
            (QuintValue::Set(a), QuintValue::Set(b)) => a == b,
            (QuintValue::Map(a), QuintValue::Map(b)) => a == b,
            (QuintValue::Record(a), QuintValue::Record(b)) => a == b,
            _ => false,
        }
    }
}

impl std::cmp::Eq for QuintValue {}

impl QuintValue {
    /// Convert to boolean if possible
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            QuintValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// Convert to integer if possible
    pub fn as_int(&self) -> Option<i64> {
        match self {
            QuintValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Convert to string if possible
    pub fn as_string(&self) -> Option<&str> {
        match self {
            QuintValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Get type name as string
    pub fn type_name(&self) -> &'static str {
        match self {
            QuintValue::Bool(_) => "bool",
            QuintValue::Int(_) => "int",
            QuintValue::String(_) => "string",
            QuintValue::List(_) => "list",
            QuintValue::Set(_) => "set",
            QuintValue::Map(_) => "map",
            QuintValue::Record(_) => "record",
        }
    }
}

/// Types of temporal properties for enhanced analysis
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TemporalPropertyType {
    /// Eventually P (liveness property)
    Eventually,
    /// Always P (safety property)
    Always,
    /// P leads to Q (response property)
    LeadsTo,
    /// P until Q (temporal ordering)
    Until,
}

/// Enhanced temporal property with structured type information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintEnhancedTemporalProperty {
    /// Name of the temporal property
    pub name: String,
    /// Structured type of temporal logic property
    pub property_type: TemporalPropertyType,
    /// Quint expression defining the temporal property
    pub expression: String,
    /// Human-readable description of the property
    pub description: String,
    /// Source location (file:line) where this property is defined
    pub source_location: String,
    /// Associated invariants that support this property
    pub supporting_invariants: Vec<String>,
    /// Priority for property checking (High, Medium, Low)
    pub priority: PropertyPriority,
}

/// Safety property with enhanced analysis information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintSafetyProperty {
    /// Name of the safety property
    pub name: String,
    /// Quint expression defining the safety property
    pub expression: String,
    /// Human-readable description of the property
    pub description: String,
    /// Source location (file:line) where this property is defined
    pub source_location: String,
    /// Type of safety property (consistency, integrity, etc.)
    pub safety_type: SafetyPropertyType,
    /// Associated state variables that this property monitors
    pub monitored_variables: Vec<String>,
}

/// Types of safety properties
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SafetyPropertyType {
    /// Data consistency properties
    Consistency,
    /// Data integrity properties
    Integrity,
    /// Authentication and authorization
    Authentication,
    /// Mutual exclusion properties
    MutualExclusion,
    /// General safety property
    General,
}

/// Property priority for testing and analysis
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum PropertyPriority {
    /// Low priority properties
    Low,
    /// Medium priority properties
    Medium,
    /// High priority properties
    High,
    /// Critical properties that must always hold
    Critical,
}

/// Violation patterns detected from property analysis
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ViolationPattern {
    /// Key consistency violation patterns
    KeyConsistency,
    /// Threshold protocol violation patterns
    ThresholdViolation,
    /// Session consistency violation patterns
    SessionConsistency,
    /// Byzantine resistance violation patterns
    ByzantineResistance,
    /// Effect API consistency violation patterns
    LedgerConsistency,
    /// Network partition tolerance violation patterns
    PartitionTolerance,
    /// General violation pattern
    General,
}

/// Enhanced Quint specification with analysis information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuintEnhancedSpec {
    /// Base specification information
    pub base_spec: QuintSpec,
    /// Enhanced temporal properties
    pub enhanced_temporal_properties: Vec<QuintEnhancedTemporalProperty>,
    /// Safety properties
    pub safety_properties: Vec<QuintSafetyProperty>,
    /// Analysis metadata
    pub analysis_metadata: SpecAnalysisMetadata,
}

/// Metadata from specification analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecAnalysisMetadata {
    /// Total number of properties analyzed
    pub total_properties: usize,
    /// Number of high-priority properties
    pub high_priority_count: usize,
    /// Detected violation patterns
    pub detected_patterns: Vec<ViolationPattern>,
    /// Complexity score for chaos generation
    pub complexity_score: f64,
    /// Analysis timestamp
    pub analyzed_at: u64,
}

/// Network chaos conditions for generated scenarios
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkChaosConditions {
    /// Message drop rate (0.0 to 1.0)
    pub message_drop_rate: Option<f64>,
    /// Latency range in milliseconds (min, max)
    pub latency_range_ms: Option<(u64, u64)>,
    /// Network partitions (groups of participants)
    pub partitions: Option<Vec<Vec<String>>>,
}

/// Chaos scenario generated from Quint property analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosScenario {
    /// Unique identifier for this scenario
    pub id: String,
    /// Human-readable name for the scenario
    pub name: String,
    /// Description of what this scenario tests
    pub description: String,
    /// Target property this scenario attempts to violate
    pub target_property: String,
    /// Type of chaos this scenario introduces
    pub chaos_type: ChaosType,
    /// Number of byzantine participants
    pub byzantine_participants: usize,
    /// Byzantine strategies to employ
    pub byzantine_strategies: Vec<String>,
    /// Network conditions for this scenario
    pub network_conditions: NetworkChaosConditions,
    /// Protocol-level disruptions
    pub protocol_disruptions: Vec<String>,
    /// Expected outcome of running this scenario
    pub expected_outcome: crate::scenario::types::ExpectedOutcome,
    /// Additional parameters for scenario execution
    pub parameters: HashMap<String, String>,
}

/// Types of chaos that can be introduced
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChaosType {
    /// Key consistency attacks
    KeyInconsistency,
    /// Threshold protocol attacks
    ThresholdAttack,
    /// Session disruption attacks
    SessionDisruption,
    /// Byzantine coordination attacks
    ByzantineCoordination,
    /// State corruption attacks
    StateCorruption,
    /// Network partition attacks
    NetworkPartition,
    /// Liveness violation attacks
    LivenessViolation,
    /// Safety violation attacks
    SafetyViolation,
    /// Causality violation attacks
    CausalityViolation,
    /// Temporal property violation attacks
    TemporalViolation,
    /// Direct property violation
    DirectViolation,
    /// Byzantine safety violation
    ByzantineSafetyViolation,
    /// Network-based safety violation
    NetworkSafetyViolation,
    /// General chaos (multiple attack vectors)
    General,
}

/// Result of property-based chaos scenario generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosGenerationResult {
    /// Number of scenarios generated
    pub scenarios_generated: usize,
    /// Generated chaos scenarios
    pub scenarios: Vec<ChaosScenario>,
    /// Properties that scenarios target
    pub targeted_properties: Vec<String>,
    /// Generation statistics
    pub generation_stats: ChaosGenerationStats,
}

/// Statistics about chaos scenario generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChaosGenerationStats {
    /// Time spent analyzing properties (ms)
    pub analysis_time_ms: u64,
    /// Time spent generating scenarios (ms)
    pub generation_time_ms: u64,
    /// Number of violation patterns detected
    pub patterns_detected: usize,
    /// Number of high-priority scenarios generated
    pub high_priority_scenarios: usize,
    /// Coverage of property types
    pub property_type_coverage: HashMap<String, usize>,
}

// Legacy simulation-specific evaluation types (different from aura-core types)
// These remain for backward compatibility with existing simulation harnesses;
// adapters translate to the newer aura-core types at the bridge layer.

/// Result of evaluating a Quint property against simulation state (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyEvaluationResult {
    /// Name of the property that was evaluated
    pub property_name: String,
    /// Whether the property holds
    pub holds: bool,
    /// Details about the evaluation
    pub details: String,
    /// Witness or counterexample (if applicable)
    pub witness: Option<String>,
    /// Evaluation time in milliseconds
    pub evaluation_time_ms: u64,
}

/// Validation result for a set of properties (legacy)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Total number of properties evaluated
    pub total_properties: usize,
    /// Number of properties that hold
    pub satisfied_properties: usize,
    /// Number of properties that were violated
    pub violated_properties: usize,
    /// Individual results for each property
    pub individual_results: Vec<PropertyEvaluationResult>,
    /// Total validation time in milliseconds
    pub total_time_ms: u64,
}

impl ValidationResult {
    /// Create a new empty validation result
    pub fn new() -> Self {
        Self {
            total_properties: 0,
            satisfied_properties: 0,
            violated_properties: 0,
            individual_results: Vec::new(),
            total_time_ms: 0,
        }
    }

    /// Add a property evaluation result
    pub fn add_result(&mut self, result: PropertyEvaluationResult) {
        self.total_properties += 1;
        if result.holds {
            self.satisfied_properties += 1;
        } else {
            self.violated_properties += 1;
        }
        self.total_time_ms += result.evaluation_time_ms;
        self.individual_results.push(result);
    }

    /// Check if all properties are satisfied
    pub fn all_satisfied(&self) -> bool {
        self.violated_properties == 0 && self.total_properties > 0
    }

    /// Get satisfaction rate as a percentage
    pub fn satisfaction_rate(&self) -> f64 {
        if self.total_properties == 0 {
            return 100.0;
        }
        (self.satisfied_properties as f64 / self.total_properties as f64) * 100.0
    }

    /// Get list of violated property names
    pub fn violated_property_names(&self) -> Vec<String> {
        self.individual_results
            .iter()
            .filter(|r| !r.holds)
            .map(|r| r.property_name.clone())
            .collect()
    }
}

impl Default for ValidationResult {
    fn default() -> Self {
        Self::new()
    }
}
