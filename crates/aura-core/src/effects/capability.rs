//! Capability Token Effects Trait Definitions
//!
//! This module defines trait interfaces for capability token operations including
//! parsing, verification, creation, and management. Capability tokens provide
//! fine-grained access control using cryptographically verifiable tokens.
//!
//! # Effect Classification
//!
//! - **Category**: Application Effect
//! - **Implementation**: `aura-authorization` (Layer 2)
//! - **Usage**: Capability token management (Biscuit, JWT, custom formats)
//!
//! This is an application effect implemented in domain crates by composing
//! infrastructure effects (crypto, storage) with capability-specific logic.
//!
//! ## Security Model
//!
//! Capability tokens provide:
//! - Cryptographic verification of permissions
//! - Delegation and attenuation of capabilities
//! - Time-bound and context-bound access control
//! - Revocation and audit trails
//!
//! ## Token Format
//!
//! Supports multiple token formats:
//! - Biscuit tokens (Datalog-based authorization)
//! - JWT tokens (JSON Web Tokens)
//! - Custom binary formats
//! - Macaroon-style tokens

use crate::AuraError;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Capability token operation error
pub type CapabilityError = AuraError;

/// Types of capability token formats supported
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CapabilityTokenFormat {
    /// Biscuit authorization tokens
    Biscuit,
    /// JSON Web Tokens (JWT)
    Jwt,
    /// Custom binary format
    Binary,
    /// Macaroon-style tokens
    Macaroon,
}

/// Capability token verification level
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum VerificationLevel {
    /// Basic signature verification only
    Basic,
    /// Signature + expiration check
    Standard,
    /// Full verification including revocation checks
    Full,
    /// Strict verification with all security checks
    Strict,
}

/// Result of capability token verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityVerificationResult {
    /// Whether the token is valid
    pub valid: bool,
    /// Extracted permissions from the token
    pub permissions: Vec<String>,
    /// Token expiration timestamp (if present)
    pub expires_at: Option<u64>,
    /// Subject (user/service) the token was issued for
    pub subject: Option<String>,
    /// Issuer of the token
    pub issuer: Option<String>,
    /// Additional claims/attributes from the token
    pub claims: HashMap<String, String>,
    /// Verification errors (if any)
    pub errors: Vec<String>,
    /// Verification level used
    pub verification_level: VerificationLevel,
}

/// Capability token creation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityTokenRequest {
    /// Subject to issue the token for
    pub subject: String,
    /// Permissions to grant
    pub permissions: Vec<String>,
    /// Token expiration time (timestamp)
    pub expires_at: Option<u64>,
    /// Additional claims to include
    pub claims: HashMap<String, String>,
    /// Token format to use
    pub format: CapabilityTokenFormat,
    /// Context restrictions (optional)
    pub context: Option<String>,
}

/// Information about a capability token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityTokenInfo {
    /// Unique identifier for this token
    pub token_id: String,
    /// Token format
    pub format: CapabilityTokenFormat,
    /// Subject the token was issued for
    pub subject: String,
    /// Issuer of the token
    pub issuer: String,
    /// Creation timestamp
    pub issued_at: u64,
    /// Expiration timestamp (if applicable)
    pub expires_at: Option<u64>,
    /// Current status of the token
    pub status: TokenStatus,
    /// Permissions granted by this token
    pub permissions: Vec<String>,
}

/// Status of a capability token
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TokenStatus {
    /// Token is active and valid
    Active,
    /// Token has expired
    Expired,
    /// Token has been revoked
    Revoked,
    /// Token is suspended temporarily
    Suspended,
    /// Token is pending activation
    Pending,
}

/// Configuration for capability token operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityConfig {
    /// Default token format to use
    pub default_format: CapabilityTokenFormat,
    /// Default expiration time (seconds from now)
    pub default_expiry_seconds: u64,
    /// Whether to enforce strict verification by default
    pub strict_verification: bool,
    /// Maximum number of permissions per token
    pub max_permissions_per_token: u32,
    /// Whether to enable revocation checking
    pub enable_revocation_checks: bool,
    /// Clock skew tolerance (seconds)
    pub clock_skew_tolerance_seconds: u64,
}

