//! Layer 3: Biometric Authentication Effect Handlers
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
use std::collections::HashMap;

/// Real biometric handler for production use
#[derive(Debug)]
pub struct RealBiometricHandler {
    platform_config: String,
}

impl RealBiometricHandler {
    /// Create a new real biometric handler
    pub fn new() -> Result<Self, BiometricError> {
        // Production integrations would initialize platform APIs here.
        Ok(Self {
            platform_config: "software-fallback".to_string(),
        })
    }
}

impl Default for RealBiometricHandler {
    fn default() -> Self {
        Self {
            platform_config: "software-fallback".to_string(),
        }
    }
}

#[async_trait]
impl BiometricEffects for RealBiometricHandler {
    fn supports_hardware_security(&self) -> bool {
        false
    }

    fn get_platform_capabilities(&self) -> Vec<String> {
        vec![self.platform_config.clone()]
    }

    async fn get_biometric_capabilities(&self) -> Result<Vec<BiometricCapability>, BiometricError> {
        let capabilities = vec![
            BiometricType::Fingerprint,
            BiometricType::Face,
            BiometricType::Iris,
            BiometricType::Voice,
            BiometricType::PalmPrint,
            BiometricType::Behavioral,
        ]
        .into_iter()
        .map(|biometric_type| {
            let security_level = biometric_type.security_level();
            BiometricCapability {
                biometric_type,
                available: false,
                hardware_present: false,
                enrolled: false,
                security_level,
                platform_features: vec!["software-fallback".to_string()],
            }
        })
        .collect();

        Ok(capabilities)
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
        Ok(BiometricEnrollmentResult {
            success: false,
            template_id: None,
            quality_score: None,
            samples_captured: 0,
            error: Some(
                "Biometric hardware not available in software-fallback handler".to_string(),
            ),
        })
    }

    async fn verify_biometric(
        &self,
        _biometric_type: BiometricType,
        _user_prompt: &str,
        _template_id: Option<&str>,
    ) -> Result<BiometricVerificationResult, BiometricError> {
        Ok(BiometricVerificationResult {
            verified: false,
            confidence_score: Some(0.0),
            matched_template_id: None,
            liveness_detected: Some(false),
            verification_time_ms: 0,
            error: Some(
                "Biometric verification not available on this platform handler".to_string(),
            ),
        })
    }

    async fn delete_biometric_template(
        &self,
        _biometric_type: BiometricType,
        _template_id: Option<&str>,
    ) -> Result<(), BiometricError> {
        // No-op: nothing stored in this handler
        Ok(())
    }

    async fn list_enrolled_templates(
        &self,
    ) -> Result<Vec<(String, BiometricType, f32)>, BiometricError> {
        Ok(Vec::new())
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
        Ok(BiometricStatistics {
            total_attempts: 0,
            successful_verifications: 0,
            failed_attempts: 0,
            average_verification_time_ms: 0,
            enrolled_templates_by_type: HashMap::new(),
            last_verification_at: None,
            false_acceptance_rate: None,
            false_rejection_rate: None,
        })
    }

    async fn cancel_biometric_operation(&self) -> Result<(), BiometricError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_real_biometric_handler_creation_succeeds() {
        let result = RealBiometricHandler::new();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_real_biometric_handler_capabilities() {
        let handler = RealBiometricHandler::default();
        let result = handler.get_biometric_capabilities().await;
        assert!(result.is_ok());
    }
}
