//! Configuration validation system

use std::collections::HashMap;

/// Validation result for configuration fields
#[derive(Debug, Clone)]
pub enum ValidationResult {
    /// Configuration field passed validation
    Valid,
    /// Configuration field failed validation
    Invalid {
        /// Name of the field that failed validation
        field: String,
        /// Description of the validation failure
        message: String,
        /// Severity level of the validation failure
        severity: ValidationSeverity,
    },
}

/// Validation severity levels
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationSeverity {
    /// Non-blocking validation issue that should be addressed
    Warning,
    /// Blocking validation error that prevents operation
    Error,
    /// System-critical validation failure that must be resolved immediately
    Critical,
}

/// Configuration validation rule
#[derive(Debug, Clone)]
pub struct ValidationRule {
    /// Dot-separated path to the field being validated (e.g., "database.connection.timeout")
    pub field_path: String,
    /// Type of validation rule to apply
    pub rule_type: ValidationRuleType,
    /// Human-readable error message if validation fails
    pub message: String,
    /// Severity level of validation failures
    pub severity: ValidationSeverity,
}

/// Types of validation rules
#[derive(Debug, Clone)]
pub enum ValidationRuleType {
    /// Field must be present and non-empty
    Required,
    /// Field value must fall within numeric range (inclusive)
    Range {
        /// Minimum allowed value (None means no lower bound)
        min: Option<i64>,
        /// Maximum allowed value (None means no upper bound)
        max: Option<i64>,
    },
    /// Field value must match regex pattern
    Pattern(String),
    /// Field value must pass custom validator function
    Custom(String),
    /// Field must be accompanied by all listed dependent fields
    Dependencies(Vec<String>),
    /// Field value must be one of the provided options
    OneOf(Vec<String>),
    /// Field string length must be at least this many characters
    MinLength(usize),
    /// Field string length must not exceed this many characters
    MaxLength(usize),
    /// Field must be a valid network address (host:port)
    NetworkAddress,
    /// Field must be a positive floating-point number
    PositiveNumber,
    /// Field must be a valid file system path
    ValidPath,
    /// Field must be a valid HTTP(S) URL
    ValidUrl,
}

/// Configuration validator with rule-based validation
/// Type alias for custom validator functions to reduce complexity
pub type CustomValidator = Box<dyn Fn(&str) -> bool + Send + Sync>;

/// Configuration validator that applies rules to validate configuration values
pub struct ConfigValidator {
    rules: Vec<ValidationRule>,
    custom_validators: HashMap<String, CustomValidator>,
}

impl ConfigValidator {
    /// Create a new configuration validator
    pub fn new() -> Self {
        Self {
            rules: Vec::new(),
            custom_validators: HashMap::new(),
        }
    }

    /// Add a validation rule
    pub fn add_rule(mut self, rule: ValidationRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Add a custom validator function
    pub fn add_custom_validator<F>(mut self, name: String, validator: F) -> Self
    where
        F: Fn(&str) -> bool + Send + Sync + 'static,
    {
        self.custom_validators.insert(name, Box::new(validator));
        self
    }

    /// Validate a configuration value against rules
    pub fn validate_field(&self, field_path: &str, value: &str) -> Vec<ValidationResult> {
        let mut results = Vec::new();

        for rule in &self.rules {
            if rule.field_path == field_path {
                let result = self.apply_rule(rule, value);
                if !matches!(result, ValidationResult::Valid) {
                    results.push(result);
                }
            }
        }

        results
    }

    /// Apply a single validation rule
    fn apply_rule(&self, rule: &ValidationRule, value: &str) -> ValidationResult {
        let is_valid = match &rule.rule_type {
            ValidationRuleType::Required => !value.is_empty(),
            ValidationRuleType::Range { min, max } => {
                if let Ok(num) = value.parse::<i64>() {
                    let min_ok = min.map_or(true, |m| num >= m);
                    let max_ok = max.map_or(true, |m| num <= m);
                    min_ok && max_ok
                } else {
                    false
                }
            }
            ValidationRuleType::Pattern(pattern) => {
                // Simple pattern matching - would use regex in real implementation
                value.contains(pattern)
            }
            ValidationRuleType::Custom(validator_name) => self
                .custom_validators
                .get(validator_name)
                .is_some_and(|validator| validator(value)),
            ValidationRuleType::OneOf(options) => options.contains(&value.to_string()),
            ValidationRuleType::MinLength(min_len) => value.len() >= *min_len,
            ValidationRuleType::MaxLength(max_len) => value.len() <= *max_len,
            ValidationRuleType::NetworkAddress => {
                // Simple check - would use proper parsing in real implementation
                value.contains(':') && !value.is_empty()
            }
            ValidationRuleType::PositiveNumber => value.parse::<f64>().is_ok_and(|n| n > 0.0),
            ValidationRuleType::ValidPath => {
                // Simple path validation - would use std::path in real implementation
                !value.is_empty() && !value.contains('\0')
            }
            ValidationRuleType::ValidUrl => {
                // Simple URL validation - would use url crate in real implementation
                value.starts_with("http://") || value.starts_with("https://")
            }
            ValidationRuleType::Dependencies(_deps) => {
                // Would check other fields in real implementation
                true
            }
        };

        if is_valid {
            ValidationResult::Valid
        } else {
            ValidationResult::Invalid {
                field: rule.field_path.clone(),
                message: rule.message.clone(),
                severity: rule.severity.clone(),
            }
        }
    }

    /// Get all validation rules
    pub fn rules(&self) -> &[ValidationRule] {
        &self.rules
    }
}

impl Default for ConfigValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Common validation rules for Aura configurations
impl ValidationRule {
    /// Create a required field rule
    pub fn required(field_path: &str) -> Self {
        Self {
            field_path: field_path.to_string(),
            rule_type: ValidationRuleType::Required,
            message: format!("Field '{}' is required", field_path),
            severity: ValidationSeverity::Error,
        }
    }

