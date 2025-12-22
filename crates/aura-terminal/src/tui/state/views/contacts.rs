//! Contacts screen view state

use super::PanelFocus;

use super::invitations::{
    CreateInvitationModalState, ImportInvitationModalState, InvitationCodeModalState,
};

/// Contacts screen state
#[derive(Clone, Debug, Default)]
pub struct ContactsViewState {
    /// Panel focus (list or detail)
    pub focus: PanelFocus,
    /// Selected contact index
    pub selected_index: usize,
    /// Total contact count (for wrap-around navigation)
    pub contact_count: usize,
    /// Filter text
    pub filter: String,
    /// Nickname edit modal state
    pub nickname_modal: NicknameModalState,
    /// Import invitation modal state (accept an invitation code)
    pub import_modal: ImportInvitationModalState,
    /// Create invitation modal state (send an invitation)
    pub create_modal: CreateInvitationModalState,
    /// Invitation code display modal state (show generated code)
    pub code_modal: InvitationCodeModalState,
    /// Demo mode: Alice's invitation code (for Ctrl+a shortcut)
    pub demo_alice_code: String,
    /// Demo mode: Carol's invitation code (for Ctrl+l shortcut)
    pub demo_carol_code: String,
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
    /// Error message if any
    pub error: Option<String>,
}

impl NicknameModalState {
    /// Create initialized state for editing a contact's nickname
    pub fn for_contact(contact_id: &str, current_name: &str) -> Self {
        Self {
            contact_id: contact_id.to_string(),
            value: current_name.to_string(),
            error: None,
        }
    }

    /// Reset state (called when dismissed)
    pub fn reset(&mut self) {
        self.contact_id.clear();
        self.value.clear();
        self.error = None;
    }

    pub fn can_submit(&self) -> bool {
        // Allow empty nicknames as "clear nickname" so the suggested name can
        // become visible again.
        !self.contact_id.trim().is_empty() && self.value.trim().len() <= 100
    }
}
