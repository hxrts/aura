//! State Mapper for Bidirectional Aura-Quint State Conversion
//!
//! This module provides infrastructure for converting between Aura's runtime
//! state and Quint's JSON-based state representation, enabling property
//! evaluation and state synchronization during generative simulations.
//!
//! # Architecture
//!
//! The state mapper uses the `QuintMappable` trait (defined in `aura-core`)
//! to handle type-specific conversions:
//!
//! ```text
//! Aura State (typed) ←→ StateMapper ←→ Quint State (JSON)
//!                           ↓
//!                    Type Registry
//!                  (QuintMappable impls)
//! ```
//!
//! # Usage
//!
//! ```ignore
//! let mapper = StateMapper::new()
//!     .with_variable("authorities", &authority_set)
//!     .with_variable("budgets", &budget_map);
//!
//! let quint_state = mapper.to_quint();
//! let restored = mapper.update_from_quint(&quint_state)?;
//! ```

use aura_core::effects::QuintMappable;
use aura_core::Result;
use serde_json::{Map, Value};
use std::any::TypeId;
use std::collections::HashMap;

/// Type converter function signature
type ConverterFn = Box<dyn Fn(&Value) -> Result<Box<dyn std::any::Any + Send>> + Send + Sync>;
type ToQuintFn = Box<dyn Fn(&dyn std::any::Any) -> Value + Send + Sync>;

/// Configuration for a type mapping
struct TypeMapping {
    quint_type_name: &'static str,
    from_quint: ConverterFn,
    to_quint: ToQuintFn,
}

/// Bidirectional state mapping between Aura and Quint
///
/// Manages conversion of state variables between Aura's typed representation
/// and Quint's JSON format for property evaluation.
pub struct StateMapper {
    /// Type mappings keyed by TypeId
    type_mappings: HashMap<TypeId, TypeMapping>,
    /// Current state variables (name -> JSON value)
    variables: Map<String, Value>,
    /// Variable metadata (name -> type info)
    variable_types: HashMap<String, TypeId>,
}

impl StateMapper {
    /// Create a new empty state mapper
    pub fn new() -> Self {
        Self {
            type_mappings: HashMap::new(),
            variables: Map::new(),
            variable_types: HashMap::new(),
        }
    }

    /// Register a type mapping for a QuintMappable type
    ///
    /// This enables the mapper to handle values of type `T`.
    pub fn register_type<T: QuintMappable + Clone + Send + Sync + 'static>(&mut self) {
        let type_id = TypeId::of::<T>();

        let from_quint: ConverterFn = Box::new(|value: &Value| {
            let typed = T::from_quint(value)?;
            Ok(Box::new(typed) as Box<dyn std::any::Any + Send>)
        });

        let to_quint: ToQuintFn = Box::new(|any: &dyn std::any::Any| {
            if let Some(typed) = any.downcast_ref::<T>() {
                typed.to_quint()
            } else {
                Value::Null
            }
        });

