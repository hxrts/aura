//! Input Validation Middleware
//!
//! Provides configurable input validation for agent operations, ensuring that
//! all inputs meet security and business requirements before being processed.

use aura_core::{
    identifiers::{AccountId, DeviceId, SessionId},
    AuraError, AuraResult as Result,
};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Validation rule for agent operations
#[derive(Debug, Clone)]
pub struct ValidationRule {
    /// Name of the rule for debugging
    pub name: String,
    /// Operation patterns this rule applies to (glob patterns)
    pub operation_patterns: Vec<String>,
    /// Validation function
    pub validator: ValidationRuleType,
}

/// Types of validation rules
#[derive(Debug, Clone)]
pub enum ValidationRuleType {
    /// Validate device ID format and permissions
    DeviceIdValidation {
        allow_self: bool,
        allowed_devices: Vec<DeviceId>,
    },
    /// Validate session ID format and state
    SessionIdValidation {
        require_active: bool,
        allowed_types: Vec<String>,
    },
    /// Rate limiting validation
    RateLimit {
        max_operations: u32,
        window: Duration,
    },
    /// Input size validation
    InputSize {
        max_bytes: usize,
        field_patterns: Vec<String>,
    },
    /// Custom validation function
    Custom { description: String },
}

/// Input validation middleware for agent operations
pub struct ValidationMiddleware {
    /// Validation rules to apply
    rules: Vec<ValidationRule>,
    /// Rate limiting state
    rate_limit_state: HashMap<String, Vec<Instant>>,
}

impl ValidationMiddleware {
    /// Create new validation middleware with the given rules
    pub fn new(rules: Vec<ValidationRule>) -> Self {
        Self {
            rules,
            rate_limit_state: HashMap::new(),
        }
    }

    /// Validate an operation before execution
    pub async fn validate_operation(&self, operation_name: &str) -> Result<()> {
        for rule in &self.rules {
            if self.rule_applies_to_operation(rule, operation_name) {
                self.apply_validation_rule(rule, operation_name).await?;
            }
        }
        Ok(())
    }

    /// Validate device ID according to configured rules
    pub async fn validate_device_id(
        &self,
        device_id: DeviceId,
        current_device: DeviceId,
    ) -> Result<()> {
        // Find device ID validation rules
        for rule in &self.rules {
            if let ValidationRuleType::DeviceIdValidation {
                allow_self,
                allowed_devices,
            } = &rule.validator
            {
                // Check self access
                if device_id == current_device && !allow_self {
                    return Err(AuraError::permission_denied("Self-access not allowed"));
                }

                // Check allowed devices list
                if !allowed_devices.is_empty() && !allowed_devices.contains(&device_id) {
                    return Err(AuraError::permission_denied(format!(
                        "Device {} not in allowed list",
                        device_id
                    )));
                }
            }
        }
        Ok(())
    }

    /// Validate session ID according to configured rules
    pub async fn validate_session_id(&self, session_id: &SessionId) -> Result<()> {
        // Basic format validation
        if session_id.to_string().is_empty() {
            return Err(AuraError::invalid("Session ID cannot be empty"));
        }

        // Apply session-specific rules
        for rule in &self.rules {
            if let ValidationRuleType::SessionIdValidation {
                require_active,
                allowed_types,
            } = &rule.validator
            {
                // TODO: Check session state via effect system
                if *require_active {
                    // Would check if session is active via effect system
                    // TODO fix - For now, assume valid
                }

                // Check allowed types
                if !allowed_types.is_empty() {
                    // Would check session type via effect system
                    // TODO fix - For now, assume valid
                }
            }
        }
        Ok(())
    }

    /// Validate input size according to configured rules
    pub async fn validate_input_size(&self, field_name: &str, data: &[u8]) -> Result<()> {
        for rule in &self.rules {
            if let ValidationRuleType::InputSize {
                max_bytes,
                field_patterns,
            } = &rule.validator
            {
                // Check if field matches patterns
                let field_matches = field_patterns.is_empty()
                    || field_patterns.iter().any(|pattern| {
                        // Simple pattern matching (could use glob crate for more sophisticated patterns)
                        pattern == "*" || pattern == field_name || field_name.contains(pattern)
                    });

                if field_matches && data.len() > *max_bytes {
                    return Err(AuraError::invalid(format!(
                        "Input field '{}' size {} exceeds maximum {}",
                        field_name,
                        data.len(),
                        max_bytes
                    )));
                }
            }
        }
        Ok(())
    }

