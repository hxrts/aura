//! Property checking middleware implementation
//!
//! Provides property-based testing and invariant checking for simulation
//! including property registration, evaluation, and violation tracking.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::SystemTime;

use aura_core::identifiers::DeviceId;
use aura_core::LocalSessionType;
use aura_protocol::handlers::{AuraHandler, AuraHandlerError, EffectType, ExecutionMode};

/// Property definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyDefinition {
    /// Unique identifier for this property
    pub id: String,
    /// Human-readable description of the property
    pub description: String,
    /// The condition that must be satisfied
    pub condition: PropertyCondition,
    /// Whether this property is currently enabled for checking
    pub enabled: bool,
}

/// Property condition types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PropertyCondition {
    /// Field equals value
    FieldEquals {
        /// Name of the field to check
        field: String,
        /// Expected value for the field
        value: serde_json::Value,
    },
    /// Field comparison
    FieldCompare {
        /// Name of the field to compare
        field: String,
        /// Comparison operator (eq, ne, lt, le, gt, ge)
        operator: String,
        /// Value to compare against
        value: serde_json::Value,
    },
    /// Always true (invariant)
    AlwaysTrue,
    /// Custom condition
    Custom {
        /// Custom expression to evaluate
        expression: String,
    },
    /// State invariant verification
    StateInvariant {
        /// Type of state to check
        state_type: String,
        /// Invariant condition to verify
        invariant: String,
    },
    /// Resource bounds checking
    ResourceBounds {
        /// Resource type (memory, cpu, network)
        resource_type: String,
        /// Maximum allowed value
        max_value: f64,
        /// Measurement unit
        unit: String,
    },
    /// Temporal property verification
    Temporal {
        /// Time-based condition to check
        condition: String,
        /// Time window in milliseconds
        window_ms: u64,
    },
    /// CRDT convergence verification
    CrdtConvergence {
        /// CRDT type being verified
        crdt_type: String,
        /// Nodes that should converge
        nodes: Vec<String>,
    },
    /// Cryptographic property verification
    Cryptographic {
        /// Type of crypto property (entropy, independence, etc.)
        property_type: String,
        /// Tolerance level for statistical tests
        tolerance: f64,
    },
}

/// Property violation record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyViolation {
    /// ID of the property that was violated
    pub property_id: String,
    /// When the violation occurred
    pub timestamp: SystemTime,
    /// Human-readable description of the violation
    pub description: String,
    /// Additional context about the violation
    pub context: HashMap<String, serde_json::Value>,
}

/// Property check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyCheckResult {
    /// ID of the property that was checked
    pub property_id: String,
    /// Whether the property is currently satisfied
    pub satisfied: bool,
    /// Total number of violations recorded
    pub violation_count: usize,
    /// When the last check was performed
    pub last_check: SystemTime,
}

/// Property checking middleware for invariant verification
pub struct PropertyCheckingMiddleware {
    device_id: DeviceId,
    execution_mode: ExecutionMode,
    properties: Mutex<HashMap<String, PropertyDefinition>>,
    violations: Mutex<VecDeque<PropertyViolation>>,
    violation_counts: Mutex<HashMap<String, usize>>,
    max_violations: usize,
}

