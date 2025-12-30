//! Quint formal verification effects
//!
//! This module defines the effect traits for Quint formal verification integration.
//! Following the algebraic effects architecture, these traits define capabilities
//! without implementation details.
//!
//! # Effect Classification
//!
//! - **Category**: Testing/Simulation Effect
//! - **Implementation**: `aura-quint` (Layer 8)
//! - **Usage**: Quint formal verification, property evaluation, model checking
//!
//! This is a testing/simulation effect for formal verification integration. Provides
//! interfaces for Quint property evaluation, state space exploration, and model
//! checking. Handlers in `aura-quint` integrate with the Quint formal verification
//! toolchain.

use crate::Result;
use serde_json::Value;
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
    async fn evaluate_property(
        &self,
        property: &Property,
        state: &Value,
    ) -> Result<EvaluationResult>;

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
    async fn verify_property(
        &self,
        property: &Property,
        state: &Value,
    ) -> Result<VerificationResult>;

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

    pub fn new_random() -> Self {
        let h = crate::hash::hash(b"quint-property-id");
        Self(hex::encode(&h[..16]))
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

    /// Generate a verification ID from entropy bytes.
    ///
    /// Use with `RandomEffects::random_bytes_32()` for runtime generation:
    /// ```ignore
    /// let entropy = random_effects.random_bytes_32().await;
    /// let id = VerificationId::generate_from_entropy(entropy);
    /// ```
    pub fn generate_from_entropy(entropy: [u8; 32]) -> Self {
        let mut uuid_bytes = [0u8; 16];
        uuid_bytes.copy_from_slice(&entropy[..16]);
        Self(uuid::Uuid::from_bytes(uuid_bytes).to_string())
    }
}

impl Property {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        kind: PropertyKind,
        expression: impl Into<String>,
    ) -> Self {
        Self {
            id: PropertyId::new(id),
            name: name.into(),
            kind,
            expression: expression.into(),
            description: None,
        }
    }

    #[must_use]
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

    #[must_use]
    pub fn with_property(mut self, property: Property) -> Self {
        self.properties.push(property);
        self
    }

    #[must_use]
    pub fn with_context(mut self, context: Value) -> Self {
        self.context = context;
        self
    }
}

// =============================================================================
// Generative Simulation Types and Traits
// =============================================================================

use std::collections::HashMap;

/// Result of executing a Quint action against Aura state
#[derive(Debug, Clone)]
pub struct ActionResult {
    /// Whether the action executed successfully
    pub success: bool,
    /// The resulting state after action execution
    pub resulting_state: Value,
    /// Effects produced by the action
    pub effects_produced: Vec<ActionEffect>,
    /// Error message if action failed
    pub error: Option<String>,
}

/// An effect produced by action execution
#[derive(Debug, Clone)]
pub struct ActionEffect {
    /// Type of effect (e.g., "journal_write", "transport_send")
    pub effect_type: String,
    /// Parameters of the effect
    pub parameters: Value,
}

/// Description of an available action
#[derive(Debug, Clone)]
pub struct ActionDescriptor {
    /// Action name (must match Quint action name)
    pub name: String,
    /// Parameter schema as JSON Schema
    pub parameter_schema: Value,
    /// Human-readable description
    pub description: String,
    /// Whether this action can be executed in current state
    pub enabled: bool,
}