impl Default for CapabilityConfig {
    fn default() -> Self {
        Self {
            default_format: CapabilityTokenFormat::Biscuit,
            default_expiry_seconds: 3600, // 1 hour
            strict_verification: true,
            max_permissions_per_token: 100,
            enable_revocation_checks: true,
            clock_skew_tolerance_seconds: 300, // 5 minutes
        }
    }
}

/// Capability token effects interface
///
/// This trait defines operations for creating, verifying, and managing capability tokens
/// that provide cryptographically verifiable access control.
///
/// # Implementation Notes
///
/// - Production: Interface with real cryptographic libraries (Biscuit, JWT, etc.)
/// - Testing: Simulate token operations with configurable verification outcomes
/// - Simulation: Deterministic token generation and verification for testing
///
/// # Security Properties
///
/// - Cryptographic signature verification
/// - Time-bound access control with expiration
/// - Revocation support and checking
/// - Delegation and attenuation capabilities
/// - Audit trail for token operations
///
/// # Stability: EXPERIMENTAL
/// This API is under development and may change in future versions.
#[async_trait]
pub trait CapabilityEffects: Send + Sync {
    /// Create a new capability token
    ///
    /// Generates a cryptographically signed capability token with the specified
    /// permissions and constraints.
    ///
    /// # Parameters
    /// - `request`: Token creation parameters including subject, permissions, expiration
    ///
    /// # Returns
    /// The generated token as bytes, or error if creation fails
    async fn create_capability_token(
        &self,
        request: CapabilityTokenRequest,
    ) -> Result<Vec<u8>, CapabilityError>;

    /// Verify a capability token
    ///
    /// Parses and verifies a capability token, checking signature, expiration,
    /// and other security constraints.
    ///
    /// # Parameters
    /// - `token`: Token bytes to verify
    /// - `verification_level`: Level of verification to perform
    /// - `required_permissions`: Optional list of permissions that must be present
    ///
    /// # Returns
    /// Verification result with token information and validation status
    async fn verify_capability_token(
        &self,
        token: &[u8],
        verification_level: VerificationLevel,
        required_permissions: Option<&[String]>,
    ) -> Result<CapabilityVerificationResult, CapabilityError>;

    /// Parse capability token without verification
    ///
    /// Extracts information from a token without performing cryptographic verification.
    /// Useful for debugging or when verification is performed separately.
    ///
    /// # Parameters
    /// - `token`: Token bytes to parse
    ///
    /// # Returns
    /// Token information without verification status
    async fn parse_capability_token(
        &self,
        token: &[u8],
    ) -> Result<CapabilityTokenInfo, CapabilityError>;

    /// Revoke a capability token
    ///
    /// Adds a token to the revocation list, making it invalid for future verification.
    ///
    /// # Parameters
    /// - `token_id`: Unique identifier of the token to revoke
    /// - `reason`: Reason for revocation (for audit trail)
    ///
    /// # Returns
    /// Success/failure result
    async fn revoke_capability_token(
        &self,
        token_id: &str,
        reason: &str,
    ) -> Result<(), CapabilityError>;

    /// Check if a token is revoked
    ///
    /// Queries the revocation list to check if a token has been revoked.
    ///
    /// # Parameters
    /// - `token_id`: Token identifier to check
    ///
    /// # Returns
    /// `true` if the token is revoked, `false` otherwise
    async fn is_token_revoked(&self, token_id: &str) -> Result<bool, CapabilityError>;

    /// Create a delegated token
    ///
    /// Creates a new token that is derived from an existing token with
    /// attenuated (reduced) permissions.
    ///
    /// # Parameters
    /// - `parent_token`: Parent token to delegate from
    /// - `new_permissions`: Subset of parent permissions for the new token
    /// - `new_expiry`: Optional new expiration time (must be <= parent expiry)
    /// - `subject`: Subject for the new token
    ///
    /// # Returns
    /// The delegated token bytes
    async fn delegate_capability_token(
        &self,
        parent_token: &[u8],
        new_permissions: &[String],
        new_expiry: Option<u64>,
        subject: &str,
    ) -> Result<Vec<u8>, CapabilityError>;

