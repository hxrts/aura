//! # Invitation Code Modal
//!
//! Modal for displaying shareable invitation codes.
//! Uses the shared CodeDisplayModal component.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::components::{CodeDisplayModal, CodeDisplayStatus};

/// Callback type for modal close
pub type CloseCallback = Arc<dyn Fn() + Send + Sync>;

/// Props for InvitationCodeModal
#[derive(Default, Props)]
pub struct InvitationCodeModalProps {
    /// Whether the modal is visible
    pub visible: bool,
    /// The invitation code to display
    pub code: String,
    /// The invitation type (for display)
    pub invitation_type: String,
    /// Callback when closing the modal
    pub on_close: Option<CloseCallback>,
    /// Whether code was copied to clipboard
    pub copied: bool,
}

/// Modal for displaying shareable invitation codes
#[component]
pub fn InvitationCodeModal(props: &InvitationCodeModalProps) -> impl Into<AnyElement<'static>> {
    element! {
        CodeDisplayModal(
            visible: props.visible,
            title: "Invitation Created".to_string(),
            subtitle: format!("Type: {}", props.invitation_type),
            status: CodeDisplayStatus::Success,
            status_text: String::new(),
            instruction: "Share this code with the recipient:".to_string(),
            code: props.code.clone(),
            help_text: "The recipient can import this code to accept your invitation.".to_string(),
            copied: props.copied,
        )
    }
}

/// State for invitation code modal
#[derive(Clone, Debug, Default)]
pub struct InvitationCodeState {
    /// Whether the modal is visible
    pub visible: bool,
    /// The invitation code
    pub code: String,
    /// The invitation type
    pub invitation_type: String,
    /// The invitation ID (for reference)
    pub invitation_id: String,
    /// Whether code was copied to clipboard
    pub copied: bool,
}

impl InvitationCodeState {
    /// Create a new invitation code state
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the modal with a code
    pub fn show(&mut self, invitation_id: String, invitation_type: String, code: String) {
        self.visible = true;
        self.invitation_id = invitation_id;
        self.invitation_type = invitation_type;
        self.code = code;
        self.copied = false;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
        self.code.clear();
        self.invitation_type.clear();
        self.invitation_id.clear();
        self.copied = false;
    }

    /// Mark code as copied
    pub fn set_copied(&mut self) {
        self.copied = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invitation_code_state() {
        let mut state = InvitationCodeState::new();
        assert!(!state.visible);
        assert!(state.code.is_empty());

        state.show(
            "inv-123".to_string(),
            "Contact".to_string(),
            "AURA-INV-abc123".to_string(),
        );
        assert!(state.visible);
        assert_eq!(state.code, "AURA-INV-abc123");
        assert_eq!(state.invitation_type, "Contact");
        assert_eq!(state.invitation_id, "inv-123");
        assert!(!state.copied);

        state.set_copied();
        assert!(state.copied);

        state.hide();
        assert!(!state.visible);
        assert!(state.code.is_empty());
        assert!(!state.copied);
    }
}
