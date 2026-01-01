//! Biometric Authentication Effects Trait Definitions
//!
//! This module defines trait interfaces for biometric authentication operations
//! that interface with platform-specific biometric APIs (TouchID, FaceID,
//! Windows Hello, fingerprint scanners, etc.).
//!
//! # Effect Classification
//!
//! - **Category**: Infrastructure Effect
//! - **Implementation**: `aura-effects` (Layer 3)
//! - **Usage**: Platform-specific biometric APIs for authentication
//!
//! This is an infrastructure effect that provides OS-level biometric integration
//! with no Aura-specific semantics. Implementations should interface with platform
//! biometric APIs and provide software fallback for testing environments.
//!
//! ## Security Model
//!
//! Biometric authentication provides:
//! - Hardware-backed biometric verification
//! - Template storage in secure enclaves
//! - Liveness detection to prevent spoofing
//! - Privacy-preserving matching (templates never leave device)
//!
//! ## Platform Support
//!
//! - iOS: Touch ID, Face ID via LocalAuthentication framework
//! - Android: Fingerprint, Face unlock via BiometricPrompt API
//! - Windows: Windows Hello (fingerprint, face, iris)
//! - macOS: Touch ID via LocalAuthentication framework
//! - Linux: Fingerprint readers via libfprint

use crate::AuraError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Biometric authentication operation error
pub type BiometricError = AuraError;

/// Types of biometric authentication supported
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BiometricType {
    /// Fingerprint recognition
    Fingerprint,
    /// Face recognition
    Face,
    /// Iris recognition
    Iris,
    /// Voice recognition
    Voice,
    /// Palm print recognition
    PalmPrint,
    /// Behavioral biometrics (typing patterns, etc.)
    Behavioral,
}

impl BiometricType {
    /// Get human-readable name for the biometric type
    pub fn display_name(&self) -> &'static str {
        match self {
            BiometricType::Fingerprint => "Fingerprint",
            BiometricType::Face => "Face ID",
            BiometricType::Iris => "Iris",
            BiometricType::Voice => "Voice",
            BiometricType::PalmPrint => "Palm Print",
            BiometricType::Behavioral => "Behavioral",
        }
    }

    /// Get recommended security level for this biometric type
    pub fn security_level(&self) -> BiometricSecurityLevel {
        match self {
            BiometricType::Fingerprint => BiometricSecurityLevel::High,
            BiometricType::Face => BiometricSecurityLevel::High,
            BiometricType::Iris => BiometricSecurityLevel::VeryHigh,
            BiometricType::Voice => BiometricSecurityLevel::Medium,
            BiometricType::PalmPrint => BiometricSecurityLevel::High,
            BiometricType::Behavioral => BiometricSecurityLevel::Low,
        }
    }
}

/// Security level classification for biometric authentication
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BiometricSecurityLevel {
    /// Low security (e.g., behavioral biometrics)
    Low,
    /// Medium security (e.g., voice recognition)
    Medium,
    /// High security (e.g., fingerprint, face)
    High,
    /// Very high security (e.g., iris recognition)
    VeryHigh,
}

/// Configuration for biometric enrollment and verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricConfig {
    /// Biometric type to configure
    pub biometric_type: BiometricType,
    /// Require liveness detection during verification
    pub liveness_detection: bool,
    /// Allow fallback to alternative authentication if biometric fails
    pub allow_fallback: bool,
    /// Timeout for biometric capture (milliseconds)
    pub capture_timeout_ms: u32,
    /// Number of retry attempts allowed
    pub max_retry_attempts: u32,
    /// Minimum quality threshold (0.0 to 1.0)
    pub minimum_quality: f32,
}

impl BiometricConfig {
    /// Create a high-security configuration for the given biometric type
    pub fn high_security(biometric_type: BiometricType) -> Self {
        Self {
            biometric_type,
            liveness_detection: true,
            allow_fallback: false,
            capture_timeout_ms: 10000, // 10 seconds
            max_retry_attempts: 3,
            minimum_quality: 0.8,
        }
    }

    /// Create a balanced configuration for the given biometric type
    pub fn balanced(biometric_type: BiometricType) -> Self {
        Self {
            biometric_type,
            liveness_detection: true,
            allow_fallback: true,
            capture_timeout_ms: 15000, // 15 seconds
            max_retry_attempts: 5,
            minimum_quality: 0.6,
        }
    }