    /// List active tokens for a subject
    ///
    /// Returns information about all active tokens issued for a specific subject.
    ///
    /// # Parameters
    /// - `subject`: Subject to list tokens for
    ///
    /// # Returns
    /// List of active token information
    async fn list_subject_tokens(
        &self,
        subject: &str,
    ) -> Result<Vec<CapabilityTokenInfo>, CapabilityError>;

    /// Validate token permissions
    ///
    /// Checks if a verified token contains the required permissions for a specific operation.
    ///
    /// # Parameters
    /// - `verification_result`: Previous verification result
    /// - `required_permissions`: Permissions needed for the operation
    /// - `operation_context`: Optional context for the operation
    ///
    /// # Returns
    /// `true` if token has sufficient permissions, `false` otherwise
    async fn validate_token_permissions(
        &self,
        verification_result: &CapabilityVerificationResult,
        required_permissions: &[String],
        operation_context: Option<&str>,
    ) -> Result<bool, CapabilityError>;

    /// Get capability token statistics
    ///
    /// Returns statistics about token usage, creation, verification, and revocation.
    ///
    /// # Returns
    /// Token usage statistics
    async fn get_token_statistics(&self) -> Result<CapabilityStatistics, CapabilityError>;

    /// Configure capability token settings
    ///
    /// Updates the configuration for capability token operations.
    ///
    /// # Parameters
    /// - `config`: New configuration to apply
    ///
    /// # Returns
    /// Success/failure result
    async fn configure_capabilities(&self, config: CapabilityConfig)
        -> Result<(), CapabilityError>;

    /// Check what token formats are supported
    ///
    /// Returns the list of capability token formats supported by this implementation.
    ///
    /// # Returns
    /// List of supported token formats
    fn get_supported_formats(&self) -> Vec<CapabilityTokenFormat>;

    /// Check if this implementation supports cryptographic verification
    fn supports_cryptographic_verification(&self) -> bool;

    /// Get implementation capabilities
    fn get_capability_features(&self) -> Vec<String>;
}

/// Statistics about capability token usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityStatistics {
    /// Total number of tokens created
    pub total_tokens_created: u64,
    /// Total number of verification attempts
    pub total_verifications: u64,
    /// Number of successful verifications
    pub successful_verifications: u64,
    /// Number of failed verifications
    pub failed_verifications: u64,
    /// Number of revoked tokens
    pub revoked_tokens: u64,
    /// Number of expired tokens
    pub expired_tokens: u64,
    /// Average token lifetime (seconds)
    pub average_token_lifetime_seconds: u64,
    /// Most common permissions granted
    pub common_permissions: HashMap<String, u64>,
    /// Token creation rate (tokens per day)
    pub creation_rate_per_day: f64,
    /// Last token created timestamp
    pub last_token_created_at: Option<u64>,
    /// Last verification timestamp
    pub last_verification_at: Option<u64>,
}

impl Default for CapabilityStatistics {
    fn default() -> Self {
        Self {
            total_tokens_created: 0,
            total_verifications: 0,
            successful_verifications: 0,
            failed_verifications: 0,
            revoked_tokens: 0,
            expired_tokens: 0,
            average_token_lifetime_seconds: 3600,
            common_permissions: HashMap::new(),
            creation_rate_per_day: 0.0,
            last_token_created_at: None,
            last_verification_at: None,
        }
    }
}

/// Helper functions for common capability token operations
impl CapabilityTokenRequest {
    /// Create a standard token request with common settings
    pub fn standard(subject: &str, permissions: &[String]) -> Self {
        Self {
            subject: subject.to_string(),
            permissions: permissions.to_vec(),
            expires_at: None, // Use default expiration
            claims: HashMap::new(),
            format: CapabilityTokenFormat::Biscuit,
            context: None,
        }
    }