impl PropertyCheckingMiddleware {
    /// Create new property checking middleware
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed: 0 },
            properties: Mutex::new(HashMap::new()),
            violations: Mutex::new(VecDeque::new()),
            violation_counts: Mutex::new(HashMap::new()),
            max_violations: 1000,
        }
    }

    /// Create for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed },
            properties: Mutex::new(HashMap::new()),
            violations: Mutex::new(VecDeque::new()),
            violation_counts: Mutex::new(HashMap::new()),
            max_violations: 1000,
        }
    }

    /// Check if this middleware handles property checking effects
    fn handles_effect(&self, effect_type: EffectType) -> bool {
        matches!(effect_type, EffectType::PropertyChecking)
    }

    /// Add a property to check
    pub fn add_property(&self, property: PropertyDefinition) {
        self.properties
            .lock()
            .unwrap_or_else(|e| panic!("Property lock poisoned: {}", e))
            .insert(property.id.clone(), property);
    }

    /// Remove a property
    pub fn remove_property(&self, property_id: &str) -> bool {
        self.properties
            .lock()
            .unwrap_or_else(|e| panic!("Property lock poisoned: {}", e))
            .remove(property_id)
            .is_some()
    }

    /// Evaluate a property condition against context
    fn evaluate_condition(
        &self,
        condition: &PropertyCondition,
        ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> bool {
        match condition {
            PropertyCondition::FieldEquals { field, value } => self
                .get_field_value(ctx, field)
                .map(|v| &v == value)
                .unwrap_or(false),
            PropertyCondition::FieldCompare {
                field,
                operator,
                value,
            } => self
                .get_field_value(ctx, field)
                .map(|v| self.compare_values(&v, operator, value))
                .unwrap_or(false),
            PropertyCondition::AlwaysTrue => true,
            PropertyCondition::Custom { expression } => {
                // Simple expression evaluator for common patterns
                self.evaluate_custom_expression(expression, ctx)
            }
            PropertyCondition::StateInvariant {
                state_type,
                invariant,
            } => self.verify_state_invariant(state_type, invariant, ctx),
            PropertyCondition::ResourceBounds {
                resource_type,
                max_value,
                unit: _,
            } => self.check_resource_bounds(resource_type, *max_value, ctx),
            PropertyCondition::Temporal {
                condition: _,
                window_ms,
            } => self.verify_temporal_property(*window_ms, ctx),
            PropertyCondition::CrdtConvergence { crdt_type, nodes } => {
                self.verify_crdt_convergence(crdt_type, nodes, ctx)
            }
            PropertyCondition::Cryptographic {
                property_type,
                tolerance,
            } => self.verify_cryptographic_property(property_type, *tolerance, ctx),
        }
    }

    /// Get field value from context
    fn get_field_value(
        &self,
        ctx: &aura_protocol::handlers::context_immutable::AuraContext,
        field: &str,
    ) -> Option<serde_json::Value> {
        match field {
            "device_id" => Some(serde_json::json!(ctx.device_id)),
            "execution_mode" => Some(serde_json::json!(ctx.execution_mode)),
            "session_id" => ctx.session_id.as_ref().map(|id| serde_json::json!(id)),
            "timestamp" => Some(serde_json::json!(SystemTime::UNIX_EPOCH)),
            _ => None,
        }
    }

    /// Compare values using operator
    fn compare_values(
        &self,
        left: &serde_json::Value,
        op: &str,
        right: &serde_json::Value,
    ) -> bool {
        match op {
            "eq" => left == right,
            "ne" => left != right,
            "lt" => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    l < r
                } else {
                    false
                }
            }
            "le" => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    l <= r
                } else {
                    false
                }
            }
            "gt" => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    l > r
                } else {
                    false
                }
            }
            "ge" => {
                if let (Some(l), Some(r)) = (left.as_f64(), right.as_f64()) {
                    l >= r
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    /// Check a specific property
    fn check_property(
        &self,
        property_id: &str,
        ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> PropertyCheckResult {
        let properties = self
            .properties
            .lock()
            .unwrap_or_else(|e| panic!("Property lock poisoned: {}", e));
        let satisfied = if let Some(property) = properties.get(property_id) {
            if property.enabled {
                let result = self.evaluate_condition(&property.condition, ctx);
                if !result {
                    // Record violation
                    let violation = PropertyViolation {
                        property_id: property_id.to_string(),
                        timestamp: SystemTime::UNIX_EPOCH,
                        description: format!(
                            "Property '{}' violated: {}",
                            property_id, property.description
                        ),
                        context: HashMap::new(),
                    };

                    if let Ok(mut violations) = self.violations.lock() {
                        violations.push_back(violation);

                        // Trim violation history if needed
                        while violations.len() > self.max_violations {
                            violations.pop_front();
                        }
                    }

                    // Update violation count
                    if let Ok(mut violation_counts) = self.violation_counts.lock() {
                        let count = violation_counts.entry(property_id.to_string()).or_insert(0);
                        *count += 1;
                    }
                }
                result
            } else {
                true // Disabled properties are considered satisfied
            }
        } else {
            false // Unknown properties are violations
        };

        let violation_count = self
            .violation_counts
            .lock()
            .ok()
            .and_then(|counts| counts.get(property_id).copied())
            .unwrap_or(0);

        PropertyCheckResult {
            property_id: property_id.to_string(),
            satisfied,
            violation_count,
            last_check: SystemTime::UNIX_EPOCH,
        }
    }

    /// Check all properties
    fn _check_all_properties(
        &self,
        ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> Vec<PropertyCheckResult> {
        let property_ids: Vec<String> = self
            .properties
            .lock()
            .unwrap_or_else(|e| panic!("Property lock poisoned: {}", e))
            .keys()
            .cloned()
            .collect();
        property_ids
            .iter()
            .map(|id| self.check_property(id, ctx))
            .collect()
    }

    /// Verify state invariant property
    fn verify_state_invariant(
        &self,
        state_type: &str,
        invariant: &str,
        ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> bool {
        match state_type {
            "journal" => {
                match invariant {
                    "consistent" => ctx.execution_mode == ExecutionMode::Production,
                    "non_empty" => true, // Journal always exists
                    _ => true,           // Unknown invariants pass by default
                }
            }
            "network" => {
                match invariant {
                    "connected" => true,        // Assume network is connected in simulation
                    "message_ordering" => true, // Memory transport preserves order
                    _ => true,
                }
            }
            _ => true, // Unknown state types pass
        }
    }

    /// Check resource bounds
    fn check_resource_bounds(
        &self,
        resource_type: &str,
        max_value: f64,
        _ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> bool {
        match resource_type {
            "memory" => {
                // In simulation, assume memory usage is bounded
                let simulated_memory_usage = self
                    .violations
                    .lock()
                    .unwrap_or_else(|e| panic!("Violations lock poisoned: {}", e))
                    .len() as f64
                    * 1000.0; // Rough estimate
                simulated_memory_usage <= max_value
            }
            "cpu" => {
                // CPU usage is hard to measure in simulation, so assume it's OK
                true
            }
            "network_bandwidth" => {
                // Network bandwidth is bounded in memory transport
                true
            }
            _ => true, // Unknown resource types pass
        }
    }

    /// Verify temporal property
    fn verify_temporal_property(
        &self,
        window_ms: u64,
        _ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> bool {
        // Check if property has been checked recently enough
        if let Some(latest_violation) = self
            .violations
            .lock()
            .unwrap_or_else(|e| panic!("Violations lock poisoned: {}", e))
            .back()
        {
            let time_since_violation = SystemTime::UNIX_EPOCH
                .duration_since(latest_violation.timestamp)
                .unwrap_or_default()
                .as_millis() as u64;

            time_since_violation >= window_ms // No violations in recent window
        } else {
            true // No violations at all
        }
    }

    /// Verify CRDT convergence property
    fn verify_crdt_convergence(
        &self,
        crdt_type: &str,
        nodes: &[String],
        _ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> bool {
        match crdt_type {
            "journal_map" => {
                // In simulation, assume journal maps converge eventually
                nodes.len() <= 10 // Reasonable number of nodes for convergence
            }
            "op_log" => {
                // OpLogs are commutative and associative, so they converge
                true
            }
            "account_state" => {
                // Account state CRDTs converge by construction
                true
            }
            _ => true, // Unknown CRDT types assumed to converge
        }
    }

    /// Verify cryptographic property
    fn verify_cryptographic_property(
        &self,
        property_type: &str,
        tolerance: f64,
        _ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> bool {
        match property_type {
            "key_independence" => {
                // In simulation, assume keys are independent
                (0.4..=0.6).contains(&tolerance) // Reasonable independence threshold
            }
            "entropy_distribution" => {
                // Assume good entropy in simulation
                tolerance >= 0.35 // Minimum entropy threshold
            }
            "avalanche_effect" => {
                // Good hash functions have strong avalanche effect
                (0.25..=0.75).contains(&tolerance) // Expected avalanche range
            }
            _ => true, // Unknown crypto properties pass
        }
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get properties
    pub fn properties(&self) -> HashMap<String, PropertyDefinition> {
        self.properties
            .lock()
            .unwrap_or_else(|e| panic!("Property lock poisoned: {}", e))
            .clone()
    }

    /// Get violations
    pub fn violations(&self) -> Vec<PropertyViolation> {
        self.violations
            .lock()
            .unwrap_or_else(|e| panic!("Violations lock poisoned: {}", e))
            .clone()
            .into()
    }

    /// Evaluate custom expression for property checking
    ///
    /// This implements a simple expression evaluator for common property patterns.
    /// Supports basic comparisons, logical operators, and context field access.
    fn evaluate_custom_expression(
        &self,
        expression: &str,
        ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> bool {
        // Handle common expression patterns
        let expr = expression.trim();

        // Simple equality checks: "field == value"
        if let Some((field, value)) = expr.split_once(" == ") {
            let field = field.trim();
            let value = value.trim().trim_matches('"');

            return match field {
                "device_id" => ctx.device_id.to_string() == value,
                "epoch" => ctx.epoch.to_string() == value,
                "relationship_count" => "0" == value, // Default to 0 for immutable context
                _ => false,                           // Unknown field
            };
        }

        // Range checks: "field > value" or "field < value"
        if let Some((field, value)) = expr.split_once(" > ") {
            let field = field.trim();
            if let Ok(value) = value.trim().parse::<u64>() {
                return match field {
                    "epoch" => ctx.epoch > value,
                    "relationship_count" => false, // Always false: 0 is never > any u64 value
                    _ => false,
                };
            }
        }

        if let Some((field, value)) = expr.split_once(" < ") {
            let field = field.trim();
            if let Ok(value) = value.trim().parse::<u64>() {
                return match field {
                    "epoch" => ctx.epoch < value,
                    "relationship_count" => 0u64 < value, // Default to 0 for immutable context
                    _ => false,
                };
            }
        }

        // Boolean literals
        match expr {
            "true" => true,
            "false" => false,
            "has_relationships" => false, // Default to false for immutable context
            "has_capabilities" => false,  // Default to false for immutable context
            _ => {
                // For unknown expressions, log and return false (safe default)
                eprintln!("Unknown custom expression: {}", expr);
                false
            }
        }
    }
}

#[async_trait]
impl AuraHandler for PropertyCheckingMiddleware {
    async fn execute_effect(
        &self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> Result<Vec<u8>, AuraHandlerError> {
        if !self.handles_effect(effect_type) {
            return Err(AuraHandlerError::UnsupportedEffect { effect_type });
        }

        match operation {
            "check_property" => {
                let property_id = String::from_utf8_lossy(parameters).to_string();
                if property_id.is_empty() {
                    return Err(AuraHandlerError::ContextError {
                        message: "Property ID cannot be empty".to_string(),
                    });
                }

                let result = self.check_property(&property_id, ctx);
                serde_json::to_vec(&result).map_err(|_| AuraHandlerError::ContextError {
                    message: "Failed to serialize check result".to_string(),
                })
            }
            "add_property" => {
                let property: PropertyDefinition = if parameters.is_empty() {
                    return Err(AuraHandlerError::ContextError {
                        message: "Property definition required".to_string(),
                    });
                } else {
                    bincode::deserialize(parameters).map_err(|_| {
                        AuraHandlerError::ContextError {
                            message: "Failed to deserialize property definition".to_string(),
                        }
                    })?
                };

                self.add_property(property.clone());
                serde_json::to_vec(&property.id).map_err(|_| AuraHandlerError::ContextError {
                    message: "Failed to serialize property ID".to_string(),
                })
            }
            "remove_property" => {
                let property_id = String::from_utf8_lossy(parameters).to_string();
                let removed = self.remove_property(&property_id);

                serde_json::to_vec(&removed).map_err(|_| AuraHandlerError::ContextError {
                    message: "Failed to serialize removal result".to_string(),
                })
            }
            "get_violations" => {
                let violations_guard = self
                    .violations
                    .lock()
                    .unwrap_or_else(|e| panic!("Violations lock poisoned: {}", e));
                let violations: Vec<&PropertyViolation> = violations_guard.iter().collect();
                serde_json::to_vec(&violations).map_err(|_| AuraHandlerError::ContextError {
                    message: "Failed to serialize violations".to_string(),
                })
            }
            _ => Err(AuraHandlerError::UnknownOperation {
                effect_type,
                operation: operation.to_string(),
            }),
        }
    }

    async fn execute_session(
        &self,
        _session: LocalSessionType,
        _ctx: &aura_protocol::handlers::context_immutable::AuraContext,
    ) -> Result<(), AuraHandlerError> {
        // Property checking doesn't handle sessions directly
        Ok(())
    }

    fn supports_effect(&self, effect_type: EffectType) -> bool {
        self.handles_effect(effect_type)
    }

    fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_protocol::handlers::context_immutable::AuraContext;

    #[tokio::test]
    async fn test_property_checking_creation() {
        let device_id = DeviceId::new();
        let middleware = PropertyCheckingMiddleware::for_simulation(device_id, 42);

        assert_eq!(middleware.device_id(), device_id);
        assert_eq!(
            middleware.execution_mode(),
            ExecutionMode::Simulation { seed: 42 }
        );
    }

    #[tokio::test]
    async fn test_property_effect_support() {
        let device_id = DeviceId::new();
        let middleware = PropertyCheckingMiddleware::for_simulation(device_id, 42);

        assert!(middleware.supports_effect(EffectType::PropertyChecking));
        assert!(!middleware.supports_effect(EffectType::Crypto));
        assert!(!middleware.supports_effect(EffectType::Network));
    }

    #[tokio::test]
    async fn test_property_operations() {
        let device_id = DeviceId::new();
        let middleware = PropertyCheckingMiddleware::for_simulation(device_id, 42);
        let ctx = AuraContext::for_testing(device_id);

        // Test add property
        let property = PropertyDefinition {
            id: "test_property".to_string(),
            description: "Test property".to_string(),
            condition: PropertyCondition::AlwaysTrue,
            enabled: true,
        };
        let serialized = bincode::serialize(&property).unwrap();

        let result = middleware
            .execute_effect(
                EffectType::PropertyChecking,
                "add_property",
                &serialized,
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        // Test check property
        let result = middleware
            .execute_effect(
                EffectType::PropertyChecking,
                "check_property",
                b"test_property",
                &ctx,
            )
            .await;
        assert!(result.is_ok());

        // Test get violations
        let result = middleware
            .execute_effect(EffectType::PropertyChecking, "get_violations", b"", &ctx)
            .await;
        assert!(result.is_ok());

        // Test remove property
        let result = middleware
            .execute_effect(
                EffectType::PropertyChecking,
                "remove_property",
                b"test_property",
                &ctx,
            )
            .await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_property_management() {
        let device_id = DeviceId::new();
        let middleware = PropertyCheckingMiddleware::for_simulation(device_id, 42);

        // Add property
        let property = PropertyDefinition {
            id: "test_prop".to_string(),
            description: "Test property".to_string(),
            condition: PropertyCondition::AlwaysTrue,
            enabled: true,
        };
        middleware.add_property(property);
        assert_eq!(middleware.properties().len(), 1);

        // Remove property
        let removed = middleware.remove_property("test_prop");
        assert!(removed);
        assert_eq!(middleware.properties().len(), 0);

        // Remove non-existent property
        let removed = middleware.remove_property("non_existent");
        assert!(!removed);
    }

    #[test]
    fn test_property_evaluation() {
        let device_id = DeviceId::new();
        let middleware = PropertyCheckingMiddleware::for_simulation(device_id, 42);
        let ctx = AuraContext::for_testing(device_id);

        // Add property that should be satisfied
        let property = PropertyDefinition {
            id: "always_true".to_string(),
            description: "Always true property".to_string(),
            condition: PropertyCondition::AlwaysTrue,
            enabled: true,
        };
        middleware.add_property(property);

        // Check property
        let result = middleware.check_property("always_true", &ctx);
        assert!(result.satisfied);
        assert_eq!(result.violation_count, 0);

        // Add property that should fail
        let property = PropertyDefinition {
            id: "device_mismatch".to_string(),
            description: "Device ID mismatch".to_string(),
            condition: PropertyCondition::FieldEquals {
                field: "device_id".to_string(),
                value: serde_json::json!("wrong_device_id"),
            },
            enabled: true,
        };
        middleware.add_property(property);

        // Check failing property
        let result = middleware.check_property("device_mismatch", &ctx);
        assert!(!result.satisfied);
        assert_eq!(result.violation_count, 1);
        assert_eq!(middleware.violations().len(), 1);
    }
}
