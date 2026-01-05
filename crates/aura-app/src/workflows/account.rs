//! Account Workflow - Portable Business Logic
//!
//! This module contains account and threshold validation operations that are
//! portable across all frontends. It follows the reactive signal pattern.
//!
//! ## ID Derivation
//!
//! Authority and Context IDs can be deterministically derived from a device ID
//! string. This ensures the same device_id always produces the same account.
//!
//! ## Account Backup
//!
//! Account backup operations (encode/decode/validate) are portable business
//! logic. The actual file I/O for export/import remains in aura-terminal.

use aura_core::identifiers::{AuthorityId, ContextId};
use aura_core::AuraError;

/// Threshold configuration for account setup
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThresholdConfig {
    /// Number of shares required to reconstruct (k in k-of-n)
    pub threshold: u32,
    /// Total number of devices/shares (n in k-of-n)
    pub num_devices: u32,
}

impl ThresholdConfig {
    /// Create a new threshold configuration
    pub fn new(threshold: u32, num_devices: u32) -> Self {
        Self {
            threshold,
            num_devices,
        }
    }

    /// Validate the threshold configuration
    ///
    /// Returns Ok(()) if valid, Err with descriptive message otherwise.
    ///
    /// # Validation Rules
    /// - Threshold must be greater than 0
    /// - Threshold must not exceed num_devices
    /// - num_devices must be greater than 0
    pub fn validate(&self) -> Result<(), AuraError> {
        if self.num_devices == 0 {
            return Err(AuraError::invalid(
                "Invalid threshold configuration: num_devices must be greater than 0",
            ));
        }

        if self.threshold == 0 {
            return Err(AuraError::invalid(
                "Invalid threshold configuration: threshold must be greater than 0",
            ));
        }

        if self.threshold > self.num_devices {
            return Err(AuraError::invalid(format!(
                "Invalid threshold configuration: threshold ({}) cannot exceed num_devices ({})",
                self.threshold, self.num_devices
            )));
        }

        Ok(())
    }

    /// Check if this is a single-device configuration (1-of-1)
    pub fn is_single_device(&self) -> bool {
        self.threshold == 1 && self.num_devices == 1
    }

    /// Get a display string like "2-of-3"
    pub fn display_string(&self) -> String {
        format!("{}-of-{}", self.threshold, self.num_devices)
    }
}

/// Validate threshold parameters for account initialization
///
/// This is the canonical validation function for threshold configurations.
/// All frontends should use this instead of implementing their own validation.
///
/// # Arguments
/// * `threshold` - Number of shares required to reconstruct
/// * `num_devices` - Total number of devices/shares
///
/// # Returns
/// * `Ok(ThresholdConfig)` if valid
/// * `Err(AuraError)` with descriptive message if invalid
pub fn validate_threshold_params(
    threshold: u32,
    num_devices: u32,
) -> Result<ThresholdConfig, AuraError> {
    let config = ThresholdConfig::new(threshold, num_devices);
    config.validate()?;
    Ok(config)
}

/// Validate that a set of threshold configs are compatible
///
/// All configs must have matching threshold values for multi-device operations.
///
/// # Arguments
/// * `configs` - Slice of (identifier, threshold) pairs
///
/// # Returns
/// * `Ok(())` if all configs are compatible
/// * `Err(AuraError)` if there's a mismatch
pub fn validate_threshold_compatibility(configs: &[(&str, u32)]) -> Result<(), AuraError> {
    if configs.is_empty() {
        return Err(AuraError::invalid(
            "No configurations provided for threshold validation",
        ));
    }

    let expected = configs[0].1;
    for (identifier, threshold) in configs.iter().skip(1) {
        if *threshold != expected {
            return Err(AuraError::invalid(format!(
                "Threshold mismatch in {identifier}: expected {expected}, got {threshold}"
            )));
        }
    }

    Ok(())
}

// ============================================================================
// Nickname Suggestion Validation
// ============================================================================

/// Maximum allowed length for a nickname suggestion.
pub const MAX_NICKNAME_SUGGESTION_LENGTH: usize = 64;

/// Minimum allowed length for a nickname suggestion.
pub const MIN_NICKNAME_SUGGESTION_LENGTH: usize = 1;

