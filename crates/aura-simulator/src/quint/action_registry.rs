//! Action Registry for Quint-to-Aura Action Mapping
//!
//! This module provides infrastructure for mapping Quint action names to Aura
//! effect handler invocations, enabling generative simulations where Quint
//! specifications drive real state transitions.
//!
//! # Architecture
//!
//! The action registry sits between Quint's abstract action model and Aura's
//! concrete effect system:
//!
//! ```text
//! Quint Action (JSON) → ActionRegistry → ActionHandler → Aura Effects
//!                           ↓
//!                    ActionResult ← Effect Execution
//! ```
//!
//! # Usage
//!
//! ```ignore
//! let mut registry = ActionRegistry::new();
//! registry.register(InitContextHandler);
//! registry.register(TransportOpHandler);
//!
//! let result = registry.execute("initContext", &params, &nondet_picks).await?;
//! ```

use async_trait::async_trait;
use aura_core::effects::{ActionDescriptor, ActionEffect, ActionResult};
use aura_core::Result;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Trait for handlers that execute Quint actions against Aura state
///
/// Each implementation maps a specific Quint action to the corresponding
/// Aura effect invocations.
#[async_trait]
pub trait ActionHandler: Send + Sync {
    /// The Quint action name this handler responds to
    fn action_name(&self) -> &str;

    /// JSON Schema describing the expected parameters
    fn parameter_schema(&self) -> Value;

    /// Human-readable description of what this action does
    fn description(&self) -> &str;

    /// Check if this action is enabled given the current state
    ///
    /// This corresponds to Quint action guards (preconditions).
    fn is_enabled(&self, state: &Value) -> bool;

    /// Execute the action against real Aura state
    ///
    /// # Arguments
    /// * `params` - Action parameters as JSON (matches parameter_schema)
    /// * `nondet_picks` - Non-deterministic choices from ITF trace
    /// * `state` - Current state for context
    ///
    /// # Returns
    /// `ActionResult` with success status, new state, and effects produced
    async fn execute(
        &self,
        params: &Value,
        nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult>;
}

/// Registry mapping Quint action names to their handlers
///
/// The registry provides:
/// - Action lookup by name
/// - Available action enumeration for state space exploration
/// - Schema validation support
#[derive(Clone)]
pub struct ActionRegistry {
    handlers: HashMap<String, Arc<dyn ActionHandler>>,
}

impl ActionRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register an action handler
    ///
    /// If a handler for the same action name already exists, it will be replaced.
    pub fn register<H: ActionHandler + 'static>(&mut self, handler: H) {
        let name = handler.action_name().to_string();
        self.handlers.insert(name, Arc::new(handler));
    }

    /// Register a boxed action handler
    pub fn register_boxed(&mut self, handler: Arc<dyn ActionHandler>) {
        let name = handler.action_name().to_string();
        self.handlers.insert(name, handler);
    }

    /// Get a handler by action name
    pub fn get(&self, action_name: &str) -> Option<Arc<dyn ActionHandler>> {
        self.handlers.get(action_name).cloned()
    }

    /// Check if an action is registered
    pub fn has_action(&self, action_name: &str) -> bool {
        self.handlers.contains_key(action_name)
    }

    /// Get all registered action names
    pub fn action_names(&self) -> Vec<&str> {
        self.handlers.keys().map(|s| s.as_str()).collect()
    }

    /// Get descriptors for all available actions in current state
    pub fn available_actions(&self, state: &Value) -> Vec<ActionDescriptor> {
        self.handlers
            .values()
            .map(|handler| ActionDescriptor {
                name: handler.action_name().to_string(),
                parameter_schema: handler.parameter_schema(),
                description: handler.description().to_string(),
                enabled: handler.is_enabled(state),
            })
            .collect()
    }

    /// Get descriptors for only enabled actions in current state
    pub fn enabled_actions(&self, state: &Value) -> Vec<ActionDescriptor> {
        self.available_actions(state)
            .into_iter()
            .filter(|d| d.enabled)
            .collect()
    }

    /// Execute an action by name
    ///
    /// Returns an error if the action is not registered.
    pub async fn execute(
        &self,
        action_name: &str,
        params: &Value,
        nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult> {
        let handler = self.handlers.get(action_name).ok_or_else(|| {
            aura_core::AuraError::invalid(format!("unknown action: {}", action_name))
        })?;

        handler.execute(params, nondet_picks, state).await
    }

    /// Number of registered handlers
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Check if the registry is empty
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ActionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionRegistry")
            .field("actions", &self.action_names())
            .finish()
    }
}