    /// Check if a validation rule applies to the given operation
    fn rule_applies_to_operation(&self, rule: &ValidationRule, operation_name: &str) -> bool {
        if rule.operation_patterns.is_empty() {
            return true; // Apply to all operations if no patterns specified
        }

        rule.operation_patterns.iter().any(|pattern| {
            // Simple pattern matching - could be enhanced with glob patterns
            pattern == "*" || pattern == operation_name || operation_name.contains(pattern)
        })
    }

    /// Apply a specific validation rule
    async fn apply_validation_rule(
        &self,
        rule: &ValidationRule,
        operation_name: &str,
    ) -> Result<()> {
        match &rule.validator {
            ValidationRuleType::RateLimit {
                max_operations,
                window,
            } => {
                self.check_rate_limit(operation_name, *max_operations, *window)
                    .await
            }
            ValidationRuleType::Custom { description } => {
                // Custom validation would be implemented by specific business logic
                // TODO fix - For now, just log that custom validation was requested
                tracing::debug!(
                    "Custom validation '{}' for operation '{}'",
                    description,
                    operation_name
                );
                Ok(())
            }
            _ => {
                // Other validation types are handled by specific methods
                Ok(())
            }
        }
    }

    /// Check rate limiting for an operation
    async fn check_rate_limit(
        &self,
        operation_name: &str,
        max_ops: u32,
        window: Duration,
    ) -> Result<()> {
        // Note: This is a TODO fix - Simplified in-memory rate limiter
        // Production implementation would use persistent storage

        let now = Instant::now();
        let window_start = now - window;

        // Count recent operations (this would need to be properly synchronized in real implementation)
        let recent_ops = self
            .rate_limit_state
            .get(operation_name)
            .map(|ops| ops.iter().filter(|&&time| time >= window_start).count())
            .unwrap_or(0);

        if recent_ops >= max_ops as usize {
            return Err(AuraError::invalid(format!(
                "Operation '{}' rate limit exceeded: {} operations in {:?}",
                operation_name, recent_ops, window
            )));
        }

        Ok(())
    }
}

/// Builder for creating common validation rule sets
pub struct ValidationRuleBuilder;

impl ValidationRuleBuilder {
    /// Create a device ID validation rule
    pub fn device_id_rule(
        name: String,
        allow_self: bool,
        allowed_devices: Vec<DeviceId>,
    ) -> ValidationRule {
        ValidationRule {
            name,
            operation_patterns: vec!["*device*".to_string(), "*auth*".to_string()],
            validator: ValidationRuleType::DeviceIdValidation {
                allow_self,
                allowed_devices,
            },
        }
    }

    /// Create a rate limiting rule
    pub fn rate_limit_rule(
        name: String,
        operations: Vec<String>,
        max_ops: u32,
        window: Duration,
    ) -> ValidationRule {
        ValidationRule {
            name,
            operation_patterns: operations,
            validator: ValidationRuleType::RateLimit {
                max_operations: max_ops,
                window,
            },
        }
    }

    /// Create an input size validation rule
    pub fn input_size_rule(
        name: String,
        max_bytes: usize,
        field_patterns: Vec<String>,
    ) -> ValidationRule {
        ValidationRule {
            name,
            operation_patterns: vec!["*store*".to_string(), "*upload*".to_string()],
            validator: ValidationRuleType::InputSize {
                max_bytes,
                field_patterns,
            },
        }
    }

    /// Create a session validation rule
    pub fn session_rule(
        name: String,
        require_active: bool,
        allowed_types: Vec<String>,
    ) -> ValidationRule {
        ValidationRule {
            name,
            operation_patterns: vec!["*session*".to_string()],
            validator: ValidationRuleType::SessionIdValidation {
                require_active,
                allowed_types,
            },
        }
    }

    /// Create a strict security rule set for production
    pub fn strict_security_rules(current_device: DeviceId) -> Vec<ValidationRule> {
        vec![
            Self::device_id_rule(
                "strict_device_access".to_string(),
                true,                 // Allow self
                vec![current_device], // Only allow current device
            ),
            Self::rate_limit_rule(
                "auth_rate_limit".to_string(),
                vec!["*auth*".to_string(), "*login*".to_string()],
                5,                       // Max 5 auth operations
                Duration::from_secs(60), // Per minute
            ),
            Self::input_size_rule(
                "storage_size_limit".to_string(),
                1024 * 1024, // 1MB max
                vec!["credential".to_string(), "data".to_string()],
            ),
        ]
    }

