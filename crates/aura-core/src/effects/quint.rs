//! Quint formal verification effects
//!
//! This module defines the effect traits for Quint formal verification integration.
//! Following the algebraic effects architecture, these traits define capabilities
//! without implementation details.

use crate::Result;
use serde_json::Value;
use uuid;

/// Identifier for a property specification
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PropertyId(pub String);

/// Identifier for a verification session
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VerificationId(pub String);

/// Quint property specification
#[derive(Debug, Clone)]
pub struct Property {
    pub id: PropertyId,
    pub name: String,
    pub kind: PropertyKind,
    pub expression: String,
    pub description: Option<String>,
}

/// Types of properties that can be verified
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyKind {
    /// Invariant property (always holds)
    Invariant,
    /// Temporal property (eventually holds)
    Temporal,
    /// Safety property (nothing bad happens)
    Safety,
    /// Liveness property (something good eventually happens)
    Liveness,
}

/// Complete specification containing multiple properties
#[derive(Debug, Clone)]
pub struct PropertySpec {
    pub name: String,
    pub properties: Vec<Property>,
    pub context: Value, // Quint context variables
}

/// Result of property evaluation
#[derive(Debug, Clone)]
pub struct EvaluationResult {
    pub property_id: PropertyId,
    pub passed: bool,
    pub counterexample: Option<Counterexample>,
    pub statistics: EvaluationStatistics,
}

/// Counterexample when property fails
#[derive(Debug, Clone)]
pub struct Counterexample {
    pub trace: Vec<Value>, // State trace leading to violation
    pub violation_step: usize,
    pub description: String,
}

/// Statistics from property evaluation
#[derive(Debug, Clone)]
pub struct EvaluationStatistics {
    pub steps_explored: usize,
    pub execution_time_ms: u64,
    pub memory_used_bytes: usize,
}

/// Full verification result for a specification
#[derive(Debug, Clone)]
pub struct VerificationResult {
    pub verification_id: VerificationId,
    pub spec_name: String,
    pub property_results: Vec<EvaluationResult>,
    pub overall_success: bool,
    pub total_time_ms: u64,
}

/// Core Quint evaluation effects (Layer 1: Foundation)
///
/// These effects provide the foundational capabilities for Quint formal verification
/// without any simulation-specific logic. They operate on pure Quint specifications
/// and abstract state values.
#[async_trait::async_trait]
pub trait QuintEvaluationEffects: Send + Sync {
    /// Load a property specification from Quint source
    async fn load_property_spec(&self, spec_source: &str) -> Result<PropertySpec>;

    /// Evaluate a single property against a state
    async fn evaluate_property(&self, property: &Property, state: &Value) -> Result<EvaluationResult>;

    /// Run full verification of a specification
    async fn run_verification(&self, spec: &PropertySpec) -> Result<VerificationResult>;

    /// Parse Quint expression into evaluable form
    async fn parse_expression(&self, expression: &str) -> Result<Value>;

    /// Create initial state from Quint specification
    async fn create_initial_state(&self, spec: &PropertySpec) -> Result<Value>;

    /// Execute a single step transition in Quint model
    async fn execute_step(&self, current_state: &Value, action: &str) -> Result<Value>;
}

/// Quint verification effects for property checking (Layer 1: Foundation)
///
/// Higher-level verification capabilities that build on QuintEvaluationEffects
/// for running complete verification campaigns.
#[async_trait::async_trait]
pub trait QuintVerificationEffects: Send + Sync {
    /// Verify a property with comprehensive checking
    async fn verify_property(&self, property: &Property, state: &Value) -> Result<VerificationResult>;

    /// Generate counterexample for a failing property
    async fn generate_counterexample(&self, property: &Property) -> Result<Option<Counterexample>>;

    /// Load complete specification suite from file
    async fn load_specification(&self, spec_path: &str) -> Result<PropertySpec>;

    /// Run model checking with state space exploration
    async fn run_model_checking(
        &self,
        spec: &PropertySpec,
        max_steps: usize,
    ) -> Result<VerificationResult>;

    /// Validate Quint specification syntax
    async fn validate_specification(&self, spec_source: &str) -> Result<Vec<String>>;
}

/// Property evaluation trait for different state types
///
/// This generic trait allows different state representations to be used
/// with Quint property evaluation.
#[async_trait::async_trait]
pub trait PropertyEvaluator<State>: Send + Sync {
    /// Check if a property holds for the given state
    async fn check_property(&self, property: &Property, state: &State) -> Result<bool>;

    /// Evaluate property and return detailed result
    async fn evaluate_property_detailed(
        &self,
        property: &Property,
        state: &State,
    ) -> Result<EvaluationResult>;
}

impl PropertyId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl VerificationId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[allow(clippy::disallowed_methods)]
    pub fn generate() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }
}

impl Property {
    pub fn new(id: impl Into<String>, name: impl Into<String>, kind: PropertyKind, expression: impl Into<String>) -> Self {
        Self {
            id: PropertyId::new(id),
            name: name.into(),
            kind,
            expression: expression.into(),
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl PropertySpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            properties: Vec::new(),
            context: Value::Object(serde_json::Map::new()),
        }
    }

    pub fn with_property(mut self, property: Property) -> Self {
        self.properties.push(property);
        self
    }

    pub fn with_context(mut self, context: Value) -> Self {
        self.context = context;
        self
    }
}