/// Quint simulation effects for generative testing (Layer 1: Foundation)
///
/// This trait bridges Quint actions to actual Aura effect execution, enabling
/// generative simulations where Quint specifications drive real state transitions.
///
/// # Usage Pattern
///
/// ```ignore
/// // Execute a Quint action against real Aura state
/// let result = effects.execute_action(
///     "completeTransportOp",
///     &json!({"ctx": "ctx1", "src": "auth1", "dst": "auth2", "cost": 5}),
///     &nondet_picks,
/// ).await?;
///
/// // Extract state for property evaluation
/// let quint_state = effects.extract_state().await?;
/// ```
#[async_trait::async_trait]
pub trait QuintSimulationEffects: Send + Sync {
    /// Execute a Quint action against real Aura state
    ///
    /// This method maps a Quint action name and parameters to the corresponding
    /// Aura effect handler invocation, executing real state transitions.
    ///
    /// # Arguments
    /// * `action_name` - The name of the Quint action (e.g., "initContext", "completeTransportOp")
    /// * `params` - Action parameters as JSON
    /// * `nondet_picks` - Non-deterministic choices made by Quint (for seeding randomness)
    ///
    /// # Returns
    /// `ActionResult` containing success status, new state, and effects produced
    async fn execute_action(
        &self,
        action_name: &str,
        params: &Value,
        nondet_picks: &HashMap<String, Value>,
    ) -> Result<ActionResult>;

    /// Extract current Aura state as Quint-compatible JSON
    ///
    /// This method converts the current Aura runtime state into a JSON format
    /// that matches Quint variable declarations, enabling property evaluation.
    async fn extract_state(&self) -> Result<Value>;

    /// Apply non-deterministic picks to seed random effects
    ///
    /// This allows Quint traces to be replayed deterministically by seeding
    /// any random operations with the choices recorded in the ITF trace.
    fn apply_nondet_picks(&mut self, picks: &HashMap<String, Value>);

    /// Get available actions for current state
    ///
    /// Returns descriptors for all actions that can be executed in the current
    /// state, enabling state space exploration.
    fn available_actions(&self) -> Vec<ActionDescriptor>;

    /// Reset state to initial configuration
    ///
    /// Resets the Aura state to a clean initial state, useful for starting
    /// new simulation runs.
    async fn reset_state(&mut self) -> Result<()>;
}

/// Trait for Aura types that can be mapped to/from Quint representations
///
/// This trait enables type-safe bidirectional conversion between Aura domain
/// types and their Quint JSON representations.
///
/// # Example Implementation
///
/// ```ignore
/// impl QuintMappable for AuthorityId {
///     fn to_quint(&self) -> Value {
///         Value::String(self.to_string())
///     }
///
///     fn from_quint(value: &Value) -> Result<Self> {
///         value.as_str()
///             .ok_or_else(|| AuraError::invalid("expected string"))
///             .and_then(|s| AuthorityId::from_str(s))
///     }
///
///     fn quint_type_name() -> &'static str {
///         "AuthorityId"
///     }
/// }
/// ```
pub trait QuintMappable: Sized {
    /// Convert this Aura type to its Quint JSON representation
    fn to_quint(&self) -> Value;

    /// Parse a Quint JSON value into this Aura type
    fn from_quint(value: &Value) -> Result<Self>;

    /// The Quint type name for documentation and schema generation
    fn quint_type_name() -> &'static str;
}

/// Marker trait for types that support Quint state extraction
///
/// Types implementing this trait can be included in the state extraction
/// for property evaluation.
pub trait QuintStateExtractable {
    /// Extract state relevant for Quint property evaluation
    fn extract_quint_state(&self) -> Value;
}

// =============================================================================
// QuintMappable implementations for primitive types
// =============================================================================

impl QuintMappable for String {
    fn to_quint(&self) -> Value {
        Value::String(self.clone())
    }

    fn from_quint(value: &Value) -> Result<Self> {
        value
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| crate::AuraError::invalid("expected string for Quint str type"))
    }

    fn quint_type_name() -> &'static str {
        "str"
    }
}

impl QuintMappable for i64 {
    fn to_quint(&self) -> Value {
        Value::Number((*self).into())
    }

    fn from_quint(value: &Value) -> Result<Self> {
        value
            .as_i64()
            .ok_or_else(|| crate::AuraError::invalid("expected integer for Quint int type"))
    }

    fn quint_type_name() -> &'static str {
        "int"
    }
}