// =============================================================================
// Built-in Action Handlers
// =============================================================================

/// No-op action handler for testing
///
/// Always succeeds without modifying state. Useful for testing registry
/// infrastructure.
pub struct NoOpHandler {
    name: String,
    description: String,
}

impl NoOpHandler {
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        Self {
            description: format!("No-op handler for '{}'", &name),
            name,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }
}

#[async_trait]
impl ActionHandler for NoOpHandler {
    fn action_name(&self) -> &str {
        &self.name
    }

    fn parameter_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": true
        })
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn is_enabled(&self, _state: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        _params: &Value,
        _nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult> {
        Ok(ActionResult {
            success: true,
            resulting_state: state.clone(),
            effects_produced: vec![ActionEffect {
                effect_type: "noop".to_string(),
                parameters: serde_json::json!({"action": self.name}),
            }],
            error: None,
        })
    }
}

/// Log action handler for debugging
///
/// Logs action execution without modifying state.
pub struct LogHandler {
    name: String,
}

impl LogHandler {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

#[async_trait]
impl ActionHandler for LogHandler {
    fn action_name(&self) -> &str {
        &self.name
    }

    fn parameter_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "message": {"type": "string"}
            },
            "additionalProperties": true
        })
    }

    fn description(&self) -> &str {
        "Log a message during simulation"
    }

    fn is_enabled(&self, _state: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        params: &Value,
        _nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult> {
        let message = params
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("(no message)");

        tracing::info!(action = %self.name, message = %message, "Quint action executed");

        Ok(ActionResult {
            success: true,
            resulting_state: state.clone(),
            effects_produced: vec![ActionEffect {
                effect_type: "log".to_string(),
                parameters: params.clone(),
            }],
            error: None,
        })
    }
}

/// Action handler that fails with a specified error
///
/// Useful for testing error handling paths.
pub struct FailHandler {
    name: String,
    error_message: String,
}

impl FailHandler {
    pub fn new(name: impl Into<String>, error_message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            error_message: error_message.into(),
        }
    }
}

#[async_trait]
impl ActionHandler for FailHandler {
    fn action_name(&self) -> &str {
        &self.name
    }

    fn parameter_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    fn description(&self) -> &str {
        "Action that always fails (for testing)"
    }

    fn is_enabled(&self, _state: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        _params: &Value,
        _nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult> {
        Ok(ActionResult {
            success: false,
            resulting_state: state.clone(),
            effects_produced: vec![],
            error: Some(self.error_message.clone()),
        })
    }
}

// =============================================================================
// Action Builder Pattern
// =============================================================================

/// Builder for creating custom action handlers with closures
///
/// Enables quick handler creation without defining a struct:
///
/// ```ignore
/// let handler = ActionBuilder::new("myAction")
///     .description("Does something useful")
///     .parameter_schema(json!({"type": "object"}))
///     .execute_fn(|params, _, state| async move {
///         // Implementation
///     })
///     .build();
/// ```
pub struct ActionBuilder<F>
where
    F: Fn(&Value, &HashMap<String, Value>, &Value) -> ActionResultFuture + Send + Sync + 'static,
{
    name: String,
    description: String,
    parameter_schema: Value,
    is_enabled_fn: Option<EnabledFn>,
    execute_fn: Option<F>,
}

/// Future type for action results
pub type ActionResultFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<ActionResult>> + Send>>;
type EnabledFn = Box<dyn Fn(&Value) -> bool + Send + Sync>;

impl<F> ActionBuilder<F>
where
    F: Fn(&Value, &HashMap<String, Value>, &Value) -> ActionResultFuture + Send + Sync + 'static,
{
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            parameter_schema: serde_json::json!({"type": "object"}),
            is_enabled_fn: None,
            execute_fn: None,
        }
    }

    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn parameter_schema(mut self, schema: Value) -> Self {
        self.parameter_schema = schema;
        self
    }

    pub fn is_enabled<G>(mut self, f: G) -> Self
    where
        G: Fn(&Value) -> bool + Send + Sync + 'static,
    {
        self.is_enabled_fn = Some(Box::new(f));
        self
    }

    pub fn execute_fn(mut self, f: F) -> Self {
        self.execute_fn = Some(f);
        self
    }

    pub fn build(self) -> ClosureActionHandler<F> {
        ClosureActionHandler {
            name: self.name,
            description: self.description,
            parameter_schema: self.parameter_schema,
            is_enabled_fn: self.is_enabled_fn,
            execute_fn: self.execute_fn.expect("execute_fn is required"),
        }
    }
}

