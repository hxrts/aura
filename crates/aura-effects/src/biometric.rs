//! Layer 3: Biometric Authentication Effect Handlers - Production Only
//!
//! Stateless single-party implementation of BiometricEffects from aura-core (Layer 1).
//! This handler implements pure biometric effect operations, delegating to platform APIs.
//!
//! **Layer Constraint**: NO mock handlers - those belong in aura-testkit (Layer 8).
//! This module contains only production-grade stateless handlers.

use async_trait::async_trait;
use aura_core::effects::{
    BiometricCapability, BiometricConfig, BiometricEffects, BiometricEnrollmentResult,
    BiometricError, BiometricStatistics, BiometricType, BiometricVerificationResult,
};

/// Real biometric handler for production use
///
/// TODO: Implement platform-specific biometric authentication
#[derive(Debug)]
pub struct RealBiometricHandler {
    _platform_config: String,
}

impl RealBiometricHandler {
    /// Create a new real biometric handler
    pub fn new() -> Result<Self, BiometricError> {
        // TODO: Initialize platform-specific biometric APIs
        Err(BiometricError::invalid("Real biometric authentication not yet implemented - use MockBiometricHandler from aura-testkit for testing"))
    }
}

impl Default for RealBiometricHandler {
    fn default() -> Self {
        Self {
            _platform_config: "unimplemented".to_string(),
        }
    }
}

#[async_trait]
impl BiometricEffects for RealBiometricHandler {
    fn supports_hardware_security(&self) -> bool {
        false
    }

    fn get_platform_capabilities(&self) -> Vec<String> {
        Vec::new()
    }

    async fn get_biometric_capabilities(&self) -> Result<Vec<BiometricCapability>, BiometricError> {
        Err(BiometricError::invalid(
            "Real biometric capabilities not yet implemented",
        ))
    }

    async fn is_biometric_available(
        &self,
        _biometric_type: BiometricType,
    ) -> Result<bool, BiometricError> {
        Ok(false)
    }

    async fn enroll_biometric(
        &self,
        _config: BiometricConfig,
        _user_prompt: &str,
    ) -> Result<BiometricEnrollmentResult, BiometricError> {
        Err(BiometricError::invalid(
            "Real biometric enrollment not yet implemented",
        ))
    }

    async fn verify_biometric(
        &self,
        _biometric_type: BiometricType,
        _user_prompt: &str,
        _template_id: Option<&str>,
    ) -> Result<BiometricVerificationResult, BiometricError> {
        Err(BiometricError::invalid(
            "Real biometric verification not yet implemented",
        ))
    }

    async fn delete_biometric_template(
        &self,
        _biometric_type: BiometricType,
        _template_id: Option<&str>,
    ) -> Result<(), BiometricError> {
        Err(BiometricError::invalid(
            "Real biometric deletion not yet implemented",
        ))
    }

    async fn list_enrolled_templates(
        &self,
    ) -> Result<Vec<(String, BiometricType, f32)>, BiometricError> {
        Err(BiometricError::invalid(
            "Real biometric listing not yet implemented",
        ))
    }

    async fn test_biometric_hardware(
        &self,
        _biometric_type: BiometricType,
    ) -> Result<bool, BiometricError> {
        Ok(false)
    }

    async fn configure_biometric_security(
        &self,
        _config: BiometricConfig,
    ) -> Result<(), BiometricError> {
        Ok(())
    }

    async fn get_biometric_statistics(&self) -> Result<BiometricStatistics, BiometricError> {
        Err(BiometricError::invalid(
            "Real biometric statistics not yet implemented",
        ))
    }

    async fn cancel_biometric_operation(&self) -> Result<(), BiometricError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_real_biometric_handler_creation_fails() {
        let result = RealBiometricHandler::new();
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_real_biometric_handler_capabilities() {
        let handler = RealBiometricHandler::default();
        let result = handler.get_biometric_capabilities().await;
        assert!(result.is_err());
    }
}