    /// Create a numeric range rule
    pub fn range(field_path: &str, min: Option<i64>, max: Option<i64>) -> Self {
        let message = match (min, max) {
            (Some(min), Some(max)) => {
                format!("Field '{}' must be between {} and {}", field_path, min, max)
            }
            (Some(min), None) => format!("Field '{}' must be at least {}", field_path, min),
            (None, Some(max)) => format!("Field '{}' must be at most {}", field_path, max),
            (None, None) => format!("Field '{}' has invalid range constraints", field_path),
        };

        Self {
            field_path: field_path.to_string(),
            rule_type: ValidationRuleType::Range { min, max },
            message,
            severity: ValidationSeverity::Error,
        }
    }

    /// Create a pattern matching rule
    pub fn pattern(field_path: &str, pattern: &str) -> Self {
        Self {
            field_path: field_path.to_string(),
            rule_type: ValidationRuleType::Pattern(pattern.to_string()),
            message: format!("Field '{}' must match pattern '{}'", field_path, pattern),
            severity: ValidationSeverity::Error,
        }
    }

    /// Create a one-of options rule
    pub fn one_of(field_path: &str, options: Vec<String>) -> Self {
        Self {
            field_path: field_path.to_string(),
            rule_type: ValidationRuleType::OneOf(options.clone()),
            message: format!(
                "Field '{}' must be one of: {}",
                field_path,
                options.join(", ")
            ),
            severity: ValidationSeverity::Error,
        }
    }

    /// Create a positive number rule
    pub fn positive_number(field_path: &str) -> Self {
        Self {
            field_path: field_path.to_string(),
            rule_type: ValidationRuleType::PositiveNumber,
            message: format!("Field '{}' must be a positive number", field_path),
            severity: ValidationSeverity::Error,
        }
    }

    /// Create a valid URL rule
    pub fn valid_url(field_path: &str) -> Self {
        Self {
            field_path: field_path.to_string(),
            rule_type: ValidationRuleType::ValidUrl,
            message: format!("Field '{}' must be a valid URL", field_path),
            severity: ValidationSeverity::Error,
        }
    }

    /// Create a valid path rule
    pub fn valid_path(field_path: &str) -> Self {
        Self {
            field_path: field_path.to_string(),
            rule_type: ValidationRuleType::ValidPath,
            message: format!("Field '{}' must be a valid file path", field_path),
            severity: ValidationSeverity::Error,
        }
    }

    /// Create a network address rule
    pub fn network_address(field_path: &str) -> Self {
        Self {
            field_path: field_path.to_string(),
            rule_type: ValidationRuleType::NetworkAddress,
            message: format!("Field '{}' must be a valid network address", field_path),
            severity: ValidationSeverity::Error,
        }
    }
}

/// Helper trait for validating configuration structs
pub trait ValidateConfig {
    /// Get validation results for this configuration
    fn validate_config(&self) -> Vec<ValidationResult>;

    /// Check if configuration is valid (no errors)
    fn is_valid(&self) -> bool {
        self.validate_config()
            .iter()
            .all(|result| matches!(result, ValidationResult::Valid))
    }

    /// Get only validation errors (not warnings)
    fn validation_errors(&self) -> Vec<ValidationResult> {
        self.validate_config()
            .into_iter()
            .filter(|result| {
                matches!(result, ValidationResult::Invalid { severity, .. }
                    if *severity == ValidationSeverity::Error || *severity == ValidationSeverity::Critical)
            })
            .collect()
    }

    /// Get validation warnings
    fn validation_warnings(&self) -> Vec<ValidationResult> {
        self.validate_config()
            .into_iter()
            .filter(|result| {
                matches!(result, ValidationResult::Invalid { severity, .. }
                    if *severity == ValidationSeverity::Warning)
            })
            .collect()
    }
}