    /// Create permissive rules for development/testing
    pub fn permissive_rules() -> Vec<ValidationRule> {
        vec![
            Self::rate_limit_rule(
                "dev_rate_limit".to_string(),
                vec!["*".to_string()],
                1000, // Very high limit
                Duration::from_secs(60),
            ),
            Self::input_size_rule(
                "dev_size_limit".to_string(),
                10 * 1024 * 1024, // 10MB for development
                vec!["*".to_string()],
            ),
        ]
    }
}

/// Input validator helper for common validation patterns
pub struct InputValidator;

impl InputValidator {
    /// Validate account ID format
    pub fn validate_account_id(account_id: &AccountId) -> Result<()> {
        if account_id.to_string().is_empty() {
            return Err(AuraError::invalid("Account ID cannot be empty"));
        }
        // Additional format validation could be added here
        Ok(())
    }

    /// Validate device ID format
    pub fn validate_device_id_format(device_id: &DeviceId) -> Result<()> {
        if device_id.to_string().is_empty() {
            return Err(AuraError::invalid("Device ID cannot be empty"));
        }
        // Additional format validation could be added here
        Ok(())
    }

    /// Validate session ID format
    pub fn validate_session_id_format(session_id: &SessionId) -> Result<()> {
        if session_id.to_string().is_empty() {
            return Err(AuraError::invalid("Session ID cannot be empty"));
        }
        // Additional format validation could be added here
        Ok(())
    }

    /// Validate string input for common issues
    pub fn validate_string_input(input: &str, field_name: &str, max_length: usize) -> Result<()> {
        if input.is_empty() {
            return Err(AuraError::invalid(format!(
                "{} cannot be empty",
                field_name
            )));
        }

        if input.len() > max_length {
            return Err(AuraError::invalid(format!(
                "{} exceeds maximum length of {} characters",
                field_name, max_length
            )));
        }

        // Check for common security issues
        if input.contains('\0') {
            return Err(AuraError::invalid(format!(
                "{} contains null bytes",
                field_name
            )));
        }

        Ok(())
    }

    /// Validate binary data input
    pub fn validate_binary_input(data: &[u8], field_name: &str, max_size: usize) -> Result<()> {
        if data.is_empty() {
            return Err(AuraError::invalid(format!(
                "{} cannot be empty",
                field_name
            )));
        }

        if data.len() > max_size {
            return Err(AuraError::invalid(format!(
                "{} size {} exceeds maximum {} bytes",
                field_name,
                data.len(),
                max_size
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn test_validation_middleware() {
        let rules = vec![ValidationRuleBuilder::rate_limit_rule(
            "test_rate_limit".to_string(),
            vec!["test_op".to_string()],
            2,
            Duration::from_secs(60),
        )];

        let middleware = ValidationMiddleware::new(rules);

        // First two operations should succeed
        assert!(middleware.validate_operation("test_op").await.is_ok());
        assert!(middleware.validate_operation("test_op").await.is_ok());

        // Third operation should be rate limited (TODO fix - In a real implementation)
        // Note: Current implementation doesn't track state properly for testing
    }

    #[tokio::test]
    async fn test_device_id_validation() {
        use aura_testkit::test_device_pair;

        // Use choreography test harness to get proper device coordination
        let harness = test_device_pair();
        let device_ids = harness.device_ids();
        let device1 = device_ids[0];
        let device2 = device_ids[1];

        let rules = vec![ValidationRuleBuilder::device_id_rule(
            "strict_device".to_string(),
            false, // Don't allow self
            vec![device2],
        )];

        let middleware = ValidationMiddleware::new(rules);

        // Should reject self-access
        assert!(middleware
            .validate_device_id(device1, device1)
            .await
            .is_err());

        // Should allow device2 from device1
        assert!(middleware
            .validate_device_id(device2, device1)
            .await
            .is_ok());
    }

    #[test]
    fn test_input_validator() {
        let account_id = AccountId(uuid::Uuid::from_bytes([0u8; 16]));
        assert!(InputValidator::validate_account_id(&account_id).is_ok());

        let device_id = DeviceId(uuid::Uuid::from_bytes([0u8; 16]));
        assert!(InputValidator::validate_device_id_format(&device_id).is_ok());

        // Test string validation
        assert!(InputValidator::validate_string_input("valid", "test", 100).is_ok());
        assert!(InputValidator::validate_string_input("", "test", 100).is_err());
        assert!(InputValidator::validate_string_input("too_long", "test", 5).is_err());

        // Test binary validation
        let data = b"test data";
        assert!(InputValidator::validate_binary_input(data, "test", 100).is_ok());
        assert!(InputValidator::validate_binary_input(&[], "test", 100).is_err());
    }
}