    /// Create a short-lived token request (5 minutes)
    ///
    /// Note: This method requires PhysicalTimeEffects to set expiration.
    /// Use `short_lived_with_time` for production code.
    pub fn short_lived(subject: &str, permissions: &[String]) -> Self {
        Self {
            subject: subject.to_string(),
            permissions: permissions.to_vec(),
            expires_at: None, // Must be set separately using PhysicalTimeEffects
            claims: HashMap::new(),
            format: CapabilityTokenFormat::Jwt,
            context: None,
        }
    }

    /// Create a short-lived token request with explicit expiration time
    pub fn short_lived_with_expiry(subject: &str, permissions: &[String], expires_at: u64) -> Self {
        Self {
            subject: subject.to_string(),
            permissions: permissions.to_vec(),
            expires_at: Some(expires_at),
            claims: HashMap::new(),
            format: CapabilityTokenFormat::Jwt,
            context: None,
        }
    }

    /// Create a read-only token request
    pub fn read_only(subject: &str, resource: &str) -> Self {
        let permissions = vec![format!("read:{}", resource)];
        Self::standard(subject, &permissions)
    }

    /// Add a custom claim to the token request
    pub fn with_claim(mut self, key: &str, value: &str) -> Self {
        self.claims.insert(key.to_string(), value.to_string());
        self
    }

    /// Set the expiration time for the token
    pub fn with_expiry(mut self, expires_at: u64) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Set the context for the token
    pub fn with_context(mut self, context: &str) -> Self {
        self.context = Some(context.to_string());
        self
    }
}

impl CapabilityVerificationResult {
    /// Check if the token is valid (signature verification passed)
    pub fn is_valid(&self) -> bool {
        self.valid
    }

    /// Check if the token is valid and not expired at the given time
    pub fn is_valid_at(&self, current_time_ms: u64) -> bool {
        if !self.valid {
            return false;
        }

        if let Some(expires_at) = self.expires_at {
            // Convert seconds to milliseconds for consistent time representation
            let expires_at_ms = expires_at * 1000;
            return current_time_ms < expires_at_ms;
        }

        true
    }

    /// Check if the token has all required permissions
    pub fn has_permissions(&self, required: &[String]) -> bool {
        required.iter().all(|perm| self.permissions.contains(perm))
    }

    /// Get the remaining validity time in seconds at the given time
    pub fn remaining_validity_seconds(&self, current_time_ms: u64) -> Option<u64> {
        self.expires_at.map(|expires| {
            let current_seconds = current_time_ms / 1000;
            expires.saturating_sub(current_seconds)
        })
    }
}

impl TokenStatus {
    /// Check if the token is usable
    pub fn is_active(&self) -> bool {
        matches!(self, TokenStatus::Active)
    }

    /// Check if the token is permanently invalid
    pub fn is_permanently_invalid(&self) -> bool {
        matches!(self, TokenStatus::Expired | TokenStatus::Revoked)
    }
}

