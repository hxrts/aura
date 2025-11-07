//! Hardware security middleware for TEE/HSM integration

use super::{CryptoContext, CryptoHandler, CryptoMiddleware, SecurityLevel};
use crate::middleware::CryptoOperation;
use crate::{CryptoError, Result};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Hardware security middleware that manages TEE/HSM operations
pub struct HardwareSecurityMiddleware {
    /// Hardware capability tracker
    tracker: Arc<RwLock<HardwareTracker>>,

    /// Configuration
    config: HardwareConfig,
}

impl HardwareSecurityMiddleware {
    /// Create new hardware security middleware
    pub fn new(config: HardwareConfig) -> Self {
        Self {
            tracker: Arc::new(RwLock::new(HardwareTracker::new())),
            config,
        }
    }

    /// Get hardware security statistics
    pub fn stats(&self) -> HardwareStats {
        let tracker = self.tracker.read().unwrap();
        tracker.stats()
    }

    /// Check hardware capabilities
    pub fn check_hardware_capabilities(&self) -> Result<HardwareCapabilities> {
        let tracker = self.tracker.read().map_err(|_| {
            CryptoError::internal_error("Failed to acquire read lock on hardware tracker")
        })?;

        Ok(tracker.get_capabilities())
    }

    /// Test hardware security module availability
    pub fn test_hsm_availability(&self) -> Result<bool> {
        // Simplified HSM test - in real implementation would:
        // - Test hardware security module connectivity
        // - Validate HSM firmware and attestation
        // - Check key storage availability
        // - Verify cryptographic operations work correctly

        if !self.config.enable_hsm_operations {
            return Ok(false);
        }

        // Simulate HSM connectivity test
        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on hardware tracker")
        })?;

        tracker.hsm_tests += 1;

        // Simulate availability check based on configuration
        let available = self.config.hsm_available;
        if available {
            tracker.successful_hsm_tests += 1;
        } else {
            tracker.failed_hsm_tests += 1;
        }

        Ok(available)
    }
}

impl CryptoMiddleware for HardwareSecurityMiddleware {
    fn process(
        &self,
        operation: CryptoOperation,
        context: &CryptoContext,
        next: &dyn CryptoHandler,
    ) -> Result<serde_json::Value> {
        // Determine if operation should use hardware security
        let use_hardware = self.should_use_hardware(&operation, context)?;

        if use_hardware {
            // Check hardware availability
            self.check_hardware_requirements(&operation)?;

            // Validate attestation if required
            if self.config.require_attestation {
                self.validate_hardware_attestation(context)?;
            }

            // Record hardware operation attempt
            self.record_hardware_operation(&operation)?;

            // Process operation with hardware security metadata
            let mut result = next.handle(operation.clone(), context)?;

            // Add hardware security metadata to response
            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    "hardware_secured".to_string(),
                    serde_json::Value::Bool(true),
                );
                obj.insert(
                    "tee_enabled".to_string(),
                    serde_json::Value::Bool(self.config.enable_tee_operations),
                );
                obj.insert(
                    "hsm_enabled".to_string(),
                    serde_json::Value::Bool(self.config.enable_hsm_operations),
                );

                if self.config.include_attestation_info {
                    obj.insert(
                        "attestation".to_string(),
                        serde_json::json!({
                            "verified": true,
                            "platform": self.get_platform_info(),
                            "security_level": format!("{:?}", context.security_level)
                        }),
                    );
                }
            }

            self.record_successful_hardware_operation()?;
            Ok(result)
        } else {
            // Process without hardware security
            let mut result = next.handle(operation, context)?;

            // Mark as software-only operation
            if let Some(obj) = result.as_object_mut() {
                obj.insert(
                    "hardware_secured".to_string(),
                    serde_json::Value::Bool(false),
                );
            }

            Ok(result)
        }
    }

    fn name(&self) -> &str {
        "hardware_security"
    }
}

