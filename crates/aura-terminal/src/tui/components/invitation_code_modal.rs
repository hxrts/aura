//! # Invitation Code Modal
//!
//! Modal for displaying shareable invitation codes.

use iocraft::prelude::*;
use std::sync::Arc;

use crate::tui::theme::Theme;

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
}

/// Modal for displaying shareable invitation codes
#[component]
pub fn InvitationCodeModal(props: &InvitationCodeModalProps) -> impl Into<AnyElement<'static>> {
    if !props.visible {
        return element! {
            View {}
        };
    }

    let code = props.code.clone();
    let invitation_type = props.invitation_type.clone();

    // Format the code for display - break into chunks for readability
    let formatted_code = if code.len() > 40 {
        // Break long codes into multiple lines
        code.chars()
            .collect::<Vec<_>>()
            .chunks(40)
            .map(|c| c.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        code.clone()
    };

    element! {
        View(
            position: Position::Absolute,
            width: 100pct,
            height: 100pct,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,

        ) {
            View(
                width: Percent(70.0),
                flex_direction: FlexDirection::Column,
                background_color: Theme::BG_MODAL,
                border_style: BorderStyle::Round,
                border_color: Theme::SUCCESS,
            ) {
                // Header
                View(
                    padding: 2,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Bottom,
                    border_color: Theme::BORDER,
                ) {
                    Text(
                        content: "✓ Invitation Created",
                        weight: Weight::Bold,
                        color: Theme::SUCCESS,
                    )
                    View(margin_top: 1) {
                        Text(
                            content: format!("Type: {}", invitation_type),
                            color: Theme::TEXT_MUTED,
                        )
                    }
                }

                // Code display
                View(padding: 2, flex_direction: FlexDirection::Column) {
                    View(margin_bottom: 1) {
                        Text(
                            content: "Share this code with the recipient:",
                            color: Theme::TEXT,
                        )
                    }

                    // Code box
                    View(
                        flex_direction: FlexDirection::Column,

                        border_style: BorderStyle::Round,
                        border_color: Theme::PRIMARY,
                        padding: 2,
                    ) {
                        Text(
                            content: formatted_code,
                            color: Theme::PRIMARY,
                            wrap: TextWrap::NoWrap,
                        )
                    }

                    View(margin_top: 2) {
                        Text(
                            content: "The recipient can import this code to accept your invitation.",
                            color: Theme::TEXT_MUTED,
                        )
                    }
                }

                // Footer
                View(
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::Center,
                    padding: 2,
                    border_style: BorderStyle::Single,
                    border_edges: Edges::Top,
                    border_color: Theme::BORDER,
                ) {
                    View(flex_direction: FlexDirection::Row, gap: 2) {
                        Text(content: "Esc", color: Theme::SECONDARY)
                        Text(content: "Close", color: Theme::TEXT_MUTED)
                    }
                }
            }
        }
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
}

impl InvitationCodeState {
    /// Create a new invitation code state
    pub fn new() -> Self {
        Self::default()
    }

    /// Show the modal with a code
    pub fn show(&mut self, invitation_id: String, invitation_type: String, code: String) {
        self.visible = true;
        self.invitation_id = invitation_id;
        self.invitation_type = invitation_type;
        self.code = code;
    }

    /// Hide the modal
    pub fn hide(&mut self) {
        self.visible = false;
        self.code.clear();
        self.invitation_type.clear();
        self.invitation_id.clear();
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

        state.hide();
        assert!(!state.visible);
        assert!(state.code.is_empty());
    }
}
