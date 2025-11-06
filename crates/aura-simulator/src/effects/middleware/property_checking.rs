//! Property checking middleware implementation
//!
//! Provides property-based testing and invariant checking for simulation
//! including property registration, evaluation, and violation tracking.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::time::SystemTime;

use aura_protocol::handlers::{
    AuraContext, AuraHandler, AuraHandlerError, EffectType, ExecutionMode,
};
use aura_types::identifiers::DeviceId;
use aura_types::sessions::LocalSessionType;

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
    properties: HashMap<String, PropertyDefinition>,
    violations: VecDeque<PropertyViolation>,
    violation_counts: HashMap<String, usize>,
    max_violations: usize,
}

impl PropertyCheckingMiddleware {
    /// Create new property checking middleware
    pub fn new(device_id: DeviceId) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed: 0 },
            properties: HashMap::new(),
            violations: VecDeque::new(),
            violation_counts: HashMap::new(),
            max_violations: 1000,
        }
    }

    /// Create for simulation mode
    pub fn for_simulation(device_id: DeviceId, seed: u64) -> Self {
        Self {
            device_id,
            execution_mode: ExecutionMode::Simulation { seed },
            properties: HashMap::new(),
            violations: VecDeque::new(),
            violation_counts: HashMap::new(),
            max_violations: 1000,
        }
    }

    /// Check if this middleware handles property checking effects
    fn handles_effect(&self, effect_type: EffectType) -> bool {
        matches!(effect_type, EffectType::PropertyChecking)
    }

    /// Add a property to check
    pub fn add_property(&mut self, property: PropertyDefinition) {
        self.properties.insert(property.id.clone(), property);
    }

    /// Remove a property
    pub fn remove_property(&mut self, property_id: &str) -> bool {
        self.properties.remove(property_id).is_some()
    }

    /// Evaluate a property condition against context
    fn evaluate_condition(&self, condition: &PropertyCondition, ctx: &AuraContext) -> bool {
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
            PropertyCondition::Custom { expression: _ } => {
                // For now, custom expressions always return true
                // In a real implementation, this would parse and evaluate the expression
                true
            }
        }
    }

    /// Get field value from context
    fn get_field_value(&self, ctx: &AuraContext, field: &str) -> Option<serde_json::Value> {
        match field {
            "device_id" => Some(serde_json::json!(ctx.device_id)),
            "execution_mode" => Some(serde_json::json!(ctx.execution_mode)),
            "session_id" => ctx.session_id.as_ref().map(|id| serde_json::json!(id)),
            "timestamp" => Some(serde_json::json!(SystemTime::now())),
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
    fn check_property(&mut self, property_id: &str, ctx: &AuraContext) -> PropertyCheckResult {
        let satisfied = if let Some(property) = self.properties.get(property_id) {
            if property.enabled {
                let result = self.evaluate_condition(&property.condition, ctx);
                if !result {
                    // Record violation
                    let violation = PropertyViolation {
                        property_id: property_id.to_string(),
                        timestamp: SystemTime::now(),
                        description: format!(
                            "Property '{}' violated: {}",
                            property_id, property.description
                        ),
                        context: HashMap::new(),
                    };
                    self.violations.push_back(violation);

                    // Update violation count
                    let count = self
                        .violation_counts
                        .entry(property_id.to_string())
                        .or_insert(0);
                    *count += 1;

                    // Trim violation history if needed
                    while self.violations.len() > self.max_violations {
                        self.violations.pop_front();
                    }
                }
                result
            } else {
                true // Disabled properties are considered satisfied
            }
        } else {
            false // Unknown properties are violations
        };

        PropertyCheckResult {
            property_id: property_id.to_string(),
            satisfied,
            violation_count: self.violation_counts.get(property_id).copied().unwrap_or(0),
            last_check: SystemTime::now(),
        }
    }

    /// Check all properties
    fn check_all_properties(&mut self, ctx: &AuraContext) -> Vec<PropertyCheckResult> {
        let property_ids: Vec<String> = self.properties.keys().cloned().collect();
        property_ids
            .iter()
            .map(|id| self.check_property(id, ctx))
            .collect()
    }

    /// Get device ID
    pub fn device_id(&self) -> DeviceId {
        self.device_id
    }

    /// Get properties
    pub fn properties(&self) -> &HashMap<String, PropertyDefinition> {
        &self.properties
    }

    /// Get violations
    pub fn violations(&self) -> &VecDeque<PropertyViolation> {
        &self.violations
    }
}

#[async_trait]
impl AuraHandler for PropertyCheckingMiddleware {
    async fn execute_effect(
        &mut self,
        effect_type: EffectType,
        operation: &str,
        parameters: &[u8],
        ctx: &mut AuraContext,
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
                let violations: Vec<&PropertyViolation> = self.violations.iter().collect();
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
        &mut self,
        _session: LocalSessionType,
        _ctx: &mut AuraContext,
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
        let mut middleware = PropertyCheckingMiddleware::for_simulation(device_id, 42);
        let mut ctx = AuraContext::for_testing(device_id);

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
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());

        // Test check property
        let result = middleware
            .execute_effect(
                EffectType::PropertyChecking,
                "check_property",
                b"test_property",
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());

        // Test get violations
        let result = middleware
            .execute_effect(
                EffectType::PropertyChecking,
                "get_violations",
                b"",
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());

        // Test remove property
        let result = middleware
            .execute_effect(
                EffectType::PropertyChecking,
                "remove_property",
                b"test_property",
                &mut ctx,
            )
            .await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_property_management() {
        let device_id = DeviceId::new();
        let mut middleware = PropertyCheckingMiddleware::for_simulation(device_id, 42);

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
        let mut middleware = PropertyCheckingMiddleware::for_simulation(device_id, 42);
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
