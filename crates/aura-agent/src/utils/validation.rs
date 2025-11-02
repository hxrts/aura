//! Input validation utilities
//!
//! Centralized validation logic for ensuring input parameters meet security
//! and format requirements. Provides a builder pattern for composable validation.

use crate::error::{AuraError, Result};

/// Builder for input validation with composable checks
pub struct Validator<'a> {
    value: &'a str,
    field_name: &'a str,
}

impl<'a> Validator<'a> {
    /// Create a new validator for the given value and field name
    pub fn new(value: &'a str, field_name: &'a str) -> Self {
        Self { value, field_name }
    }

    /// Check that the value is not empty
    pub fn not_empty(self) -> Result<Self> {
        if self.value.is_empty() {
            return Err(AuraError::agent_invalid_state(format!(
                "{} cannot be empty",
                self.field_name
            )));
        }
        Ok(self)
    }

    /// Check that the value length is within the specified range
    pub fn length_range(self, min: usize, max: usize) -> Result<Self> {
        let len = self.value.len();
        if len < min {
            return Err(AuraError::agent_invalid_state(format!(
                "{} too short (min {} characters)",
                self.field_name, min
            )));
        }
        if len > max {
            return Err(AuraError::agent_invalid_state(format!(
                "{} too long (max {} characters)",
                self.field_name, max
            )));
        }
        Ok(self)
    }

    /// Check that the value length does not exceed the maximum
    pub fn max_length(self, max: usize) -> Result<Self> {
        if self.value.len() > max {
            return Err(AuraError::agent_invalid_state(format!(
                "{} too long (max {} characters)",
                self.field_name, max
            )));
        }
        Ok(self)
    }

    /// Check that the value contains only alphanumeric characters and allowed symbols
    pub fn alphanumeric_with(self, allowed_symbols: &str) -> Result<Self> {
        if !self
            .value
            .chars()
            .all(|c| c.is_alphanumeric() || allowed_symbols.contains(c))
        {
            return Err(AuraError::agent_invalid_state(format!(
                "{} contains invalid characters (only alphanumeric and {} allowed)",
                self.field_name, allowed_symbols
            )));
        }
        Ok(self)
    }

    /// Check that the value contains only alphanumeric characters
    pub fn alphanumeric_only(self) -> Result<Self> {
        if !self.value.chars().all(|c| c.is_alphanumeric()) {
            return Err(AuraError::agent_invalid_state(format!(
                "{} contains invalid characters (only alphanumeric allowed)",
                self.field_name
            )));
        }
        Ok(self)
    }

    /// Apply a custom validation function
    pub fn custom<F>(self, validator: F, error_message: &str) -> Result<Self>
    where
        F: Fn(&str) -> bool,
    {
        if !validator(self.value) {
            return Err(AuraError::agent_invalid_state(format!(
                "{} {}",
                self.field_name, error_message
            )));
        }
        Ok(self)
    }

    /// Complete validation and return success
    pub fn validate(self) -> Result<()> {
        Ok(())
    }
}

/// Validate app ID according to Aura requirements
pub fn validate_app_id(app_id: &str) -> Result<()> {
    Validator::new(app_id, "App ID")
        .not_empty()?
        .max_length(64)?
        .alphanumeric_with("-_.")?
        .validate()
}

/// Validate context according to Aura requirements
pub fn validate_context(context: &str) -> Result<()> {
    Validator::new(context, "Context")
        .not_empty()?
        .max_length(128)?
        .alphanumeric_with("-_.:")?
        .validate()
}

/// Validate capability string according to Aura requirements
pub fn validate_capability(capability: &str) -> Result<()> {
    Validator::new(capability, "Capability")
        .not_empty()?
        .max_length(128)?
        .alphanumeric_with("-_.:")?
        .validate()
}

/// Validate a list of capabilities
pub fn validate_capabilities(capabilities: &[String]) -> Result<()> {
    for capability in capabilities {
        validate_capability(capability)?;
    }
    Ok(())
}

/// Validate input parameters for Aura operations
pub fn validate_input_parameters(
    app_id: &str,
    context: &str,
    capabilities: &[String],
) -> Result<()> {
    validate_app_id(app_id)?;
    validate_context(context)?;
    validate_capabilities(capabilities)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validator_not_empty() {
        assert!(Validator::new("test", "field").not_empty().is_ok());
        assert!(Validator::new("", "field").not_empty().is_err());
    }

    #[test]
    fn test_validator_max_length() {
        assert!(Validator::new("test", "field").max_length(10).is_ok());
        assert!(Validator::new("test", "field").max_length(3).is_err());
    }

    #[test]
    fn test_validator_alphanumeric_with() {
        assert!(Validator::new("test-123", "field")
            .alphanumeric_with("-_.")
            .is_ok());
        assert!(Validator::new("test@123", "field")
            .alphanumeric_with("-_.")
            .is_err());
    }

    #[test]
    fn test_validate_app_id() {
        assert!(validate_app_id("my-app").is_ok());
        assert!(validate_app_id("my_app.v1").is_ok());
        assert!(validate_app_id("").is_err());
        assert!(validate_app_id("my@app").is_err());
        assert!(validate_app_id(&"a".repeat(65)).is_err());
    }

    #[test]
    fn test_validate_context() {
        assert!(validate_context("user:123").is_ok());
        assert!(validate_context("session-abc_def").is_ok());
        assert!(validate_context("").is_err());
        assert!(validate_context("user@123").is_err());
        assert!(validate_context(&"a".repeat(129)).is_err());
    }

    #[test]
    fn test_validate_capability() {
        assert!(validate_capability("storage:read").is_ok());
        assert!(validate_capability("identity-derive").is_ok());
        assert!(validate_capability("").is_err());
        assert!(validate_capability("storage@read").is_err());
        assert!(validate_capability(&"a".repeat(129)).is_err());
    }

    #[test]
    fn test_validate_input_parameters() {
        assert!(
            validate_input_parameters("my-app", "user:123", &["storage:read".to_string()]).is_ok()
        );

        assert!(validate_input_parameters("", "user:123", &[]).is_err());
        assert!(validate_input_parameters("my-app", "", &[]).is_err());
        assert!(validate_input_parameters("my-app", "user:123", &["".to_string()]).is_err());
    }

    #[test]
    fn test_validator_chain() {
        let result = Validator::new("test-123", "field")
            .not_empty()
            .and_then(|v| v.max_length(10))
            .and_then(|v| v.alphanumeric_with("-_"))
            .and_then(|v| v.validate());

        assert!(result.is_ok());
    }
}
