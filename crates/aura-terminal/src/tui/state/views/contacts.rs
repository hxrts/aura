//! Contacts screen view state

use crate::tui::navigation::TwoPanelFocus;
use crate::tui::state::form::{Validatable, ValidationError};

/// Focus within the contacts list column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ContactsListFocus {
    /// Focus on LAN-discovered peers list.
    LanPeers,
    /// Focus on saved contacts list.
    #[default]
    Contacts,
}

impl ContactsListFocus {
    /// Toggle between LAN peers and contacts.
    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::LanPeers => Self::Contacts,
            Self::Contacts => Self::LanPeers,
        }
    }

    #[must_use]
    pub fn is_lan(self) -> bool {
        matches!(self, Self::LanPeers)
    }

    #[must_use]
    pub fn is_contacts(self) -> bool {
        matches!(self, Self::Contacts)
    }
}

/// Contacts screen state
#[derive(Clone, Debug, Default)]
pub struct ContactsViewState {
    /// Panel focus (list or detail)
    pub focus: TwoPanelFocus,
    /// Focus within the list column (LAN peers vs contacts)
    pub list_focus: ContactsListFocus,
    /// Selected contact index
    pub selected_index: usize,
    /// Total contact count (for wrap-around navigation)
    pub contact_count: usize,
    /// Selected LAN peer index
    pub lan_selected_index: usize,
    /// Total LAN peer count (for wrap-around navigation)
    pub lan_peer_count: usize,
    /// Filter text
    pub filter: String,
    /// Demo mode: Alice's invitation code (for Ctrl+a shortcut)
    pub demo_alice_code: String,
    /// Demo mode: Carol's invitation code (for Ctrl+l shortcut)
    pub demo_carol_code: String,
    // Note: Modal state is now stored in ModalQueue, not here.
    // Use modal_queue.enqueue(QueuedModal::ContactsNickname/Import/Create/Code(...)) to show modals.
}

/// State for nickname edit modal
///
/// Note: Visibility is controlled by ModalQueue, not a `visible` field.
#[derive(Clone, Debug, Default)]
pub struct NicknameModalState {
    /// Contact ID being edited
    pub contact_id: String,
    /// Current nickname value
    pub value: String,
    /// Nickname suggestion (shown as hint when editing)
    pub nickname_suggestion: Option<String>,
    /// Error message if any
    pub error: Option<String>,
}

impl NicknameModalState {
    /// Create initialized state for editing a contact's nickname
    #[must_use]
    pub fn for_contact(contact_id: &str, current_name: &str) -> Self {
        Self {
            contact_id: contact_id.to_string(),
            value: current_name.to_string(),
            nickname_suggestion: None,
            error: None,
        }
    }

    /// Set the nickname suggestion (shown as hint in the modal)
    #[must_use]
    pub fn with_suggestion(mut self, suggested: Option<String>) -> Self {
        self.nickname_suggestion = suggested;
        self
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.contact_id.clear();
        self.value.clear();
        self.nickname_suggestion = None;
        self.error = None;
    }

    #[must_use]
    pub fn can_submit(&self) -> bool {
        // Allow empty nicknames as "clear nickname" so the suggested name can
        // become visible again.
        !self.contact_id.trim().is_empty() && self.value.trim().len() <= 100
    }
}

// ============================================================================
// Form Data Types with Validation
// ============================================================================

/// Form data for nickname editing (portable, validatable)
#[derive(Clone, Debug, Default)]
pub struct NicknameFormData {
    /// Nickname value (can be empty to clear)
    pub value: String,
}

impl Validatable for NicknameFormData {
    fn validate(&self) -> Vec<ValidationError> {
        let mut errors = vec![];
        if self.value.len() > 100 {
            errors.push(ValidationError::too_long("nickname", 100));
        }
        errors
    }
}