    /// Create a user-friendly configuration for the given biometric type
    pub fn user_friendly(biometric_type: BiometricType) -> Self {
        Self {
            biometric_type,
            liveness_detection: false,
            allow_fallback: true,
            capture_timeout_ms: 30000, // 30 seconds
            max_retry_attempts: 10,
            minimum_quality: 0.4,
        }
    }
}

/// Result of biometric enrollment operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricEnrollmentResult {
    /// Whether enrollment was successful
    pub success: bool,
    /// Unique identifier for the enrolled template
    pub template_id: Option<String>,
    /// Quality score of the enrolled template (0.0 to 1.0)
    pub quality_score: Option<f32>,
    /// Number of samples captured during enrollment
    pub samples_captured: u32,
    /// Error message if enrollment failed
    pub error: Option<String>,
}

/// Result of biometric verification operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricVerificationResult {
    /// Whether verification was successful
    pub verified: bool,
    /// Confidence score of the match (0.0 to 1.0)
    pub confidence_score: Option<f32>,
    /// Template ID that was matched
    pub matched_template_id: Option<String>,
    /// Whether liveness was detected (if enabled)
    pub liveness_detected: Option<bool>,
    /// Time taken for verification (milliseconds)
    pub verification_time_ms: u32,
    /// Error message if verification failed
    pub error: Option<String>,
}

/// Information about available biometric capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiometricCapability {
    /// Type of biometric supported
    pub biometric_type: BiometricType,
    /// Whether this biometric is currently available
    pub available: bool,
    /// Whether biometric hardware is present
    pub hardware_present: bool,
    /// Whether biometric data is enrolled
    pub enrolled: bool,
    /// Security level of this biometric
    pub security_level: BiometricSecurityLevel,
    /// Platform-specific capability flags
    pub platform_features: Vec<String>,
}

/// Biometric effects interface
///
/// This trait defines operations for biometric authentication that interface
/// with platform-specific biometric APIs and hardware security modules.
///
/// # Implementation Notes
///
/// - Production: Interface with platform APIs (LocalAuthentication, BiometricPrompt, Windows Hello)
/// - Testing: Simulate biometric operations with configurable success/failure patterns
/// - Simulation: Deterministic biometric verification for reproducible testing
///
/// # Security Properties
///
/// - Templates stored in hardware security modules when available
/// - Biometric data never transmitted over network
/// - Liveness detection to prevent spoofing attacks
/// - Secure template comparison in hardware
///
/// # Stability: EXPERIMENTAL
/// This API is under development and may change in future versions.
#[async_trait]
pub trait BiometricEffects: Send + Sync {
    /// Check what biometric capabilities are available on this device
    ///
    /// Queries the platform to determine which biometric authentication
    /// methods are supported and currently available.
    ///
    /// # Returns
    /// List of available biometric capabilities with their current status
    async fn get_biometric_capabilities(&self) -> Result<Vec<BiometricCapability>, BiometricError>;

    /// Check if a specific biometric type is available and enrolled
    ///
    /// Quick check for whether a specific biometric authentication method
    /// can be used immediately.
    ///
    /// # Parameters
    /// - `biometric_type`: Type of biometric to check
    ///
    /// # Returns
    /// `true` if the biometric is available and has enrolled data
    async fn is_biometric_available(
        &self,
        biometric_type: BiometricType,
    ) -> Result<bool, BiometricError>;

    /// Enroll a new biometric template
    ///
    /// Captures biometric data from the user and stores a template securely.
    /// This typically involves multiple samples to create a high-quality template.
    ///
    /// # Parameters
    /// - `config`: Configuration for the enrollment process
    /// - `user_prompt`: Message to display to the user during enrollment
    ///
    /// # Returns
    /// Result containing enrollment success/failure and template information
    async fn enroll_biometric(
        &self,
        config: BiometricConfig,
        user_prompt: &str,
    ) -> Result<BiometricEnrollmentResult, BiometricError>;

