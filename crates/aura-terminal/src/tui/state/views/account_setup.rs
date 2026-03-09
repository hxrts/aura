//! Account setup modal state

use aura_app::ui::prelude::*;

// Re-export portable validation for callers that only import this module
pub use aura_app::ui::types::{
    is_valid_nickname_suggestion, validate_nickname_suggestion, NicknameSuggestionError,
    MAX_NICKNAME_SUGGESTION_LENGTH, MIN_NICKNAME_SUGGESTION_LENGTH,
};

/// State for account setup modal
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum AccountSetupField {
    #[default]
    AccountName,
    DeviceImportCode,
}

/// State for account setup modal
#[derive(Clone, Debug, Default)]
pub struct AccountSetupModalState {
    /// Current nickname suggestion input
    pub nickname_suggestion: String,
    /// Current device import code input
    pub device_import_code: String,
    /// Currently focused onboarding field
    pub active_field: AccountSetupField,
    /// Whether account creation is in progress
    pub creating: bool,
    /// Whether account was created successfully
    pub success: bool,
    /// Error message if creation failed
    pub error: Option<String>,
}

impl AccountSetupModalState {
    /// Whether we can create an account with the current input.
    /// Uses portable validation from aura-app.
    #[must_use]
    pub fn can_create_account(&self) -> bool {
        can_submit_account_setup(&self.nickname_suggestion, self.creating, self.success)
    }

    /// Whether we can import a device enrollment code with the current input.
    #[must_use]
    pub fn can_import_device(&self) -> bool {
        !self.device_import_code.trim().is_empty() && !self.creating && !self.success
    }

    /// Start the creating state.
    pub fn start_submitting(&mut self) {
        self.creating = true;
        self.error = None;
    }

    /// Check if spinner should be shown.
    ///
    /// We intentionally avoid reading OS time directly from the TUI state.
    /// Runtime-backed operations should report progress via signals.
    #[must_use]
    pub fn should_show_spinner(&self) -> bool {
        self.creating
    }

    /// Set success state
    pub fn set_success(&mut self) {
        self.creating = false;
        self.success = true;
    }

    /// Set error state
    pub fn set_error(&mut self, msg: String) {
        self.creating = false;
        self.error = Some(msg);
    }

    /// Reset to input state (for retry after error)
    pub fn reset_to_input(&mut self) {
        self.creating = false;
        self.success = false;
        self.error = None;
    }

    /// Advance focus between onboarding fields.
    pub fn focus_next_field(&mut self) {
        self.active_field = match self.active_field {
            AccountSetupField::AccountName => AccountSetupField::DeviceImportCode,
            AccountSetupField::DeviceImportCode => AccountSetupField::AccountName,
        };
    }

    /// Focus the account name field.
    pub fn focus_account_name(&mut self) {
        self.active_field = AccountSetupField::AccountName;
    }

    /// Focus the device import code field.
    pub fn focus_device_import_code(&mut self) {
        self.active_field = AccountSetupField::DeviceImportCode;
    }
}