/// Nickname suggestion validation error types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NicknameSuggestionError {
    /// Nickname suggestion is empty or whitespace-only
    Empty,
    /// Nickname suggestion exceeds maximum length
    TooLong {
        /// Actual length
        length: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Nickname suggestion contains invalid characters
    InvalidChars {
        /// Description of the issue
        reason: String,
    },
}

impl std::fmt::Display for NicknameSuggestionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "Nickname suggestion cannot be empty"),
            Self::TooLong { length, max } => {
                write!(f, "Nickname suggestion too long: {length} characters (max {max})")
            }
            Self::InvalidChars { reason } => {
                write!(f, "Nickname suggestion contains invalid characters: {reason}")
            }
        }
    }
}

impl std::error::Error for NicknameSuggestionError {}

/// Validate a nickname suggestion for account setup.
///
/// # Arguments
/// * `name` - The nickname suggestion to validate
///
/// # Returns
/// * `Ok(String)` - The trimmed, validated nickname suggestion
/// * `Err(NicknameSuggestionError)` - If validation fails
///
/// # Validation Rules
/// - Must not be empty or whitespace-only
/// - Must not exceed `MAX_NICKNAME_SUGGESTION_LENGTH` characters
/// - Must not contain control characters
///
/// # Examples
/// ```rust
/// use aura_app::ui::workflows::account::validate_nickname_suggestion;
///
/// assert!(validate_nickname_suggestion("Alice").is_ok());
/// assert!(validate_nickname_suggestion("").is_err());
/// assert!(validate_nickname_suggestion("   ").is_err());
/// ```
pub fn validate_nickname_suggestion(name: &str) -> Result<String, NicknameSuggestionError> {
    let trimmed = name.trim();

    // Check for empty
    if trimmed.is_empty() {
        return Err(NicknameSuggestionError::Empty);
    }

    // Check length
    if trimmed.len() > MAX_NICKNAME_SUGGESTION_LENGTH {
        return Err(NicknameSuggestionError::TooLong {
            length: trimmed.len(),
            max: MAX_NICKNAME_SUGGESTION_LENGTH,
        });
    }

    // Check for control characters (excluding normal whitespace)
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(NicknameSuggestionError::InvalidChars {
            reason: "control characters not allowed".to_string(),
        });
    }

    Ok(trimmed.to_string())
}

/// Check if a nickname suggestion is valid without returning the trimmed value.
///
/// Convenience function for form validation that just needs a boolean.
///
/// # Examples
/// ```rust
/// use aura_app::ui::workflows::account::is_valid_nickname_suggestion;
///
/// assert!(is_valid_nickname_suggestion("Alice"));
/// assert!(!is_valid_nickname_suggestion(""));
/// ```
#[must_use]
pub fn is_valid_nickname_suggestion(name: &str) -> bool {
    validate_nickname_suggestion(name).is_ok()
}

/// Check if a form can be submitted based on nickname suggestion.
///
/// This mirrors the TUI's `can_submit()` logic for portable use.
///
/// # Arguments
/// * `nickname_suggestion` - The current nickname suggestion input
/// * `is_creating` - Whether creation is already in progress
/// * `is_success` - Whether creation already succeeded
///
/// # Returns
/// `true` if the form can be submitted
#[must_use]
pub fn can_submit_account_setup(nickname_suggestion: &str, is_creating: bool, is_success: bool) -> bool {
    is_valid_nickname_suggestion(nickname_suggestion) && !is_creating && !is_success
}

// =============================================================================
// ID Derivation Functions
// =============================================================================

/// Derive an authority ID deterministically from a device ID string
///
/// This ensures the same device_id always creates the same authority.
/// The derivation uses a hash of `"authority:{device_id}"`.
///
/// # Arguments
/// * `device_id` - The device identifier string
///
/// # Returns
/// * A deterministically derived `AuthorityId`
///
/// # Example
/// ```
/// use aura_app::ui::workflows::account::derive_authority_id;
///
/// let id1 = derive_authority_id("my-device");
/// let id2 = derive_authority_id("my-device");
/// assert_eq!(id1, id2); // Same input -> same output
/// ```
pub fn derive_authority_id(device_id: &str) -> AuthorityId {
    let entropy = aura_core::hash::hash(format!("authority:{device_id}").as_bytes());
    AuthorityId::new_from_entropy(entropy)
}