        self.type_mappings.insert(
            type_id,
            TypeMapping {
                quint_type_name: T::quint_type_name(),
                from_quint,
                to_quint,
            },
        );
    }

    /// Set a state variable value
    ///
    /// The value is converted to Quint JSON format using `QuintMappable`.
    pub fn set_variable<T: QuintMappable + 'static>(
        &mut self,
        name: impl Into<String>,
        value: &T,
    ) -> &mut Self {
        let name = name.into();
        self.variables.insert(name.clone(), value.to_quint());
        self.variable_types.insert(name, TypeId::of::<T>());
        self
    }

    /// Builder pattern for setting variables
    pub fn with_variable<T: QuintMappable + 'static>(
        mut self,
        name: impl Into<String>,
        value: &T,
    ) -> Self {
        self.set_variable(name, value);
        self
    }

    /// Get a state variable value
    ///
    /// Returns `None` if the variable doesn't exist.
    pub fn get_variable(&self, name: &str) -> Option<&Value> {
        self.variables.get(name)
    }

    /// Get a typed state variable value
    ///
    /// Returns `None` if the variable doesn't exist or type doesn't match.
    pub fn get_typed<T: QuintMappable>(&self, name: &str) -> Result<Option<T>> {
        match self.variables.get(name) {
            Some(value) => Ok(Some(T::from_quint(value)?)),
            None => Ok(None),
        }
    }

    /// Remove a state variable
    pub fn remove_variable(&mut self, name: &str) -> Option<Value> {
        self.variable_types.remove(name);
        self.variables.remove(name)
    }

    /// Check if a variable exists
    pub fn has_variable(&self, name: &str) -> bool {
        self.variables.contains_key(name)
    }

    /// Get all variable names
    pub fn variable_names(&self) -> Vec<&str> {
        self.variables.keys().map(|s| s.as_str()).collect()
    }

    /// Convert all state to Quint JSON format
    pub fn to_quint(&self) -> Value {
        Value::Object(self.variables.clone())
    }

    /// Update state from Quint JSON
    ///
    /// Only updates variables that exist in both the mapper and the input.
    /// Returns the list of updated variable names.
    pub fn update_from_quint(&mut self, quint_state: &Value) -> Result<Vec<String>> {
        let obj = quint_state
            .as_object()
            .ok_or_else(|| aura_core::AuraError::invalid("expected object for Quint state"))?;

        let mut updated = Vec::new();

        for (name, value) in obj {
            if self.variables.contains_key(name) {
                self.variables.insert(name.clone(), value.clone());
                updated.push(name.clone());
            }
        }

        Ok(updated)
    }

    /// Merge another state mapper's variables into this one
    pub fn merge(&mut self, other: &StateMapper) {
        for (name, value) in &other.variables {
            self.variables.insert(name.clone(), value.clone());
        }
        for (name, type_id) in &other.variable_types {
            self.variable_types.insert(name.clone(), *type_id);
        }
    }

    /// Create a snapshot of current state
    pub fn snapshot(&self) -> StateSnapshot {
        StateSnapshot {
            variables: self.variables.clone(),
            variable_types: self.variable_types.clone(),
        }
    }

    /// Restore state from a snapshot
    pub fn restore(&mut self, snapshot: StateSnapshot) {
        self.variables = snapshot.variables;
        self.variable_types = snapshot.variable_types;
    }

    /// Clear all state variables
    pub fn clear(&mut self) {
        self.variables.clear();
        self.variable_types.clear();
    }

    /// Number of state variables
    pub fn len(&self) -> usize {
        self.variables.len()
    }

    /// Check if state is empty
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }
}

impl Default for StateMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for StateMapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StateMapper")
            .field("variables", &self.variable_names())
            .finish()
    }
}

/// Snapshot of mapper state for checkpointing
#[derive(Debug, Clone)]
pub struct StateSnapshot {
    variables: Map<String, Value>,
    variable_types: HashMap<String, TypeId>,
}

impl StateSnapshot {
    /// Get the JSON representation
    pub fn to_value(&self) -> Value {
        Value::Object(self.variables.clone())
    }

    /// Number of variables in snapshot
    pub fn len(&self) -> usize {
        self.variables.len()
    }

    /// Check if snapshot is empty
    pub fn is_empty(&self) -> bool {
        self.variables.is_empty()
    }
}

// =============================================================================
// Specialized State Mappers
// =============================================================================

/// State mapper pre-configured with common Aura types
pub struct AuraStateMapper {
    inner: StateMapper,
}

impl AuraStateMapper {
    /// Create a new Aura state mapper with standard type registrations
    pub fn new() -> Self {
        use aura_core::types::{AuthorityId, ContextId, Epoch, FlowBudget};

        let mut mapper = StateMapper::new();
        mapper.register_type::<String>();
        mapper.register_type::<i64>();
        mapper.register_type::<u64>();
        mapper.register_type::<bool>();
        mapper.register_type::<AuthorityId>();
        mapper.register_type::<ContextId>();
        mapper.register_type::<Epoch>();
        mapper.register_type::<FlowBudget>();

        Self { inner: mapper }
    }

    /// Access the inner state mapper
    pub fn inner(&self) -> &StateMapper {
        &self.inner
    }

    /// Access the inner state mapper mutably
    pub fn inner_mut(&mut self) -> &mut StateMapper {
        &mut self.inner
    }

