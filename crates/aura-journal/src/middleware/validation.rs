//! Input validation middleware for journal operations

use super::{JournalContext, JournalHandler, JournalMiddleware};
use crate::error::{Error, Result};
use crate::operations::JournalOperation;
use crate::types::{DeviceMetadata, GuardianMetadata};
use aura_types::{AccountId, DeviceId};

/// Validation middleware that validates operation inputs
pub struct ValidationMiddleware {
    /// Configuration
    config: ValidationConfig,
}

impl ValidationMiddleware {
    /// Create new validation middleware
    pub fn new(config: ValidationConfig) -> Self {
        Self { config }
    }
}

impl JournalMiddleware for ValidationMiddleware {
    fn process(
        &self,
        operation: JournalOperation,
        context: &JournalContext,
        next: &dyn JournalHandler,
    ) -> Result<serde_json::Value> {
        // Skip validation if disabled
        if !self.config.enable_validation {
            return next.handle(operation, context);
        }

        // Validate context
        self.validate_context(context)?;

        // Validate operation-specific inputs
        self.validate_operation(&operation)?;

        // All validations passed, proceed with operation
        next.handle(operation, context)
    }

    fn name(&self) -> &str {
        "validation"
    }
}

impl ValidationMiddleware {
    fn validate_context(&self, context: &JournalContext) -> Result<()> {
        // Validate account ID
        if self.config.validate_account_ids {
            self.validate_account_id(&context.account_id)?;
        }

        // Validate device ID
        if self.config.validate_device_ids {
            self.validate_device_id(&context.device_id)?;
        }

        // Validate operation type
        if self.config.validate_operation_types {
            self.validate_operation_type(&context.operation_type)?;
        }

        // Validate timestamp
        if self.config.validate_timestamps {
            self.validate_timestamp(context.timestamp)?;
        }

        Ok(())
    }

    fn validate_operation(&self, operation: &JournalOperation) -> Result<()> {
        match operation {
            JournalOperation::AddDevice { device } => {
                self.validate_device_metadata(device)?;
            }

            JournalOperation::RemoveDevice { device_id } => {
                if self.config.validate_device_ids {
                    self.validate_device_id(device_id)?;
                }
            }

            JournalOperation::AddGuardian { guardian } => {
                self.validate_guardian_metadata(guardian)?;
            }

            JournalOperation::IncrementEpoch => {
                // No specific validation for epoch increment
            }

            JournalOperation::GetDevices => {
                // No specific validation for read operations
            }

            JournalOperation::GetEpoch => {
                // No specific validation for read operations
            }
        }

        Ok(())
    }

    fn validate_account_id(&self, account_id: &AccountId) -> Result<()> {
        let account_str = account_id.to_string();

        if account_str.is_empty() {
            return Err(Error::invalid_operation("Account ID cannot be empty"));
        }

        if account_str.len() < self.config.min_account_id_length {
            return Err(Error::invalid_operation(format!(
                "Account ID too short: {} < {}",
                account_str.len(),
                self.config.min_account_id_length
            )));
        }

        if account_str.len() > self.config.max_account_id_length {
            return Err(Error::invalid_operation(format!(
                "Account ID too long: {} > {}",
                account_str.len(),
                self.config.max_account_id_length
            )));
        }

        Ok(())
    }

    fn validate_device_id(&self, device_id: &DeviceId) -> Result<()> {
        let device_str = device_id.to_string();

        if device_str.is_empty() {
            return Err(Error::invalid_operation("Device ID cannot be empty"));
        }

        if device_str.len() < self.config.min_device_id_length {
            return Err(Error::invalid_operation(format!(
                "Device ID too short: {} < {}",
                device_str.len(),
                self.config.min_device_id_length
            )));
        }

        if device_str.len() > self.config.max_device_id_length {
            return Err(Error::invalid_operation(format!(
                "Device ID too long: {} > {}",
                device_str.len(),
                self.config.max_device_id_length
            )));
        }

        Ok(())
    }

    fn validate_operation_type(&self, operation_type: &str) -> Result<()> {
        if operation_type.is_empty() {
            return Err(Error::invalid_operation("Operation type cannot be empty"));
        }

        if operation_type.len() > self.config.max_operation_type_length {
            return Err(Error::invalid_operation(format!(
                "Operation type too long: {} > {}",
                operation_type.len(),
                self.config.max_operation_type_length
            )));
        }

        // Check for valid characters (alphanumeric and underscores)
        if !operation_type
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_')
        {
            return Err(Error::invalid_operation(
                "Operation type contains invalid characters",
            ));
        }

        Ok(())
    }

    fn validate_timestamp(&self, timestamp: u64) -> Result<()> {
        let now = aura_types::time::current_unix_timestamp();

        // Check if timestamp is too far in the past
        if now > timestamp && (now - timestamp) > self.config.max_timestamp_age_seconds {
            return Err(Error::invalid_operation(format!(
                "Timestamp too old: {} seconds ago",
                now - timestamp
            )));
        }

        // Check if timestamp is too far in the future
        if timestamp > now && (timestamp - now) > self.config.max_timestamp_future_seconds {
            return Err(Error::invalid_operation(format!(
                "Timestamp too far in future: {} seconds ahead",
                timestamp - now
            )));
        }

        Ok(())
    }

