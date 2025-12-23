//! Invitation modal view state
//!
//! Invitation codes are managed from the Contacts screen (workflow + modals),
//! but the modal state types are shared.

/// State for create invitation modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct CreateInvitationModalState {
    /// Receiver authority ID (required for creating an invitation)
    pub receiver_id: String,
    /// Best-effort receiver display name (for UI hints)
    pub receiver_name: String,
    /// Invitation type selection index
    pub type_index: usize,
    /// Optional message
    pub message: String,
    /// TTL in hours
    pub ttl_hours: u64,
    /// Current step (0 = type, 1 = message, 2 = ttl)
    pub step: usize,
    /// Error message if any
    pub error: Option<String>,
}

impl CreateInvitationModalState {
    /// Create new modal state with defaults
    pub fn new() -> Self {
        Self {
            receiver_id: String::new(),
            receiver_name: String::new(),
            type_index: 0,
            message: String::new(),
            ttl_hours: 24, // Default 24 hours
            step: 0,
            error: None,
        }
    }

    /// Create initialized state for a specific receiver.
    pub fn for_receiver(receiver_id: impl Into<String>, receiver_name: impl Into<String>) -> Self {
        Self {
            receiver_id: receiver_id.into(),
            receiver_name: receiver_name.into(),
            ..Self::new()
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.receiver_id.clear();
        self.receiver_name.clear();
        self.type_index = 0;
        self.message.clear();
        self.ttl_hours = 24;
        self.step = 0;
        self.error = None;
    }

    pub fn next_step(&mut self) {
        if self.step < 2 {
            self.step += 1;
        }
    }

    pub fn prev_step(&mut self) {
        if self.step > 0 {
            self.step -= 1;
        }
    }

    pub fn ttl_secs(&self) -> Option<u64> {
        if self.ttl_hours == 0 {
            None
        } else {
            Some(self.ttl_hours.saturating_mul(3600))
        }
    }
}

/// State for import invitation modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct ImportInvitationModalState {
    /// Code input buffer
    pub code: String,
    /// Error message if any
    pub error: Option<String>,
    /// Whether import is in progress
    pub importing: bool,
}

impl ImportInvitationModalState {
    /// Create new modal state
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with pre-filled code
    pub fn with_code(code: &str) -> Self {
        Self {
            code: code.to_string(),
            error: None,
            importing: false,
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.code.clear();
        self.error = None;
        self.importing = false;
    }

    pub fn can_submit(&self) -> bool {
        !self.code.trim().is_empty() && !self.importing
    }
}

/// State for invitation code display modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct InvitationCodeModalState {
    /// Invitation ID
    pub invitation_id: String,
    /// The code to display
    pub code: String,
    /// Whether code is loading
    pub loading: bool,
    /// Error message if any
    pub error: Option<String>,
}

impl InvitationCodeModalState {
    /// Create initialized state for showing an invitation code
    pub fn for_invitation(invitation_id: &str) -> Self {
        Self {
            invitation_id: invitation_id.to_string(),
            code: String::new(),
            loading: true,
            error: None,
        }
    }

    pub fn for_code(code: String) -> Self {
        Self {
            invitation_id: String::new(),
            code,
            loading: false,
            error: None,
        }
    }

    pub fn set_code(&mut self, code: String) {
        self.code = code;
        self.loading = false;
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.invitation_id.clear();
        self.code.clear();
        self.loading = false;
        self.error = None;
    }
}