// Removed chrono dependency - use PhysicalTimeEffects trait for time operations

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_token_request_creation() {
        let permissions = vec!["read:data".to_string(), "write:logs".to_string()];
        let request = CapabilityTokenRequest::standard("user123", &permissions);

        assert_eq!(request.subject, "user123");
        assert_eq!(request.permissions, permissions);
        assert_eq!(request.format, CapabilityTokenFormat::Biscuit);
        assert!(request.claims.is_empty());
    }

    #[test]
    fn test_capability_token_request_short_lived() {
        let permissions = vec!["admin:all".to_string()];
        let request = CapabilityTokenRequest::short_lived("admin", &permissions);

        assert_eq!(request.subject, "admin");
        assert_eq!(request.format, CapabilityTokenFormat::Jwt);
        assert!(request.expires_at.is_none()); // Must be set separately
    }

    #[test]
    fn test_capability_token_request_short_lived_with_expiry() {
        let permissions = vec!["admin:all".to_string()];
        let expires_at = 1234567890;
        let request =
            CapabilityTokenRequest::short_lived_with_expiry("admin", &permissions, expires_at);

        assert_eq!(request.subject, "admin");
        assert_eq!(request.format, CapabilityTokenFormat::Jwt);
        assert_eq!(request.expires_at, Some(expires_at));
    }

    #[test]
    fn test_capability_token_request_read_only() {
        let request = CapabilityTokenRequest::read_only("viewer", "database");

        assert_eq!(request.subject, "viewer");
        assert_eq!(request.permissions, vec!["read:database"]);
    }

    #[test]
    fn test_capability_token_request_builder() {
        let request = CapabilityTokenRequest::standard("user", &["read:data".to_string()])
            .with_claim("department", "engineering")
            .with_context("development")
            .with_expiry(1234567890);

        assert_eq!(
            request.claims.get("department"),
            Some(&"engineering".to_string())
        );
        assert_eq!(request.context, Some("development".to_string()));
        assert_eq!(request.expires_at, Some(1234567890));
    }

    #[test]
    fn test_verification_result_expiration_times() {
        let result = CapabilityVerificationResult {
            valid: true,
            permissions: vec![],
            expires_at: Some(1640999400), // 70 minutes after epoch start
            subject: None,
            issuer: None,
            claims: HashMap::new(),
            errors: vec![],
            verification_level: VerificationLevel::Standard,
        };

        // Test remaining time calculation
        let current_time_ms = 1640995800000; // 1 hour after epoch start
        assert_eq!(
            result.remaining_validity_seconds(current_time_ms),
            Some(3600)
        ); // 1 hour remaining
    }

    #[test]
    fn test_verification_result_validation() {
        let result = CapabilityVerificationResult {
            valid: true,
            permissions: vec!["read:data".to_string(), "write:logs".to_string()],
            expires_at: Some(9999999999), // Far future in seconds
            subject: Some("user".to_string()),
            issuer: Some("auth_service".to_string()),
            claims: HashMap::new(),
            errors: vec![],
            verification_level: VerificationLevel::Standard,
        };

        assert!(result.is_valid());
        // Test with current time well before expiration (using milliseconds)
        let current_time_ms = 1640995200000; // 2022-01-01 in milliseconds
        assert!(result.is_valid_at(current_time_ms));

        assert!(result.has_permissions(&["read:data".to_string()]));
        assert!(result.has_permissions(&["read:data".to_string(), "write:logs".to_string()]));
        assert!(!result.has_permissions(&["admin:all".to_string()]));

        // Test expiration
        let far_future_ms = 99999999990000; // Beyond expiration
        assert!(!result.is_valid_at(far_future_ms));
    }

    #[test]
    fn test_token_status_checks() {
        assert!(TokenStatus::Active.is_active());
        assert!(!TokenStatus::Expired.is_active());
        assert!(!TokenStatus::Revoked.is_active());

        assert!(TokenStatus::Expired.is_permanently_invalid());
        assert!(TokenStatus::Revoked.is_permanently_invalid());
        assert!(!TokenStatus::Active.is_permanently_invalid());
        assert!(!TokenStatus::Suspended.is_permanently_invalid());
    }

    #[test]
    fn test_capability_config_defaults() {
        let config = CapabilityConfig::default();

        assert_eq!(config.default_format, CapabilityTokenFormat::Biscuit);
        assert_eq!(config.default_expiry_seconds, 3600);
        assert!(config.strict_verification);
        assert!(config.enable_revocation_checks);
        assert_eq!(config.max_permissions_per_token, 100);
    }

    #[test]
    fn test_capability_statistics_defaults() {
        let stats = CapabilityStatistics::default();

        assert_eq!(stats.total_tokens_created, 0);
        assert_eq!(stats.total_verifications, 0);
        assert_eq!(stats.successful_verifications, 0);
        assert!(stats.common_permissions.is_empty());
        assert_eq!(stats.creation_rate_per_day, 0.0);
    }
}
