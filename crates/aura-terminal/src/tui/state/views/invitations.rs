//! Invitation modal view state
//!
//! Invitation codes are managed from the Contacts screen (workflow + modals),
//! but the modal state types are shared.

/// Focused field in create invitation modal
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CreateInvitationField {
    #[default]
    Type,
    Message,
    Ttl,
}

/// State for create invitation modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct CreateInvitationModalState {
    /// Receiver authority ID (required for creating an invitation)
    pub receiver_id: String,
    /// Best-effort receiver display name (for UI hints)
    pub receiver_name: String,
    /// Invitation type selection index (0=Guardian, 1=Contact, 2=Channel)
    pub type_index: usize,
    /// Optional message
    pub message: String,
    /// TTL in hours
    pub ttl_hours: u64,
    /// Currently focused field
    pub focused_field: CreateInvitationField,
    /// Error message if any
    pub error: Option<String>,
}

impl CreateInvitationModalState {
    /// TTL preset values in hours
    const TTL_PRESETS: [u64; 4] = [1, 24, 168, 720]; // 1h, 1d, 1w, 30d

    /// Create new modal state with defaults
    #[must_use]
    pub fn new() -> Self {
        Self {
            receiver_id: String::new(),
            receiver_name: String::new(),
            type_index: 0,
            message: String::new(),
            ttl_hours: 24, // Default 24 hours
            focused_field: CreateInvitationField::Type,
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
        self.focused_field = CreateInvitationField::Type;
        self.error = None;
    }

    /// Move focus to next field
    pub fn focus_next(&mut self) {
        self.focused_field = match self.focused_field {
            CreateInvitationField::Type => CreateInvitationField::Message,
            CreateInvitationField::Message => CreateInvitationField::Ttl,
            CreateInvitationField::Ttl => CreateInvitationField::Type,
        };
    }

    /// Move focus to previous field
    pub fn focus_prev(&mut self) {
        self.focused_field = match self.focused_field {
            CreateInvitationField::Type => CreateInvitationField::Ttl,
            CreateInvitationField::Message => CreateInvitationField::Type,
            CreateInvitationField::Ttl => CreateInvitationField::Message,
        };
    }

    /// Cycle type to next option
    pub fn type_next(&mut self) {
        self.type_index = (self.type_index + 1) % 3;
    }

    /// Cycle type to previous option
    pub fn type_prev(&mut self) {
        self.type_index = if self.type_index == 0 {
            2
        } else {
            self.type_index - 1
        };
    }

    /// Cycle TTL to next preset
    pub fn ttl_next(&mut self) {
        let current_idx = Self::TTL_PRESETS
            .iter()
            .position(|&h| h == self.ttl_hours)
            .unwrap_or(1); // Default to 24h index
        let next_idx = (current_idx + 1) % Self::TTL_PRESETS.len();
        self.ttl_hours = Self::TTL_PRESETS[next_idx];
    }

    /// Cycle TTL to previous preset
    pub fn ttl_prev(&mut self) {
        let current_idx = Self::TTL_PRESETS
            .iter()
            .position(|&h| h == self.ttl_hours)
            .unwrap_or(1); // Default to 24h index
        let prev_idx = if current_idx == 0 {
            Self::TTL_PRESETS.len() - 1
        } else {
            current_idx - 1
        };
        self.ttl_hours = Self::TTL_PRESETS[prev_idx];
    }

    #[must_use]
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
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create with pre-filled code
    #[must_use]
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

    #[must_use]
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
    /// Whether code was copied to clipboard
    pub copied: bool,
}

impl InvitationCodeModalState {
    /// Create initialized state for showing an invitation code
    #[must_use]
    pub fn for_invitation(invitation_id: &str) -> Self {
        Self {
            invitation_id: invitation_id.to_string(),
            code: String::new(),
            loading: true,
            error: None,
            copied: false,
        }
    }

    #[must_use]
    pub fn for_code(code: String) -> Self {
        Self {
            invitation_id: String::new(),
            code,
            loading: false,
            error: None,
            copied: false,
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

    /// Mark code as copied to clipboard
    pub fn set_copied(&mut self) {
        self.copied = true;
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.invitation_id.clear();
        self.code.clear();
        self.loading = false;
        self.error = None;
    }
}