/// Derive a context ID deterministically from a device ID string
///
/// This ensures the same device_id always creates the same context.
/// The derivation uses a hash of `"context:{device_id}"`.
///
/// # Arguments
/// * `device_id` - The device identifier string
///
/// # Returns
/// * A deterministically derived `ContextId`
///
/// # Example
/// ```
/// use aura_app::ui::workflows::account::derive_context_id;
///
/// let id1 = derive_context_id("my-device");
/// let id2 = derive_context_id("my-device");
/// assert_eq!(id1, id2); // Same input -> same output
/// ```
pub fn derive_context_id(device_id: &str) -> ContextId {
    let entropy = aura_core::hash::hash(format!("context:{device_id}").as_bytes());
    ContextId::new_from_entropy(entropy)
}

/// Derive a context ID for a recovered authority
///
/// Used during guardian-based recovery when the original context_id
/// is not available. Derives deterministically from the recovered authority.
///
/// # Arguments
/// * `recovered_authority` - The authority ID being recovered
///
/// # Returns
/// * A deterministically derived `ContextId`
pub fn derive_recovered_context_id(recovered_authority: &AuthorityId) -> ContextId {
    let authority_bytes = recovered_authority.to_bytes();
    let entropy = aura_core::hash::hash(
        format!("context:recovered:{}", hex::encode(authority_bytes)).as_bytes(),
    );
    ContextId::new_from_entropy(entropy)
}

