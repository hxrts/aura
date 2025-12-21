//! Account setup modal state

/// State for account setup modal
#[derive(Clone, Debug, Default)]
pub struct AccountSetupModalState {
    /// Current display name input
    pub display_name: String,
    /// Whether account creation is in progress
    pub creating: bool,
    /// Timestamp (ms since epoch) when creating started - for debounced spinner
    pub creating_started_ms: Option<u64>,
    /// Whether account was created successfully
    pub success: bool,
    /// Error message if creation failed
    pub error: Option<String>,
}

/// Debounce threshold for showing spinner (ms)
pub const SPINNER_DEBOUNCE_MS: u64 = 300;

impl AccountSetupModalState {
    /// Whether we can submit the form
    pub fn can_submit(&self) -> bool {
        !self.display_name.trim().is_empty() && !self.creating && !self.success
    }

    /// Start the creating state with timestamp for debounced spinner
    pub fn start_creating(&mut self) {
        self.creating = true;
        self.creating_started_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        );
        self.error = None;
    }

    /// Check if spinner should be shown (creating AND elapsed > debounce threshold)
    pub fn should_show_spinner(&self) -> bool {
        if !self.creating {
            return false;
        }
        let Some(started) = self.creating_started_ms else {
            return false;
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        now.saturating_sub(started) >= SPINNER_DEBOUNCE_MS
    }

    /// Set success state
    pub fn set_success(&mut self) {
        self.creating = false;
        self.creating_started_ms = None;
        self.success = true;
    }

    /// Set error state
    pub fn set_error(&mut self, msg: String) {
        self.creating = false;
        self.creating_started_ms = None;
        self.error = Some(msg);
    }

    /// Reset to input state (for retry after error)
    pub fn reset_to_input(&mut self) {
        self.creating = false;
        self.creating_started_ms = None;
        self.success = false;
        self.error = None;
    }
}