impl QuintMappable for u64 {
    fn to_quint(&self) -> Value {
        Value::Number((*self).into())
    }

    fn from_quint(value: &Value) -> Result<Self> {
        value.as_u64().ok_or_else(|| {
            crate::AuraError::invalid("expected non-negative integer for Quint int type")
        })
    }

    fn quint_type_name() -> &'static str {
        "int"
    }
}

impl QuintMappable for bool {
    fn to_quint(&self) -> Value {
        Value::Bool(*self)
    }

    fn from_quint(value: &Value) -> Result<Self> {
        value
            .as_bool()
            .ok_or_else(|| crate::AuraError::invalid("expected boolean for Quint bool type"))
    }

    fn quint_type_name() -> &'static str {
        "bool"
    }
}

impl<T: QuintMappable> QuintMappable for Vec<T> {
    fn to_quint(&self) -> Value {
        Value::Array(self.iter().map(|item| item.to_quint()).collect())
    }

    fn from_quint(value: &Value) -> Result<Self> {
        value
            .as_array()
            .ok_or_else(|| crate::AuraError::invalid("expected array for Quint List type"))?
            .iter()
            .map(T::from_quint)
            .collect()
    }

    fn quint_type_name() -> &'static str {
        "List"
    }
}

impl<T: QuintMappable + std::hash::Hash + Eq> QuintMappable for std::collections::HashSet<T> {
    fn to_quint(&self) -> Value {
        Value::Array(self.iter().map(|item| item.to_quint()).collect())
    }

    fn from_quint(value: &Value) -> Result<Self> {
        value
            .as_array()
            .ok_or_else(|| crate::AuraError::invalid("expected array for Quint Set type"))?
            .iter()
            .map(T::from_quint)
            .collect()
    }

    fn quint_type_name() -> &'static str {
        "Set"
    }
}

impl<K: QuintMappable + std::hash::Hash + Eq, V: QuintMappable> QuintMappable
    for std::collections::HashMap<K, V>
where
    K: ToString + std::str::FromStr,
{
    fn to_quint(&self) -> Value {
        let map: serde_json::Map<String, Value> = self
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_quint()))
            .collect();
        Value::Object(map)
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or_else(|| crate::AuraError::invalid("expected object for Quint Map type"))?;

        obj.iter()
            .map(|(k, v)| {
                let key = K::from_str(k)
                    .map_err(|_| crate::AuraError::invalid(format!("invalid map key: {k}")))?;
                let val = V::from_quint(v)?;
                Ok((key, val))
            })
            .collect()
    }

    fn quint_type_name() -> &'static str {
        "Map"
    }
}

// =============================================================================
// QuintMappable implementations for core Aura types
// =============================================================================

use crate::types::{epochs::Epoch, AuthorityId, ContextId, FlowBudget};

impl QuintMappable for AuthorityId {
    fn to_quint(&self) -> Value {
        // Use the display format which includes the "authority-" prefix
        Value::String(self.to_string())
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let s = value
            .as_str()
            .ok_or_else(|| crate::AuraError::invalid("expected string for AuthorityId"))?;
        s.parse()
            .map_err(|e| crate::AuraError::invalid(format!("invalid AuthorityId: {e}")))
    }

    fn quint_type_name() -> &'static str {
        "AuthorityId"
    }
}

impl QuintMappable for ContextId {
    fn to_quint(&self) -> Value {
        // Use the display format which includes the "context:" prefix
        Value::String(self.to_string())
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let s = value
            .as_str()
            .ok_or_else(|| crate::AuraError::invalid("expected string for ContextId"))?;
        s.parse()
            .map_err(|e| crate::AuraError::invalid(format!("invalid ContextId: {e}")))
    }

    fn quint_type_name() -> &'static str {
        "ContextId"
    }
}

impl QuintMappable for Epoch {
    fn to_quint(&self) -> Value {
        Value::Number(self.value().into())
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let n = value
            .as_u64()
            .ok_or_else(|| crate::AuraError::invalid("expected integer for Epoch"))?;
        Ok(Epoch::new(n))
    }