/// Action handler implemented via closures
pub struct ClosureActionHandler<F>
where
    F: Fn(&Value, &HashMap<String, Value>, &Value) -> ActionResultFuture + Send + Sync,
{
    name: String,
    description: String,
    parameter_schema: Value,
    is_enabled_fn: Option<EnabledFn>,
    execute_fn: F,
}

#[async_trait]
impl<F> ActionHandler for ClosureActionHandler<F>
where
    F: Fn(&Value, &HashMap<String, Value>, &Value) -> ActionResultFuture + Send + Sync,
{
    fn action_name(&self) -> &str {
        &self.name
    }

    fn parameter_schema(&self) -> Value {
        self.parameter_schema.clone()
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn is_enabled(&self, state: &Value) -> bool {
        self.is_enabled_fn
            .as_ref()
            .map(|f| f(state))
            .unwrap_or(true)
    }

    async fn execute(
        &self,
        params: &Value,
        nondet_picks: &HashMap<String, Value>,
        state: &Value,
    ) -> Result<ActionResult> {
        (self.execute_fn)(params, nondet_picks, state).await
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_registration() {
        let mut registry = ActionRegistry::new();
        assert!(registry.is_empty());

        registry.register(NoOpHandler::new("test_action"));
        assert_eq!(registry.len(), 1);
        assert!(registry.has_action("test_action"));
        assert!(!registry.has_action("nonexistent"));
    }

    #[test]
    fn test_registry_action_names() {
        let mut registry = ActionRegistry::new();
        registry.register(NoOpHandler::new("action1"));
        registry.register(NoOpHandler::new("action2"));
        registry.register(LogHandler::new("action3"));

        let names = registry.action_names();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"action1"));
        assert!(names.contains(&"action2"));
        assert!(names.contains(&"action3"));
    }

    #[test]
    fn test_available_actions() {
        let mut registry = ActionRegistry::new();
        registry.register(NoOpHandler::new("action1"));
        registry.register(NoOpHandler::new("action2"));

        let state = serde_json::json!({});
        let available = registry.available_actions(&state);

        assert_eq!(available.len(), 2);
        for desc in &available {
            assert!(desc.enabled);
        }
    }

    #[tokio::test]
    async fn test_noop_handler_execution() {
        let handler = NoOpHandler::new("test");
        let params = serde_json::json!({});
        let nondet = HashMap::new();
        let state = serde_json::json!({"counter": 0});

        let result = handler.execute(&params, &nondet, &state).await.unwrap();

        assert!(result.success);
        assert_eq!(result.resulting_state, state);
        assert_eq!(result.effects_produced.len(), 1);
        assert_eq!(result.effects_produced[0].effect_type, "noop");
    }

    #[tokio::test]
    async fn test_fail_handler_execution() {
        let handler = FailHandler::new("failing_action", "intentional failure");
        let params = serde_json::json!({});
        let nondet = HashMap::new();
        let state = serde_json::json!({});

        let result = handler.execute(&params, &nondet, &state).await.unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert_eq!(result.error.unwrap(), "intentional failure");
    }

    #[tokio::test]
    async fn test_registry_execute() {
        let mut registry = ActionRegistry::new();
        registry.register(NoOpHandler::new("test_action"));

        let params = serde_json::json!({});
        let nondet = HashMap::new();
        let state = serde_json::json!({});

        let result = registry
            .execute("test_action", &params, &nondet, &state)
            .await
            .unwrap();

        assert!(result.success);
    }

    #[tokio::test]
    async fn test_registry_execute_unknown_action() {
        let registry = ActionRegistry::new();
        let params = serde_json::json!({});
        let nondet = HashMap::new();
        let state = serde_json::json!({});

        let result = registry
            .execute("nonexistent", &params, &nondet, &state)
            .await;

        assert!(result.is_err());
    }

    #[test]
    fn test_registry_debug() {
        let mut registry = ActionRegistry::new();
        registry.register(NoOpHandler::new("action1"));
        registry.register(NoOpHandler::new("action2"));

        let debug_str = format!("{:?}", registry);
        assert!(debug_str.contains("ActionRegistry"));
        assert!(debug_str.contains("action1") || debug_str.contains("action2"));
    }
}
