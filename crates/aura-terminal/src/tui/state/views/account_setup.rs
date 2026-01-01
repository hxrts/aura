//! Account setup modal state

use aura_app::ui::prelude::*;

// Re-export portable validation for callers that only import this module
pub use aura_app::ui::types::{
    is_valid_display_name, validate_display_name, DisplayNameError,
    MAX_DISPLAY_NAME_LENGTH, MIN_DISPLAY_NAME_LENGTH,
};

/// State for account setup modal
#[derive(Clone, Debug, Default)]
pub struct AccountSetupModalState {
    /// Current display name input
    pub display_name: String,
    /// Whether account creation is in progress
    pub creating: bool,
    /// Whether account was created successfully
    pub success: bool,
    /// Error message if creation failed
    pub error: Option<String>,
}

impl AccountSetupModalState {
    /// Whether we can submit the form.
    /// Uses portable validation from aura-app.
    #[must_use]
    pub fn can_submit(&self) -> bool {
        can_submit_account_setup(&self.display_name, self.creating, self.success)
    }

    /// Start the creating state.
    pub fn start_creating(&mut self) {
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
}