    fn validate_device_metadata(&self, device: &DeviceMetadata) -> Result<()> {
        // Validate device ID
        if self.config.validate_device_ids {
            self.validate_device_id(&device.device_id)?;
        }

        // Validate device name
        if device.device_name.is_empty() {
            return Err(Error::invalid_operation("Device name cannot be empty"));
        }

        if device.device_name.len() > self.config.max_device_name_length {
            return Err(Error::invalid_operation(format!(
                "Device name too long: {} > {}",
                device.device_name.len(),
                self.config.max_device_name_length
            )));
        }

        // Validate timestamps
        if self.config.validate_timestamps {
            self.validate_timestamp(device.added_at)?;
            self.validate_timestamp(device.last_seen)?;
        }

        // Validate that last_seen is not before added_at
        if device.last_seen < device.added_at {
            return Err(Error::invalid_operation(
                "Device last_seen cannot be before added_at",
            ));
        }

        // Validate nonce constraints
        if device.next_nonce > self.config.max_nonce_value {
            return Err(Error::invalid_operation(format!(
                "Next nonce too large: {} > {}",
                device.next_nonce, self.config.max_nonce_value
            )));
        }

        // Validate used nonces don't exceed limits
        if device.used_nonces.len() > self.config.max_used_nonces {
            return Err(Error::invalid_operation(format!(
                "Too many used nonces: {} > {}",
                device.used_nonces.len(),
                self.config.max_used_nonces
            )));
        }

        Ok(())
    }

    fn validate_guardian_metadata(&self, guardian: &GuardianMetadata) -> Result<()> {
        // Validate guardian ID
        let guardian_str = guardian.guardian_id.to_string();
        if guardian_str.is_empty() {
            return Err(Error::invalid_operation("Guardian ID cannot be empty"));
        }

        // Validate email (contact info)
        if guardian.email.is_empty() {
            return Err(Error::invalid_operation("Guardian email cannot be empty"));
        }

        if guardian.email.len() > self.config.max_contact_info_length {
            return Err(Error::invalid_operation(format!(
                "Guardian email too long: {} > {}",
                guardian.email.len(),
                self.config.max_contact_info_length
            )));
        }

        // Validate timestamps
        if self.config.validate_timestamps {
            self.validate_timestamp(guardian.added_at)?;
        }

        Ok(())
    }
}

/// Configuration for validation middleware
#[derive(Debug, Clone)]
pub struct ValidationConfig {
    /// Whether validation is enabled
    pub enable_validation: bool,

    /// Whether to validate account IDs
    pub validate_account_ids: bool,

    /// Whether to validate device IDs
    pub validate_device_ids: bool,

    /// Whether to validate operation types
    pub validate_operation_types: bool,

    /// Whether to validate timestamps
    pub validate_timestamps: bool,

    /// Minimum account ID length
    pub min_account_id_length: usize,

    /// Maximum account ID length
    pub max_account_id_length: usize,

    /// Minimum device ID length
    pub min_device_id_length: usize,

    /// Maximum device ID length
    pub max_device_id_length: usize,

    /// Maximum operation type length
    pub max_operation_type_length: usize,

    /// Maximum device name length
    pub max_device_name_length: usize,

    /// Maximum contact info length
    pub max_contact_info_length: usize,

    /// Maximum timestamp age in seconds
    pub max_timestamp_age_seconds: u64,

    /// Maximum timestamp future in seconds
    pub max_timestamp_future_seconds: u64,

    /// Maximum nonce value
    pub max_nonce_value: u64,

    /// Maximum number of used nonces to track
    pub max_used_nonces: usize,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            enable_validation: true,
            validate_account_ids: true,
            validate_device_ids: true,
            validate_operation_types: true,
            validate_timestamps: true,
            min_account_id_length: 1,
            max_account_id_length: 256,
            min_device_id_length: 1,
            max_device_id_length: 256,
            max_operation_type_length: 64,
            max_device_name_length: 128,
            max_contact_info_length: 512,
            max_timestamp_age_seconds: 3600,   // 1 hour
            max_timestamp_future_seconds: 300, // 5 minutes
            max_nonce_value: u64::MAX / 2,     // Reasonable upper bound
            max_used_nonces: 10000,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use crate::operations::JournalOperation;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_validation_middleware_valid_operation() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = ValidationMiddleware::new(ValidationConfig::default());
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test".to_string());
        let operation = JournalOperation::GetEpoch;

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validation_middleware_invalid_context() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let mut config = ValidationConfig::default();
        config.max_operation_type_length = 2; // Very short limit

        let middleware = ValidationMiddleware::new(config);
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test_operation_type".to_string()); // Too long
        let operation = JournalOperation::GetEpoch;

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_err());
    }

    #[test]
    fn test_validation_disabled() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let mut config = ValidationConfig::default();
        config.enable_validation = false;
        config.max_operation_type_length = 2; // Very short limit

        let middleware = ValidationMiddleware::new(config);
        let handler = NoOpHandler;
        let context = JournalContext::new(account_id, device_id, "test_operation_type".to_string()); // Would be too long if validation enabled
        let operation = JournalOperation::GetEpoch;

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok()); // Should succeed because validation is disabled
    }
}