impl HardwareSecurityMiddleware {
    fn should_use_hardware(
        &self,
        operation: &CryptoOperation,
        context: &CryptoContext,
    ) -> Result<bool> {
        // Determine hardware usage based on operation type and security level
        match operation {
            CryptoOperation::DeriveKey { .. } => {
                // Key derivation can benefit from TEE protection
                Ok(self.config.enable_tee_operations
                    && context.security_level >= SecurityLevel::High)
            }

            CryptoOperation::GenerateSignature { .. } => {
                // Signature generation should use HSM for critical operations
                Ok(self.config.enable_hsm_operations
                    && context.security_level >= SecurityLevel::Critical)
            }

            CryptoOperation::RotateKeys { .. } => {
                // Key rotation requires highest security
                Ok(
                    (self.config.enable_hsm_operations || self.config.enable_tee_operations)
                        && context.security_level >= SecurityLevel::Critical,
                )
            }

            CryptoOperation::GenerateRandom { num_bytes } => {
                // Use hardware RNG for large or critical random generation
                Ok(self.config.enable_hardware_rng
                    && (*num_bytes > self.config.hardware_rng_threshold
                        || context.security_level >= SecurityLevel::Critical))
            }

            CryptoOperation::Encrypt { .. } | CryptoOperation::Decrypt { .. } => {
                // Encryption/decryption can use TEE for high security
                Ok(self.config.enable_tee_operations
                    && context.security_level >= SecurityLevel::High)
            }

            _ => {
                // Other operations don't require hardware security by default
                Ok(false)
            }
        }
    }

    fn check_hardware_requirements(&self, operation: &CryptoOperation) -> Result<()> {
        let required_capabilities = self.get_required_capabilities(operation);
        let available_capabilities = self.check_hardware_capabilities()?;

        // Check TEE requirements
        if required_capabilities.requires_tee && !available_capabilities.tee_available {
            return Err(CryptoError::hardware_not_available(
                "TEE (Trusted Execution Environment) not available",
            ));
        }

        // Check HSM requirements
        if required_capabilities.requires_hsm && !available_capabilities.hsm_available {
            return Err(CryptoError::hardware_not_available(
                "HSM (Hardware Security Module) not available",
            ));
        }

        // Check hardware RNG requirements
        if required_capabilities.requires_hardware_rng
            && !available_capabilities.hardware_rng_available
        {
            return Err(CryptoError::hardware_not_available(
                "Hardware RNG not available",
            ));
        }

        Ok(())
    }

    fn get_required_capabilities(&self, operation: &CryptoOperation) -> RequiredCapabilities {
        match operation {
            CryptoOperation::GenerateSignature { .. } | CryptoOperation::RotateKeys { .. } => {
                RequiredCapabilities {
                    requires_tee: false,
                    requires_hsm: self.config.enable_hsm_operations,
                    requires_hardware_rng: false,
                }
            }

            CryptoOperation::DeriveKey { .. }
            | CryptoOperation::Encrypt { .. }
            | CryptoOperation::Decrypt { .. } => RequiredCapabilities {
                requires_tee: self.config.enable_tee_operations,
                requires_hsm: false,
                requires_hardware_rng: false,
            },

            CryptoOperation::GenerateRandom { .. } => RequiredCapabilities {
                requires_tee: false,
                requires_hsm: false,
                requires_hardware_rng: self.config.enable_hardware_rng,
            },

            _ => RequiredCapabilities {
                requires_tee: false,
                requires_hsm: false,
                requires_hardware_rng: false,
            },
        }
    }

    fn validate_hardware_attestation(&self, _context: &CryptoContext) -> Result<()> {
        // Simplified attestation validation - in real implementation would:
        // - Verify platform attestation certificates
        // - Check TEE/HSM firmware signatures
        // - Validate security state and configuration
        // - Ensure no tampering or compromise

        if !self.config.require_attestation {
            return Ok(());
        }

        // Simulate attestation validation
        if !self.config.attestation_valid {
            return Err(CryptoError::attestation_failed(
                "Hardware attestation validation failed",
            ));
        }

        Ok(())
    }