    /// Set a state variable
    pub fn set<T: QuintMappable + 'static>(&mut self, name: impl Into<String>, value: &T) {
        self.inner.set_variable(name, value);
    }

    /// Get a typed state variable
    pub fn get<T: QuintMappable>(&self, name: &str) -> Result<Option<T>> {
        self.inner.get_typed(name)
    }

    /// Convert to Quint state
    pub fn to_quint(&self) -> Value {
        self.inner.to_quint()
    }

    /// Update from Quint state
    pub fn update_from_quint(&mut self, state: &Value) -> Result<Vec<String>> {
        self.inner.update_from_quint(state)
    }
}

impl Default for AuraStateMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for AuraStateMapper {
    type Target = StateMapper;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for AuraStateMapper {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

// =============================================================================
// Simulation State Mapper
// =============================================================================

use crate::quint::aura_state_extractors::QuintSimulationState;

/// State mapper specialized for simulation state
///
/// Provides bidirectional mapping between `QuintSimulationState` and Quint JSON,
/// supporting the capability properties verification workflow.
pub struct SimulationStateMapper {
    inner: StateMapper,
}

impl SimulationStateMapper {
    /// Create a new simulation state mapper
    pub fn new() -> Self {
        use aura_core::types::{AuthorityId, ContextId, Epoch, FlowBudget};

        let mut mapper = StateMapper::new();
        mapper.register_type::<String>();
        mapper.register_type::<i64>();
        mapper.register_type::<u64>();
        mapper.register_type::<bool>();
        mapper.register_type::<AuthorityId>();
        mapper.register_type::<ContextId>();
        mapper.register_type::<Epoch>();
        mapper.register_type::<FlowBudget>();

        Self { inner: mapper }
    }

    /// Load state from a `QuintSimulationState`
    ///
    /// This converts the typed Aura state into the Quint JSON format
    /// for property evaluation.
    pub fn load_from_simulation_state(&mut self, state: &QuintSimulationState) {
        let quint_state = state.to_quint();

        // Extract and set individual variables for fine-grained access
        if let Some(budgets) = quint_state.get("budgets") {
            self.inner
                .variables
                .insert("budgets".to_string(), budgets.clone());
        }
        if let Some(tokens) = quint_state.get("tokens") {
            self.inner
                .variables
                .insert("tokens".to_string(), tokens.clone());
        }
        if let Some(epochs) = quint_state.get("current_epoch") {
            self.inner
                .variables
                .insert("current_epoch".to_string(), epochs.clone());
        }
        if let Some(ops) = quint_state.get("completed_ops") {
            self.inner
                .variables
                .insert("completed_ops".to_string(), ops.clone());
        }

        // Also store the combined state for convenience
        self.inner
            .variables
            .insert("simulation_state".to_string(), quint_state);
    }

    /// Extract changes back to a `QuintSimulationState`
    ///
    /// This is used for non-deterministic picks where Quint may have
    /// modified state variables during action execution.
    pub fn extract_to_simulation_state(&self) -> Result<QuintSimulationState> {
        let mut state = QuintSimulationState::new();

        // Try to get from combined state first
        if let Some(combined) = self.inner.variables.get("simulation_state") {
            state.update_from_quint(combined).map_err(|e| {
                aura_core::AuraError::invalid(format!("failed to update from quint: {}", e))
            })?;
        } else {
            // Build from individual variables
            let quint_state = self.inner.to_quint();
            state.update_from_quint(&quint_state).map_err(|e| {
                aura_core::AuraError::invalid(format!("failed to update from quint: {}", e))
            })?;
        }

        Ok(state)
    }

    /// Update specific fields from Quint JSON (for non-deterministic updates)
    ///
    /// This is called after Quint actions that may have made non-deterministic
    /// choices, allowing the simulation to incorporate those choices.
    pub fn apply_nondet_updates(&mut self, updates: &Value) -> Result<Vec<String>> {
        let mut updated_vars = Vec::new();

        if let Some(obj) = updates.as_object() {
            for (key, value) in obj {
                if self.inner.variables.contains_key(key) {
                    self.inner.variables.insert(key.clone(), value.clone());
                    updated_vars.push(key.clone());
                }
            }

            // If we got individual updates, rebuild the combined state
            if !updated_vars.is_empty() && !updated_vars.contains(&"simulation_state".to_string()) {
                let combined = self.build_combined_state();
                self.inner
                    .variables
                    .insert("simulation_state".to_string(), combined);
            }
        }

        Ok(updated_vars)
    }

    /// Build combined simulation state from individual variables
    fn build_combined_state(&self) -> Value {
        serde_json::json!({
            "budgets": self.inner.variables.get("budgets").cloned().unwrap_or(serde_json::json!({})),
            "tokens": self.inner.variables.get("tokens").cloned().unwrap_or(serde_json::json!({})),
            "current_epoch": self.inner.variables.get("current_epoch").cloned().unwrap_or(serde_json::json!({})),
            "completed_ops": self.inner.variables.get("completed_ops").cloned().unwrap_or(serde_json::json!([]))
        })
    }

    /// Get the Quint state for property evaluation
    pub fn to_quint(&self) -> Value {
        self.inner.to_quint()
    }

    /// Create a snapshot
    pub fn snapshot(&self) -> StateSnapshot {
        self.inner.snapshot()
    }

    /// Restore from snapshot
    pub fn restore(&mut self, snapshot: StateSnapshot) {
        self.inner.restore(snapshot);
    }

    /// Access the inner mapper
    pub fn inner(&self) -> &StateMapper {
        &self.inner
    }

    /// Access the inner mapper mutably
    pub fn inner_mut(&mut self) -> &mut StateMapper {
        &mut self.inner
    }
}

impl Default for SimulationStateMapper {
    fn default() -> Self {
        Self::new()
    }
}

impl std::ops::Deref for SimulationStateMapper {
    type Target = StateMapper;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for SimulationStateMapper {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

// =============================================================================
// State Diff Utilities
// =============================================================================

/// Difference between two states
#[derive(Debug, Clone, Default)]
pub struct StateDiff {
    /// Variables that were added
    pub added: HashMap<String, Value>,
    /// Variables that were removed
    pub removed: HashMap<String, Value>,
    /// Variables that were modified (old_value, new_value)
    pub modified: HashMap<String, (Value, Value)>,
}

impl StateDiff {
    /// Compute the difference between two states
    pub fn compute(old_state: &Value, new_state: &Value) -> Self {
        let old_obj = old_state.as_object();
        let new_obj = new_state.as_object();

        let (old_map, new_map) = match (old_obj, new_obj) {
            (Some(old), Some(new)) => (old, new),
            _ => return StateDiff::default(),
        };

        let mut diff = StateDiff::default();

        // Find added and modified
        for (key, new_val) in new_map {
            match old_map.get(key) {
                Some(old_val) if old_val != new_val => {
                    diff.modified
                        .insert(key.clone(), (old_val.clone(), new_val.clone()));
                }
                None => {
                    diff.added.insert(key.clone(), new_val.clone());
                }
                _ => {}
            }
        }

        // Find removed
        for (key, old_val) in old_map {
            if !new_map.contains_key(key) {
                diff.removed.insert(key.clone(), old_val.clone());
            }
        }

        diff
    }

    /// Check if there are any changes
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.modified.is_empty()
    }

    /// Total number of changes
    pub fn change_count(&self) -> usize {
        self.added.len() + self.removed.len() + self.modified.len()
    }

    /// Apply this diff to a state
    pub fn apply(&self, state: &mut Value) {
        if let Some(obj) = state.as_object_mut() {
            // Apply removals
            for key in self.removed.keys() {
                obj.remove(key);
            }

            // Apply additions
            for (key, value) in &self.added {
                obj.insert(key.clone(), value.clone());
            }

            // Apply modifications
            for (key, (_, new_val)) in &self.modified {
                obj.insert(key.clone(), new_val.clone());
            }
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::types::{AuthorityId, Epoch, FlowBudget};

    #[test]
    fn test_state_mapper_basic() {
        let mut mapper = StateMapper::new();
        assert!(mapper.is_empty());

        mapper.set_variable("counter", &42i64);
        assert_eq!(mapper.len(), 1);
        assert!(mapper.has_variable("counter"));

        let value = mapper.get_variable("counter").unwrap();
        assert_eq!(value.as_i64().unwrap(), 42);
    }

    #[test]
    fn test_state_mapper_typed_get() {
        let mut mapper = StateMapper::new();
        let budget = FlowBudget {
            limit: 100,
            spent: 25,
            epoch: Epoch::new(5),
        };

        mapper.set_variable("budget", &budget);

        let retrieved: FlowBudget = mapper.get_typed("budget").unwrap().unwrap();
        assert_eq!(retrieved.limit, 100);
        assert_eq!(retrieved.spent, 25);
        assert_eq!(retrieved.epoch, Epoch::new(5));
    }

    #[test]
    fn test_state_mapper_authority_id() {
        let mut mapper = StateMapper::new();
        let auth = AuthorityId::new_from_entropy([42u8; 32]);

        mapper.set_variable("authority", &auth);

        let retrieved: AuthorityId = mapper.get_typed("authority").unwrap().unwrap();
        assert_eq!(retrieved, auth);
    }

    #[test]
    fn test_state_mapper_to_quint() {
        let mut mapper = StateMapper::new();
        mapper.set_variable("x", &10i64);
        mapper.set_variable("y", &20i64);

        let quint = mapper.to_quint();
        let obj = quint.as_object().unwrap();

        assert_eq!(obj.get("x").unwrap().as_i64().unwrap(), 10);
        assert_eq!(obj.get("y").unwrap().as_i64().unwrap(), 20);
    }

    #[test]
    fn test_state_mapper_update_from_quint() {
        let mut mapper = StateMapper::new();
        mapper.set_variable("counter", &0i64);
        mapper.set_variable("flag", &false);

        let new_state = serde_json::json!({
            "counter": 42,
            "flag": true,
            "extra": "ignored"
        });

        let updated = mapper.update_from_quint(&new_state).unwrap();

        assert_eq!(updated.len(), 2);
        assert!(updated.contains(&"counter".to_string()));
        assert!(updated.contains(&"flag".to_string()));

        // "extra" should not be added since it wasn't in original
        assert!(!mapper.has_variable("extra"));

        let counter: i64 = mapper.get_typed("counter").unwrap().unwrap();
        assert_eq!(counter, 42);
    }

    #[test]
    fn test_state_mapper_snapshot() {
        let mut mapper = StateMapper::new();
        mapper.set_variable("a", &1i64);
        mapper.set_variable("b", &2i64);

        let snapshot = mapper.snapshot();

        mapper.set_variable("a", &100i64);
        mapper.set_variable("c", &3i64);

        mapper.restore(snapshot);

        assert_eq!(mapper.len(), 2);
        let a: i64 = mapper.get_typed("a").unwrap().unwrap();
        assert_eq!(a, 1);
        assert!(!mapper.has_variable("c"));
    }

    #[test]
    fn test_state_mapper_clear() {
        let mut mapper = StateMapper::new();
        mapper.set_variable("x", &1i64);
        mapper.set_variable("y", &2i64);

        assert_eq!(mapper.len(), 2);
        mapper.clear();
        assert!(mapper.is_empty());
    }

    #[test]
    fn test_aura_state_mapper() {
        let mut mapper = AuraStateMapper::new();

        let auth = AuthorityId::new_from_entropy([1u8; 32]);
        let budget = FlowBudget {
            limit: 50,
            spent: 10,
            epoch: Epoch::new(1),
        };

        mapper.set("authority", &auth);
        mapper.set("budget", &budget);

        let retrieved_auth: AuthorityId = mapper.get("authority").unwrap().unwrap();
        let retrieved_budget: FlowBudget = mapper.get("budget").unwrap().unwrap();

        assert_eq!(retrieved_auth, auth);
        assert_eq!(retrieved_budget.limit, 50);
    }

    #[test]
    fn test_state_diff_compute() {
        let old = serde_json::json!({
            "a": 1,
            "b": 2,
            "c": 3
        });

        let new = serde_json::json!({
            "a": 1,      // unchanged
            "b": 20,     // modified
            "d": 4       // added
            // c removed
        });

        let diff = StateDiff::compute(&old, &new);

        assert_eq!(diff.added.len(), 1);
        assert!(diff.added.contains_key("d"));

        assert_eq!(diff.removed.len(), 1);
        assert!(diff.removed.contains_key("c"));

        assert_eq!(diff.modified.len(), 1);
        assert!(diff.modified.contains_key("b"));
        let (old_b, new_b) = diff.modified.get("b").unwrap();
        assert_eq!(old_b.as_i64().unwrap(), 2);
        assert_eq!(new_b.as_i64().unwrap(), 20);
    }

    #[test]
    fn test_state_diff_apply() {
        let mut state = serde_json::json!({
            "a": 1,
            "b": 2
        });

        let diff = StateDiff {
            added: [("c".to_string(), serde_json::json!(3))]
                .into_iter()
                .collect(),
            removed: [("a".to_string(), serde_json::json!(1))]
                .into_iter()
                .collect(),
            modified: [(
                "b".to_string(),
                (serde_json::json!(2), serde_json::json!(20)),
            )]
            .into_iter()
            .collect(),
        };

        diff.apply(&mut state);

        let obj = state.as_object().unwrap();
        assert!(!obj.contains_key("a"));
        assert_eq!(obj.get("b").unwrap().as_i64().unwrap(), 20);
        assert_eq!(obj.get("c").unwrap().as_i64().unwrap(), 3);
    }

    #[test]
    fn test_state_diff_empty() {
        let state = serde_json::json!({"a": 1});
        let diff = StateDiff::compute(&state, &state);
        assert!(diff.is_empty());
        assert_eq!(diff.change_count(), 0);
    }

    // =========================================================================
    // SimulationStateMapper Tests
    // =========================================================================

    use crate::quint::aura_state_extractors::QuintSimulationState;
    use aura_core::types::ContextId;

    fn create_test_simulation_state() -> QuintSimulationState {
        let mut state = QuintSimulationState::new();
        let ctx = ContextId::new_from_entropy([1u8; 32]);
        let auth = AuthorityId::new_from_entropy([2u8; 32]);

        state.init_context(ctx, auth, 100);
        state.init_authority(auth, 4);
        state
    }

    #[test]
    fn test_simulation_state_mapper_load() {
        let sim_state = create_test_simulation_state();
        let mut mapper = SimulationStateMapper::new();

        mapper.load_from_simulation_state(&sim_state);

        // Verify variables are loaded
        assert!(mapper.has_variable("budgets"));
        assert!(mapper.has_variable("tokens"));
        assert!(mapper.has_variable("current_epoch"));
        assert!(mapper.has_variable("completed_ops"));
        assert!(mapper.has_variable("simulation_state"));
    }

    #[test]
    fn test_simulation_state_mapper_extract() {
        let original = create_test_simulation_state();
        let mut mapper = SimulationStateMapper::new();

        mapper.load_from_simulation_state(&original);
        let extracted = mapper.extract_to_simulation_state().unwrap();

        // Verify budgets match
        assert_eq!(original.budgets.len(), extracted.budgets.len());
        assert_eq!(original.tokens.len(), extracted.tokens.len());
    }

    #[test]
    fn test_simulation_state_mapper_nondet_updates() {
        let sim_state = create_test_simulation_state();
        let mut mapper = SimulationStateMapper::new();

        mapper.load_from_simulation_state(&sim_state);

        // Simulate Quint making a non-deterministic update
        let updates = serde_json::json!({
            "tokens": {
                "authority-test": {
                    "cap_level": 2,
                    "attenuation_count": 1
                }
            }
        });

        let updated = mapper.apply_nondet_updates(&updates).unwrap();
        assert!(updated.contains(&"tokens".to_string()));

        // simulation_state should be rebuilt
        assert!(mapper.has_variable("simulation_state"));
    }

    #[test]
    fn test_simulation_state_mapper_snapshot_restore() {
        let sim_state = create_test_simulation_state();
        let mut mapper = SimulationStateMapper::new();

        mapper.load_from_simulation_state(&sim_state);
        let snapshot = mapper.snapshot();

        // Modify state
        mapper
            .inner_mut()
            .variables
            .insert("extra".to_string(), serde_json::json!("test"));

        // Restore
        mapper.restore(snapshot);
        assert!(!mapper.has_variable("extra"));
        assert!(mapper.has_variable("budgets"));
    }

    #[test]
    fn test_simulation_state_mapper_to_quint() {
        let sim_state = create_test_simulation_state();
        let mut mapper = SimulationStateMapper::new();

        mapper.load_from_simulation_state(&sim_state);
        let quint = mapper.to_quint();

        let obj = quint.as_object().unwrap();
        assert!(obj.contains_key("budgets"));
        assert!(obj.contains_key("tokens"));
        assert!(obj.contains_key("simulation_state"));
    }
}
