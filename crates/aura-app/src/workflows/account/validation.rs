use crate::views::PendingAccountBootstrap;
use aura_core::types::identifiers::{AuthorityId, ContextId};
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
                write!(
                    f,
                    "Nickname suggestion too long: {length} characters (max {max})"
                )
            }
            Self::InvalidChars { reason } => {
                write!(
                    f,
                    "Nickname suggestion contains invalid characters: {reason}"
                )
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

    if trimmed.is_empty() {
        return Err(NicknameSuggestionError::Empty);
    }

    if trimmed.len() > MAX_NICKNAME_SUGGESTION_LENGTH {
        return Err(NicknameSuggestionError::TooLong {
            length: trimmed.len(),
            max: MAX_NICKNAME_SUGGESTION_LENGTH,
        });
    }

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
#[must_use]
pub fn can_submit_account_setup(
    nickname_suggestion: &str,
    is_creating: bool,
    is_success: bool,
) -> bool {
    is_valid_nickname_suggestion(nickname_suggestion) && !is_creating && !is_success
}

/// Prepare typed first-run bootstrap metadata from nickname input.
pub fn prepare_pending_account_bootstrap(
    nickname_suggestion: &str,
) -> Result<PendingAccountBootstrap, AuraError> {
    let nickname_suggestion = validate_nickname_suggestion(nickname_suggestion)
        .map_err(|error| AuraError::invalid(error.to_string()))?;
    Ok(PendingAccountBootstrap::new(nickname_suggestion))
}

/// Derive a context ID for a recovered authority.
pub fn derive_recovered_context_id(recovered_authority: &AuthorityId) -> ContextId {
    let authority_bytes = recovered_authority.to_bytes();
    let entropy = aura_core::hash::hash(
        format!("context:recovered:{}", hex::encode(authority_bytes)).as_bytes(),
    );
    ContextId::new_from_entropy(entropy)
}

/// Parse and validate a backup code, returning the decoded backup.
pub fn parse_backup_code(
    backup_code: &str,
) -> Result<crate::views::account::AccountBackup, AuraError> {
    let backup =
        crate::views::account::AccountBackup::decode(backup_code).map_err(AuraError::invalid)?;
    backup.validate().map_err(AuraError::invalid)?;
    Ok(backup)
}