/// Parse and validate a backup code, returning the decoded backup
///
/// This is a convenience function that combines decoding and validation.
///
/// # Arguments
/// * `backup_code` - The backup code string (format: `aura:backup:v1:<base64>`)
///
/// # Returns
/// * `Ok(AccountBackup)` if valid
/// * `Err(AuraError)` with descriptive message if invalid
pub fn parse_backup_code(
    backup_code: &str,
) -> Result<crate::views::account::AccountBackup, AuraError> {
    let backup =
        crate::views::account::AccountBackup::decode(backup_code).map_err(AuraError::invalid)?;

    backup.validate().map_err(AuraError::invalid)?;

    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_threshold_config_valid() {
        let config = ThresholdConfig::new(2, 3);
        assert!(config.validate().is_ok());
        assert_eq!(config.display_string(), "2-of-3");
        assert!(!config.is_single_device());
    }

    #[test]
    fn test_threshold_config_single_device() {
        let config = ThresholdConfig::new(1, 1);
        assert!(config.validate().is_ok());
        assert!(config.is_single_device());
    }

    #[test]
    fn test_threshold_config_threshold_zero() {
        let config = ThresholdConfig::new(0, 3);
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("threshold must be greater than 0"));
    }

    #[test]
    fn test_threshold_config_num_devices_zero() {
        let config = ThresholdConfig::new(1, 0);
        let err = config.validate().unwrap_err();
        assert!(err
            .to_string()
            .contains("num_devices must be greater than 0"));
    }

    #[test]
    fn test_threshold_config_exceeds_devices() {
        let config = ThresholdConfig::new(5, 3);
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("cannot exceed"));
    }

    #[test]
    fn test_validate_threshold_params() {
        assert!(validate_threshold_params(2, 3).is_ok());
        assert!(validate_threshold_params(0, 3).is_err());
        assert!(validate_threshold_params(5, 3).is_err());
    }

    #[test]
    fn test_validate_threshold_compatibility() {
        // All same - should pass
        let configs = vec![("config1", 2), ("config2", 2), ("config3", 2)];
        assert!(validate_threshold_compatibility(&configs).is_ok());

        // Mismatch - should fail
        let configs = vec![("config1", 2), ("config2", 3)];
        let err = validate_threshold_compatibility(&configs).unwrap_err();
        assert!(err.to_string().contains("mismatch"));

        // Empty - should fail
        let configs: Vec<(&str, u32)> = vec![];
        assert!(validate_threshold_compatibility(&configs).is_err());
    }

    #[test]
    fn test_derive_authority_id_deterministic() {
        let id1 = derive_authority_id("test-device");
        let id2 = derive_authority_id("test-device");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_derive_authority_id_different_inputs() {
        let id1 = derive_authority_id("device-1");
        let id2 = derive_authority_id("device-2");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_derive_context_id_deterministic() {
        let id1 = derive_context_id("test-device");
        let id2 = derive_context_id("test-device");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_derive_context_id_different_from_authority() {
        let authority = derive_authority_id("test-device");
        let context = derive_context_id("test-device");
        // They should be different IDs (different prefixes in derivation)
        assert_ne!(authority.to_bytes(), context.to_bytes());
    }

    #[test]
    fn test_derive_recovered_context_id() {
        let authority = derive_authority_id("test-device");
        let context1 = derive_recovered_context_id(&authority);
        let context2 = derive_recovered_context_id(&authority);
        assert_eq!(context1, context2);
    }

    // =========================================================================
    // Nickname Suggestion Validation Tests
    // =========================================================================

    #[test]
    fn test_validate_nickname_suggestion_valid() {
        assert_eq!(validate_nickname_suggestion("Alice").unwrap(), "Alice");
        assert_eq!(validate_nickname_suggestion("Bob Smith").unwrap(), "Bob Smith");
        assert_eq!(validate_nickname_suggestion("  Trimmed  ").unwrap(), "Trimmed");
    }

    #[test]
    fn test_validate_nickname_suggestion_empty() {
        assert_eq!(validate_nickname_suggestion(""), Err(NicknameSuggestionError::Empty));
        assert_eq!(validate_nickname_suggestion("   "), Err(NicknameSuggestionError::Empty));
        assert_eq!(validate_nickname_suggestion("\t\n"), Err(NicknameSuggestionError::Empty));
    }

    #[test]
    fn test_validate_nickname_suggestion_too_long() {
        let long_name = "a".repeat(MAX_NICKNAME_SUGGESTION_LENGTH + 1);
        match validate_nickname_suggestion(&long_name) {
            Err(NicknameSuggestionError::TooLong { length, max }) => {
                assert_eq!(length, MAX_NICKNAME_SUGGESTION_LENGTH + 1);
                assert_eq!(max, MAX_NICKNAME_SUGGESTION_LENGTH);
            }
            other => panic!("Expected TooLong error, got {other:?}"),
        }
    }

    #[test]
    fn test_validate_nickname_suggestion_max_length_ok() {
        let max_name = "a".repeat(MAX_NICKNAME_SUGGESTION_LENGTH);
        assert!(validate_nickname_suggestion(&max_name).is_ok());
    }

    #[test]
    fn test_validate_nickname_suggestion_control_chars() {
        assert!(matches!(
            validate_nickname_suggestion("Alice\x00Bob"),
            Err(NicknameSuggestionError::InvalidChars { .. })
        ));
        assert!(matches!(
            validate_nickname_suggestion("Name\x07Bell"),
            Err(NicknameSuggestionError::InvalidChars { .. })
        ));
    }

    #[test]
    fn test_is_valid_nickname_suggestion() {
        assert!(is_valid_nickname_suggestion("Alice"));
        assert!(!is_valid_nickname_suggestion(""));
        assert!(!is_valid_nickname_suggestion("   "));
    }

    #[test]
    fn test_can_submit_account_setup() {
        // Valid name, not creating, not success -> can submit
        assert!(can_submit_account_setup("Alice", false, false));

        // Empty name -> cannot submit
        assert!(!can_submit_account_setup("", false, false));

        // Valid name but already creating -> cannot submit
        assert!(!can_submit_account_setup("Alice", true, false));

        // Valid name but already succeeded -> cannot submit
        assert!(!can_submit_account_setup("Alice", false, true));

        // Both flags set -> cannot submit
        assert!(!can_submit_account_setup("Alice", true, true));
    }

    #[test]
    fn test_nickname_suggestion_error_display() {
        assert_eq!(
            NicknameSuggestionError::Empty.to_string(),
            "Nickname suggestion cannot be empty"
        );
        assert_eq!(
            NicknameSuggestionError::TooLong {
                length: 100,
                max: 64
            }
            .to_string(),
            "Nickname suggestion too long: 100 characters (max 64)"
        );
        assert_eq!(
            NicknameSuggestionError::InvalidChars {
                reason: "control characters not allowed".to_string()
            }
            .to_string(),
            "Nickname suggestion contains invalid characters: control characters not allowed"
        );
    }
}