    /// Verify a user using biometric authentication
    ///
    /// Captures biometric data and compares it against enrolled templates.
    /// Returns verification result with confidence score.
    ///
    /// # Parameters
    /// - `biometric_type`: Type of biometric to verify
    /// - `user_prompt`: Message to display to the user during verification
    /// - `template_id`: Optional specific template to verify against
    ///
    /// # Returns
    /// Result containing verification success/failure and match information
    async fn verify_biometric(
        &self,
        biometric_type: BiometricType,
        user_prompt: &str,
        template_id: Option<&str>,
    ) -> Result<BiometricVerificationResult, BiometricError>;

    /// Delete an enrolled biometric template
    ///
    /// Securely removes a biometric template from storage.
    ///
    /// # Parameters
    /// - `biometric_type`: Type of biometric template to remove
    /// - `template_id`: ID of the specific template to remove (if None, removes all)
    ///
    /// # Returns
    /// Success/failure result
    async fn delete_biometric_template(
        &self,
        biometric_type: BiometricType,
        template_id: Option<&str>,
    ) -> Result<(), BiometricError>;

    /// List enrolled biometric templates
    ///
    /// Returns information about biometric templates currently enrolled
    /// on the device, without exposing the actual template data.
    ///
    /// # Returns
    /// List of template IDs and metadata for enrolled biometrics
    async fn list_enrolled_templates(
        &self,
    ) -> Result<Vec<(String, BiometricType, f32)>, BiometricError>; // (id, type, quality)

    /// Test biometric hardware functionality
    ///
    /// Performs a basic test of biometric hardware to ensure it's functioning
    /// correctly. This doesn't require user interaction.
    ///
    /// # Parameters
    /// - `biometric_type`: Type of biometric hardware to test
    ///
    /// # Returns
    /// `true` if hardware is functioning correctly
    async fn test_biometric_hardware(
        &self,
        biometric_type: BiometricType,
    ) -> Result<bool, BiometricError>;

    /// Configure biometric security settings
    ///
    /// Updates platform-specific security settings for biometric authentication.
    ///
    /// # Parameters
    /// - `config`: New configuration to apply
    ///
    /// # Returns
    /// Success/failure result
    async fn configure_biometric_security(
        &self,
        config: BiometricConfig,
    ) -> Result<(), BiometricError>;

    /// Get biometric authentication statistics
    ///
    /// Returns usage statistics and performance metrics for biometric authentication.
    ///
    /// # Returns
    /// Statistics about biometric authentication usage
    async fn get_biometric_statistics(&self) -> Result<BiometricStatistics, BiometricError>;

    /// Cancel any ongoing biometric operation
    ///
    /// Cancels enrollment or verification operations that are currently in progress.
    async fn cancel_biometric_operation(&self) -> Result<(), BiometricError>;

    /// Check if this implementation supports hardware security
    fn supports_hardware_security(&self) -> bool;

    /// Get platform-specific biometric capabilities
    fn get_platform_capabilities(&self) -> Vec<String>;
}

/// Statistics about biometric authentication usage
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BiometricStatistics {
    /// Total number of verification attempts
    pub total_attempts: u64,
    /// Number of successful verifications
    pub successful_verifications: u64,
    /// Number of failed verification attempts
    pub failed_attempts: u64,
    /// Average verification time in milliseconds
    pub average_verification_time_ms: u32,
    /// Number of enrolled templates by type
    pub enrolled_templates_by_type: std::collections::HashMap<BiometricType, u32>,
    /// Last verification timestamp
    pub last_verification_at: Option<u64>,
    /// False acceptance rate (if available)
    pub false_acceptance_rate: Option<f32>,
    /// False rejection rate (if available)
    pub false_rejection_rate: Option<f32>,
}

/// Helper functions for common biometric operations
impl BiometricCapability {
    /// Check if this biometric can be used for authentication
    pub fn is_usable(&self) -> bool {
        self.available && self.hardware_present && self.enrolled
    }

    /// Check if this biometric meets a minimum security level
    pub fn meets_security_level(&self, required_level: BiometricSecurityLevel) -> bool {
        matches!(
            (required_level, &self.security_level),
            (BiometricSecurityLevel::Low, _)
                | (
                    BiometricSecurityLevel::Medium,
                    BiometricSecurityLevel::Medium
                )
                | (BiometricSecurityLevel::Medium, BiometricSecurityLevel::High)
                | (
                    BiometricSecurityLevel::Medium,
                    BiometricSecurityLevel::VeryHigh
                )
                | (BiometricSecurityLevel::High, BiometricSecurityLevel::High)
                | (
                    BiometricSecurityLevel::High,
                    BiometricSecurityLevel::VeryHigh
                )
                | (
                    BiometricSecurityLevel::VeryHigh,
                    BiometricSecurityLevel::VeryHigh
                )
        )
    }
}