    fn quint_type_name() -> &'static str {
        "Epoch"
    }
}

impl QuintMappable for FlowBudget {
    fn to_quint(&self) -> Value {
        let mut map = serde_json::Map::new();
        map.insert("limit".to_string(), Value::Number(self.limit.into()));
        map.insert("spent".to_string(), Value::Number(self.spent.into()));
        map.insert("epoch".to_string(), self.epoch.to_quint());
        Value::Object(map)
    }

    fn from_quint(value: &Value) -> Result<Self> {
        let obj = value
            .as_object()
            .ok_or_else(|| crate::AuraError::invalid("expected object for FlowBudget"))?;

        let limit = obj
            .get("limit")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| crate::AuraError::invalid("missing or invalid 'limit' field"))?;

        let spent = obj
            .get("spent")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| crate::AuraError::invalid("missing or invalid 'spent' field"))?;

        let epoch_value = obj
            .get("epoch")
            .ok_or_else(|| crate::AuraError::invalid("missing 'epoch' field"))?;
        let epoch = Epoch::from_quint(epoch_value)?;

        Ok(FlowBudget {
            limit,
            spent,
            epoch,
        })
    }

    fn quint_type_name() -> &'static str {
        "FlowBudget"
    }
}

// =============================================================================
// Unit Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authority_id_quint_roundtrip() {
        let original = AuthorityId::new_from_entropy([42u8; 32]);
        let quint_value = original.to_quint();
        let restored = AuthorityId::from_quint(&quint_value).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_context_id_quint_roundtrip() {
        let original = ContextId::new_from_entropy([43u8; 32]);
        let quint_value = original.to_quint();
        let restored = ContextId::from_quint(&quint_value).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_epoch_quint_roundtrip() {
        let original = Epoch::new(42);
        let quint_value = original.to_quint();
        let restored = Epoch::from_quint(&quint_value).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_flow_budget_quint_roundtrip() {
        let original = FlowBudget {
            limit: 1000,
            spent: 250,
            epoch: Epoch::new(5),
        };
        let quint_value = original.to_quint();
        let restored = FlowBudget::from_quint(&quint_value).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_flow_budget_quint_structure() {
        let budget = FlowBudget {
            limit: 100,
            spent: 25,
            epoch: Epoch::new(3),
        };
        let quint_value = budget.to_quint();

        // Verify the JSON structure matches Quint expectations
        let obj = quint_value.as_object().unwrap();
        assert_eq!(obj.get("limit").unwrap().as_u64().unwrap(), 100);
        assert_eq!(obj.get("spent").unwrap().as_u64().unwrap(), 25);
        assert_eq!(obj.get("epoch").unwrap().as_u64().unwrap(), 3);
    }

    #[test]
    fn test_vec_authority_id_quint_roundtrip() {
        let original = vec![
            AuthorityId::new_from_entropy([1u8; 32]),
            AuthorityId::new_from_entropy([2u8; 32]),
        ];
        let quint_value = original.to_quint();
        let restored = Vec::<AuthorityId>::from_quint(&quint_value).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_hashset_authority_id_quint_roundtrip() {
        use std::collections::HashSet;
        let mut original = HashSet::new();
        original.insert(AuthorityId::new_from_entropy([1u8; 32]));
        original.insert(AuthorityId::new_from_entropy([2u8; 32]));

        let quint_value = original.to_quint();
        let restored = HashSet::<AuthorityId>::from_quint(&quint_value).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn test_invalid_authority_id_from_quint() {
        let invalid = Value::Number(42.into());
        let result = AuthorityId::from_quint(&invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_flow_budget_from_quint() {
        let invalid = Value::String("not a budget".to_string());
        let result = FlowBudget::from_quint(&invalid);
        assert!(result.is_err());
    }
}
