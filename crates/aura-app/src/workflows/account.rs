//! Account Workflow - Portable Business Logic
//!
//! This module contains account and threshold validation operations that are
//! portable across all frontends. It follows the reactive signal pattern.
//!
//! ## Account Backup
//!
//! Account backup operations (encode/decode/validate) are portable business
//! logic. The actual file I/O for export/import remains in aura-terminal.
mod bootstrap;
mod validation;

pub use bootstrap::{
    finalize_runtime_account_bootstrap, has_runtime_account_config,
    has_runtime_bootstrapped_account, initialize_runtime_account,
    reconcile_pending_runtime_account_bootstrap, PendingRuntimeBootstrapAction,
    PendingRuntimeBootstrapResolution,
};
pub use validation::{
    can_submit_account_setup, derive_recovered_context_id, is_valid_nickname_suggestion,
    parse_backup_code, prepare_pending_account_bootstrap, validate_nickname_suggestion,
    validate_threshold_compatibility, validate_threshold_params, NicknameSuggestionError,
    ThresholdConfig, MAX_NICKNAME_SUGGESTION_LENGTH, MIN_NICKNAME_SUGGESTION_LENGTH,
};

#[cfg(test)]
mod tests {
    use super::*;
    use aura_core::AuthorityId;

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
    fn test_derive_recovered_context_id() {
        let authority = AuthorityId::new_from_entropy([7u8; 32]);
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
        assert_eq!(
            validate_nickname_suggestion("Bob Smith").unwrap(),
            "Bob Smith"
        );
        assert_eq!(
            validate_nickname_suggestion("  Trimmed  ").unwrap(),
            "Trimmed"
        );
    }

    #[test]
    fn test_validate_nickname_suggestion_empty() {
        assert_eq!(
            validate_nickname_suggestion(""),
            Err(NicknameSuggestionError::Empty)
        );
        assert_eq!(
            validate_nickname_suggestion("   "),
            Err(NicknameSuggestionError::Empty)
        );
        assert_eq!(
            validate_nickname_suggestion("\t\n"),
            Err(NicknameSuggestionError::Empty)
        );
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