impl BiometricVerificationResult {
    /// Check if verification was successful with sufficient confidence
    pub fn is_verified_with_confidence(&self, minimum_confidence: f32) -> bool {
        self.verified
            && self
                .confidence_score
                .is_some_and(|score| score >= minimum_confidence)
    }

    /// Check if liveness was properly detected (when required)
    pub fn has_liveness(&self) -> bool {
        self.liveness_detected.unwrap_or(true) // Assume liveness if not checked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_biometric_type_display_names() {
        assert_eq!(BiometricType::Fingerprint.display_name(), "Fingerprint");
        assert_eq!(BiometricType::Face.display_name(), "Face ID");
        assert_eq!(BiometricType::Iris.display_name(), "Iris");
    }

    #[test]
    fn test_biometric_security_levels() {
        assert_eq!(
            BiometricType::Fingerprint.security_level(),
            BiometricSecurityLevel::High
        );
        assert_eq!(
            BiometricType::Iris.security_level(),
            BiometricSecurityLevel::VeryHigh
        );
        assert_eq!(
            BiometricType::Voice.security_level(),
            BiometricSecurityLevel::Medium
        );
    }

    #[test]
    fn test_biometric_config_presets() {
        let high_sec = BiometricConfig::high_security(BiometricType::Face);
        assert!(high_sec.liveness_detection);
        assert!(!high_sec.allow_fallback);
        assert!(high_sec.minimum_quality >= 0.8);

        let user_friendly = BiometricConfig::user_friendly(BiometricType::Fingerprint);
        assert!(!user_friendly.liveness_detection);
        assert!(user_friendly.allow_fallback);
        assert!(user_friendly.max_retry_attempts >= 10);
    }

    #[test]
    fn test_biometric_capability_usability() {
        let usable = BiometricCapability {
            biometric_type: BiometricType::Fingerprint,
            available: true,
            hardware_present: true,
            enrolled: true,
            security_level: BiometricSecurityLevel::High,
            platform_features: vec![],
        };
        assert!(usable.is_usable());

        let not_enrolled = BiometricCapability {
            enrolled: false,
            ..usable
        };
        assert!(!not_enrolled.is_usable());
    }

    #[test]
    fn test_security_level_requirements() {
        let high_sec_capability = BiometricCapability {
            biometric_type: BiometricType::Iris,
            available: true,
            hardware_present: true,
            enrolled: true,
            security_level: BiometricSecurityLevel::VeryHigh,
            platform_features: vec![],
        };

        assert!(high_sec_capability.meets_security_level(BiometricSecurityLevel::Low));
        assert!(high_sec_capability.meets_security_level(BiometricSecurityLevel::Medium));
        assert!(high_sec_capability.meets_security_level(BiometricSecurityLevel::High));
        assert!(high_sec_capability.meets_security_level(BiometricSecurityLevel::VeryHigh));

        let low_sec_capability = BiometricCapability {
            security_level: BiometricSecurityLevel::Low,
            ..high_sec_capability
        };
        assert!(low_sec_capability.meets_security_level(BiometricSecurityLevel::Low));
        assert!(!low_sec_capability.meets_security_level(BiometricSecurityLevel::High));
    }

    #[test]
    fn test_verification_result_confidence() {
        let good_result = BiometricVerificationResult {
            verified: true,
            confidence_score: Some(0.95),
            matched_template_id: Some("template1".to_string()),
            liveness_detected: Some(true),
            verification_time_ms: 500,
            error: None,
        };

        assert!(good_result.is_verified_with_confidence(0.9));
        assert!(!good_result.is_verified_with_confidence(0.99));
        assert!(good_result.has_liveness());

        let low_confidence = BiometricVerificationResult {
            confidence_score: Some(0.5),
            ..good_result
        };
        assert!(!low_confidence.is_verified_with_confidence(0.9));
    }
}