    fn record_hardware_operation(&self, operation: &CryptoOperation) -> Result<()> {
        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on hardware tracker")
        })?;

        let operation_type = self.operation_type_string(operation);
        tracker.record_hardware_operation(operation_type);

        Ok(())
    }

    fn record_successful_hardware_operation(&self) -> Result<()> {
        let mut tracker = self.tracker.write().map_err(|_| {
            CryptoError::internal_error("Failed to acquire write lock on hardware tracker")
        })?;

        tracker.successful_hardware_operations += 1;

        Ok(())
    }

    fn operation_type_string(&self, operation: &CryptoOperation) -> String {
        match operation {
            CryptoOperation::DeriveKey { .. } => "derive_key".to_string(),
            CryptoOperation::GenerateSignature { .. } => "generate_signature".to_string(),
            CryptoOperation::VerifySignature { .. } => "verify_signature".to_string(),
            CryptoOperation::GenerateRandom { .. } => "generate_random".to_string(),
            CryptoOperation::RotateKeys { .. } => "rotate_keys".to_string(),
            CryptoOperation::Encrypt { .. } => "encrypt".to_string(),
            CryptoOperation::Decrypt { .. } => "decrypt".to_string(),
            CryptoOperation::Hash { .. } => "hash".to_string(),
        }
    }

    fn get_platform_info(&self) -> serde_json::Value {
        // Simplified platform info - in real implementation would include:
        // - Hardware platform details
        // - TEE/HSM versions and capabilities
        // - Security configuration
        // - Attestation chain information

        serde_json::json!({
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "tee_available": self.config.enable_tee_operations,
            "hsm_available": self.config.enable_hsm_operations,
            "hardware_rng": self.config.enable_hardware_rng
        })
    }
}

/// Configuration for hardware security middleware
#[derive(Debug, Clone)]
pub struct HardwareConfig {
    /// Whether to enable TEE operations
    pub enable_tee_operations: bool,

    /// Whether to enable HSM operations
    pub enable_hsm_operations: bool,

    /// Whether to enable hardware RNG
    pub enable_hardware_rng: bool,

    /// Whether to require hardware attestation
    pub require_attestation: bool,

    /// Whether attestation is valid (for testing)
    pub attestation_valid: bool,

    /// Whether HSM is available (for testing)
    pub hsm_available: bool,

    /// Threshold for using hardware RNG (bytes)
    pub hardware_rng_threshold: usize,

    /// Whether to include attestation info in responses
    pub include_attestation_info: bool,

    /// TEE/HSM operation timeout
    pub hardware_operation_timeout: Duration,
}

impl Default for HardwareConfig {
    fn default() -> Self {
        Self {
            enable_tee_operations: false, // Disabled by default - requires platform support
            enable_hsm_operations: false, // Disabled by default - requires HSM hardware
            enable_hardware_rng: false,   // Disabled by default - requires hardware support
            require_attestation: false,
            attestation_valid: true,      // For testing purposes
            hsm_available: false,         // For testing purposes
            hardware_rng_threshold: 1024, // Use hardware RNG for >1KB
            include_attestation_info: false,
            hardware_operation_timeout: Duration::from_secs(30),
        }
    }
}

/// Required hardware capabilities for an operation
#[derive(Debug, Clone)]
struct RequiredCapabilities {
    requires_tee: bool,
    requires_hsm: bool,
    requires_hardware_rng: bool,
}

/// Available hardware capabilities
#[derive(Debug, Clone)]
pub struct HardwareCapabilities {
    pub tee_available: bool,
    pub hsm_available: bool,
    pub hardware_rng_available: bool,
    pub attestation_supported: bool,
}

/// Hardware operation tracker
struct HardwareTracker {
    hardware_operations: HashMap<String, u64>, // operation_type -> count
    total_hardware_operations: u64,
    successful_hardware_operations: u64,
    failed_hardware_operations: u64,
    hsm_tests: u64,
    successful_hsm_tests: u64,
    failed_hsm_tests: u64,
}

impl HardwareTracker {
    fn new() -> Self {
        Self {
            hardware_operations: HashMap::new(),
            total_hardware_operations: 0,
            successful_hardware_operations: 0,
            failed_hardware_operations: 0,
            hsm_tests: 0,
            successful_hsm_tests: 0,
            failed_hsm_tests: 0,
        }
    }

    fn record_hardware_operation(&mut self, operation_type: String) {
        *self.hardware_operations.entry(operation_type).or_insert(0) += 1;
        self.total_hardware_operations += 1;
    }

    fn get_capabilities(&self) -> HardwareCapabilities {
        // Simplified capability detection - in real implementation would:
        // - Query actual hardware capabilities
        // - Test TEE/HSM availability
        // - Check attestation support
        // - Verify hardware RNG functionality

        HardwareCapabilities {
            tee_available: false,          // Would detect actual TEE
            hsm_available: false,          // Would detect actual HSM
            hardware_rng_available: false, // Would detect hardware RNG
            attestation_supported: false,  // Would check attestation support
        }
    }

    fn stats(&self) -> HardwareStats {
        HardwareStats {
            total_hardware_operations: self.total_hardware_operations,
            successful_hardware_operations: self.successful_hardware_operations,
            failed_hardware_operations: self.failed_hardware_operations,
            hsm_tests: self.hsm_tests,
            successful_hsm_tests: self.successful_hsm_tests,
            failed_hsm_tests: self.failed_hsm_tests,
            operation_counts: self.hardware_operations.clone(),
        }
    }
}

/// Hardware security statistics
#[derive(Debug, Clone)]
pub struct HardwareStats {
    /// Total hardware-secured operations
    pub total_hardware_operations: u64,

    /// Successful hardware operations
    pub successful_hardware_operations: u64,

    /// Failed hardware operations
    pub failed_hardware_operations: u64,

    /// HSM connectivity tests
    pub hsm_tests: u64,

    /// Successful HSM tests
    pub successful_hsm_tests: u64,

    /// Failed HSM tests
    pub failed_hsm_tests: u64,

    /// Operation counts by type
    pub operation_counts: HashMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::middleware::handler::NoOpHandler;
    use aura_crypto::Effects;
    use aura_types::{AccountIdExt, DeviceIdExt};

    #[test]
    fn test_hardware_security_middleware() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let middleware = HardwareSecurityMiddleware::new(HardwareConfig::default());
        let handler = NoOpHandler;
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Standard,
        );
        let operation = CryptoOperation::GenerateRandom { num_bytes: 32 };

        let result = middleware.process(operation, &context, &handler);
        assert!(result.is_ok());

        // Should not use hardware for small random generation at standard security
        let response = result.unwrap();
        assert_eq!(response.get("hardware_secured").unwrap(), false);
    }

    #[test]
    fn test_hardware_requirements_detection() {
        let config = HardwareConfig {
            enable_hsm_operations: true,
            ..HardwareConfig::default()
        };
        let middleware = HardwareSecurityMiddleware::new(config);

        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        // Critical security level signature should use hardware
        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Critical,
        );
        let operation = CryptoOperation::GenerateSignature {
            message: b"test".to_vec(),
            signing_package: b"package".to_vec(),
        };

        let should_use = middleware
            .should_use_hardware(&operation, &context)
            .unwrap();
        assert!(should_use);
    }

    #[test]
    fn test_hardware_capabilities() {
        let middleware = HardwareSecurityMiddleware::new(HardwareConfig::default());
        let capabilities = middleware.check_hardware_capabilities().unwrap();

        // Default config should have no hardware capabilities
        assert!(!capabilities.tee_available);
        assert!(!capabilities.hsm_available);
        assert!(!capabilities.hardware_rng_available);
        assert!(!capabilities.attestation_supported);
    }

    #[test]
    fn test_hsm_availability() {
        let config = HardwareConfig {
            enable_hsm_operations: true,
            hsm_available: true,
            ..HardwareConfig::default()
        };
        let middleware = HardwareSecurityMiddleware::new(config);

        let available = middleware.test_hsm_availability().unwrap();
        assert!(available);

        let stats = middleware.stats();
        assert_eq!(stats.hsm_tests, 1);
        assert_eq!(stats.successful_hsm_tests, 1);
    }

    #[test]
    fn test_attestation_validation() {
        let effects = Effects::test(42);
        let account_id = aura_types::AccountId::new_with_effects(&effects);
        let device_id = aura_types::DeviceId::new_with_effects(&effects);

        let config = HardwareConfig {
            require_attestation: true,
            attestation_valid: false, // Invalid attestation
            ..HardwareConfig::default()
        };
        let middleware = HardwareSecurityMiddleware::new(config);

        let context = CryptoContext::new(
            account_id,
            device_id,
            "test".to_string(),
            SecurityLevel::Critical,
        );

        // Should fail with invalid attestation
        let result = middleware.validate_hardware_attestation(&context);
        assert!(result.is_err());
    }
}
